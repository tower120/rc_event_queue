use crate::dynamic_array::DynamicArray;
use crate::sync::{Ordering, AtomicPtr, AtomicUsize};
use crate::event_queue::{EventQueue, Settings};
use std::ptr::{null_mut, NonNull};
use std::ptr;
use crate::chunk_state::{AtomicPackedChunkState, ChunkState, PackedChunkState};
use crate::StartPositionEpoch;

/// Error, indicating insufficient capacity
pub struct CapacityError<V>{
    pub value: V,
}

struct Header<T, S: Settings>{
    /// Just to compare chunks by age/sequence fast. Brings order.
    /// Will overflow after years... So just ignore that possibility.
    pub(super) id      : usize,
    pub(super) next    : AtomicPtr<DynamicChunk<T, S>>,

    /// When == readers count, it is safe to delete this chunk.
    /// Chunk read completely if reader consumed CHUNK_SIZE'ed element.
    /// Last chunk always exists
    pub(super) read_completely_times : AtomicUsize,

    // This needed to access Event from EventReader.
    // Never changes.
    pub(super) event : *const EventQueue<T, S>,

    /// LenAndEpoch. Epoch same across all chunks. Epoch updated in all chunks at [EventQueue::clear]
    /// len fused with epoch for optimization purposes. This allow to get start_position_epoch without
    /// touching EventQueue and without additional atomic load(acquire)
    chunk_state: AtomicPackedChunkState,
}

#[repr(transparent)]
pub struct DynamicChunk<T, S: Settings>(
    DynamicArray< Header<T, S>, T >
);

impl<T, S: Settings> DynamicChunk<T, S>{
    #[inline]
    pub fn id(&self) -> usize{
        self.0.header().id
    }

    #[inline]
    pub fn next(&self, load_ordering: Ordering) -> *mut Self{
        self.0.header().next.load(load_ordering)
    }

    #[inline]
    pub fn set_next(&mut self, ptr: *mut Self, store_ordering: Ordering) {
        self.0.header().next.store(ptr, store_ordering);

        // Relaxed because &mut self
        let mut chunk_state = self.0.header().chunk_state.load(Ordering::Relaxed);
        chunk_state.set_has_next(!ptr.is_null());
        self.0.header().chunk_state.store(chunk_state, store_ordering);
    }

    #[inline]
    pub fn read_completely_times(&self) -> &AtomicUsize{
        &self.0.header().read_completely_times
    }

    #[inline]
    pub fn event(&self) -> &EventQueue<T, S>{
        unsafe { &*self.0.header().event }
    }

    pub fn construct(
        id: usize,
        epoch: StartPositionEpoch,
        event : *const EventQueue<T, S>,
        len: usize
    ) -> *mut Self{
        let header = Header{
            id,
            next: AtomicPtr::new(null_mut()),
            read_completely_times: AtomicUsize::new(0),
            event,
            chunk_state: AtomicPackedChunkState::new(
                PackedChunkState::pack(
                    ChunkState{len: 0, has_next: false, epoch}
                )
            )
        };
        unsafe{
            let this = DynamicArray::<Header<T, S>, T>::construct_uninit(
                header,
                len
            );

            // This is ok, due to transparent
            this as *mut _ as *mut Self
        }
    }

    /// Unsafe - because `this` state is unknown
    /// Reuse previously stored chunk.
    /// Should be used in deinitialize -> reinitialize cycle.
    pub unsafe fn from_recycled(
        mut recycled: DynamicChunkRecycled<T, S>,
        id: usize,
        epoch: StartPositionEpoch,
    ) -> *mut Self {
        let header = recycled.chunk.as_mut().0.header_mut();
        header.id = id;
        header.next = AtomicPtr::new(null_mut());
        header.read_completely_times = AtomicUsize::new(0);
        header.chunk_state = AtomicPackedChunkState::new(
            PackedChunkState::pack(
                ChunkState{len: 0, has_next: false, epoch}
            )
        );

        let ptr = recycled.chunk.as_ptr();
            std::mem::forget(recycled);
        ptr
    }

// ----------------------------------------------------------------
//                      STORAGE
// ----------------------------------------------------------------
    #[inline]
    pub fn set_epoch(&mut self, epoch: StartPositionEpoch, load_ordering: Ordering, store_ordering: Ordering){
        let mut chunk_state = self.chunk_state(load_ordering);
        chunk_state.set_epoch(epoch);

        self.0.header().chunk_state.store(chunk_state, store_ordering);
    }

    /// Needs additional synchronization, because several threads writing simultaneously may finish writes
    /// not in order, but len increases sequentially. This may cause items before len index being not fully written.
    #[inline(always)]
    pub fn try_push(&mut self, value: T, store_ordering: Ordering) -> Result<(), CapacityError<T>>{
        // Relaxed because updated only with &mut self
        let chunk_state = self.chunk_state(Ordering::Relaxed);
        let index = chunk_state.len();
        if (index as usize) >= self.capacity() {
            return Result::Err(CapacityError{value});
        }

        unsafe{ self.push_at(value, index, chunk_state, store_ordering); }

        return Result::Ok(());
    }

    #[inline(always)]
    pub unsafe fn push_unchecked(&mut self, value: T, store_ordering: Ordering){
        // Relaxed because updated only with &mut self
        let chunk_state = self.chunk_state(Ordering::Relaxed);
        let index = chunk_state.len();

        self.push_at(value, index, chunk_state, store_ordering);
    }

    #[inline(always)]
    unsafe fn push_at(&mut self, value: T, index: u32, mut chunk_state: PackedChunkState, store_ordering: Ordering) {
        debug_assert!((index as usize) < self.capacity());

        self.0.write_at(index as usize, value);

        chunk_state.set_len(index+1);

        self.0.header().chunk_state.store(chunk_state, store_ordering);
    }

    /// Append items from iterator, until have free space
    /// Returns Ok if everything fit, CapacityError() - if not
    pub fn extend<I>(&mut self, iter: &mut I, store_ordering: Ordering) -> Result<(), CapacityError<()>>
        where I:Iterator<Item = T>
    {
        let mut chunk_state = self.chunk_state(Ordering::Relaxed);
        let mut index = chunk_state.len() as usize;

        loop {
            if index == self.capacity(){
                chunk_state.set_len(self.capacity() as u32);
                self.0.header().chunk_state.store(chunk_state, store_ordering);
                return Result::Err(CapacityError{value:()});
            }

            match iter.next(){
                None => {
                    chunk_state.set_len(index as u32);
                    self.0.header().chunk_state.store(chunk_state, store_ordering);
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
    pub fn chunk_state(&self, ordering: Ordering) -> PackedChunkState {
        self.0.header().chunk_state.load(ordering)
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.0.len()
    }

    pub unsafe fn destruct(this: *mut Self){
        std::mem::drop(Self::recycle(this));
    }

    /// destruct all items. Can be stored for reuse.
    /// Should be called exactly once before reinitialization.
    #[must_use]
    pub unsafe fn recycle(this: *mut Self) -> DynamicChunkRecycled<T, S>{
        if std::mem::needs_drop::<T>() {
            // Relaxed because &mut self
            let len = (*this).chunk_state(Ordering::Relaxed).len() as usize;
            for i in 0..len {
                 ptr::drop_in_place((*this).0.slice_mut().get_unchecked_mut(i));
            }
        }
        DynamicChunkRecycled {chunk: NonNull::new_unchecked(this)}
    }
}

#[repr(transparent)]
pub struct DynamicChunkRecycled<T, S: Settings>{
    chunk: ptr::NonNull<DynamicChunk<T, S>>
}
impl<T, S: Settings> DynamicChunkRecycled<T, S>{
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        unsafe{ self.chunk.as_ref().capacity() }
    }
}
impl<T, S: Settings> Drop for DynamicChunkRecycled<T, S>{
    fn drop(&mut self) {
        unsafe {
            DynamicArray::<Header<T, S>, T>::destruct_uninit(
                self.chunk.as_ptr() as *mut DynamicArray<Header<T, S>, T>
            )
        }
    }
}

