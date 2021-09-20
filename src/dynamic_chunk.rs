use crate::dynamic_array::DynamicArray;
use crate::sync::{Ordering, AtomicPtr, AtomicUsize, AtomicU64};
use crate::event_queue::EventQueue;
use std::mem::MaybeUninit;
use std::ptr::{null, null_mut};
use crate::len_and_epoch::LenAndEpoch;
use std::ptr;

/// Error, indicating insufficient capacity
pub struct CapacityError<V>{
    pub value: V,
}

struct Header<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>{
    /// Just to compare chunks by age/sequence fast. Brings order.
    /// Will overflow after years... So just ignore that possibility.
    pub(super) id      : usize,
    pub(super) next    : AtomicPtr<DynamicChunk<T, CHUNK_SIZE, AUTO_CLEANUP>>,

    /// When == readers count, it is safe to delete this chunk.
    /// Chunk read completely if reader consumed CHUNK_SIZE'ed element.
    /// Last chunk always exists
    pub(super) read_completely_times : AtomicUsize,

    // This needed to access Event from EventReader.
    // Never changes.
    pub(super) event : *const EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>,

    /// LenAndEpoch. Epoch same across all chunks. Epoch updated in all chunks at [EventQueue::clear]
    /// len fused with epoch for optimization purposes. This allow to get start_position_epoch without
    /// touching EventQueue and without additional atomic load(acquire)
    len_and_start_position_epoch: AtomicU64,
}

#[repr(transparent)]
pub struct DynamicChunk<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>(
    DynamicArray< Header<T, CHUNK_SIZE, AUTO_CLEANUP>, T >
);

impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> DynamicChunk<T, CHUNK_SIZE, AUTO_CLEANUP>{
    #[inline]
    pub fn id(&self) -> usize{
        self.0.header().id
    }

    #[inline]
    pub fn next(&self) -> &AtomicPtr<Self>{
        &self.0.header().next
    }

    #[inline]
    pub fn read_completely_times(&self) -> &AtomicUsize{
        &self.0.header().read_completely_times
    }

    #[inline]
    pub fn event(&self) -> &EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>{
        unsafe { &*self.0.header().event }
    }

    #[inline]
    pub fn set_event(&mut self, event: &EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>) {
        self.0.header_mut().event = event;
    }

// ----------------------------------------------------------------
//                      STORAGE
// ----------------------------------------------------------------
    pub fn construct(
        id: usize,
        epoch: u32,
        event : *const EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>,
        len: usize
    ) -> *mut Self{
        let header = Header{
            id,
            next: AtomicPtr::new(null_mut()),
            read_completely_times: AtomicUsize::new(0),
            event,
            len_and_start_position_epoch: AtomicU64::new(LenAndEpoch::new(0, epoch).into())
        };
        unsafe{
            let this = DynamicArray::<Header<T, CHUNK_SIZE, AUTO_CLEANUP>, T>::construct_uninit(
                header,
                len
            );

            // This is ok, due to transparent
             this as *mut _ as *mut Self
        }
    }

    #[inline]
    pub fn set_epoch(&mut self, epoch: u32, load_ordering: Ordering, store_ordering: Ordering){
        let len = self.len_and_epoch(load_ordering).len();
        self.0.header().len_and_start_position_epoch.store(
            LenAndEpoch::new(len, epoch).into(),
            store_ordering
        );
    }

    /// Needs additional synchronization, because several threads writing simultaneously may finish writes
    /// not in order, but len increases sequentially. This may cause items before len index being not fully written.
    #[inline(always)]
    pub fn try_push(&mut self, value: T, store_ordering: Ordering) -> Result<(), CapacityError<T>>{
        // Relaxed because updated only with &mut self
        let len_and_epoch: LenAndEpoch = self.len_and_epoch(Ordering::Relaxed);
        let index = len_and_epoch.len();
        let epoch = len_and_epoch.epoch();
        if (index as usize) >= self.capacity() {
            return Result::Err(CapacityError{value});
        }

        unsafe{ self.push_at(value, index, epoch, store_ordering); }

        return Result::Ok(());
    }

    #[inline(always)]
    pub unsafe fn push_unchecked(&mut self, value: T, store_ordering: Ordering){
        // Relaxed because updated only with &mut self
        let len_and_epoch: LenAndEpoch = self.len_and_epoch(Ordering::Relaxed);
        let index = len_and_epoch.len();
        let epoch = len_and_epoch.epoch();

        self.push_at(value, index, epoch, store_ordering);
    }

    #[inline(always)]
    pub unsafe fn push_at(&mut self, value: T, index: u32, epoch: u32, store_ordering: Ordering) {
        debug_assert!((index as usize) < CHUNK_SIZE);

        self.0.write_at(index as usize, value);

        self.0.header().len_and_start_position_epoch.store(
            LenAndEpoch::new(index+1, epoch).into(),
            store_ordering
        );
    }

    /// Append items from iterator, until have free space
    /// Returns Ok if everything fit, CapacityError() - if not
    pub fn extend<I>(&mut self, iter: &mut I, store_ordering: Ordering) -> Result<(), CapacityError<()>>
        where I:Iterator<Item = T>
    {
        let len_and_epoch: LenAndEpoch = self.len_and_epoch(Ordering::Relaxed);
        let epoch = len_and_epoch.epoch();
        let mut index = len_and_epoch.len() as usize;

        loop {
            if index == self.capacity(){
                self.0.header().len_and_start_position_epoch.store(
                    LenAndEpoch::new(self.capacity() as u32, epoch).into(),
                    store_ordering
                );
                return Result::Err(CapacityError{value:()});
            }

            match iter.next(){
                None => {
                    self.0.header().len_and_start_position_epoch.store(
                        LenAndEpoch::new(index as u32, epoch).into(),
                        store_ordering
                    );
                    return Result::Ok(());
                }
                Some(value) => {
                    unsafe{
                        self.0.write_at(index as usize, value);
                    }
                }
            }

            index+=1;
        }
    }

    #[inline(always)]
    pub unsafe fn get_unchecked(&self, index: usize) -> &T{
        self.0.slice().get_unchecked(index)
    }

    #[inline(always)]
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T{
        self.0.slice_mut().get_unchecked_mut(index)
    }

    #[inline(always)]
    pub fn len_and_epoch(&self, ordering: Ordering) -> LenAndEpoch {
        self.0.header().len_and_start_position_epoch.load(ordering).into()
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.0.len()
    }

    pub unsafe fn destruct(this: *mut Self){
        if std::mem::needs_drop::<T>() {
            let len = (*this).len_and_epoch(Ordering::Acquire).len() as usize;
            for i in 0..len {
                 unsafe{ ptr::drop_in_place((*this).0.slice_mut().get_unchecked_mut(i)); }
            }
        }

        DynamicArray::<Header<T, CHUNK_SIZE, AUTO_CLEANUP>, T>::destruct_uninit(
            this as *mut DynamicArray<Header<T, CHUNK_SIZE, AUTO_CLEANUP>, T>
        )
    }
}
