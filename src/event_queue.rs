#[cfg(not(loom))]
#[cfg(test)]
mod test;

use crate::sync::{Ordering};
use crate::sync::{Mutex, Arc};
use crate::sync::{SpinMutex};

use std::ptr::{null_mut, null, NonNull};
use crate::event_reader::EventReader;
use std::ops::ControlFlow;
use std::ops::ControlFlow::{Continue, Break};
use std::marker::PhantomPinned;
use std::pin::Pin;
use crate::cursor::Cursor;
use crate::dynamic_chunk::{DynamicChunk};
#[cfg(feature = "double_buffering")]
use crate::dynamic_chunk::{DynamicChunkRecycled};
use crate::{StartPositionEpoch};

/// This way you can control when chunk's memory deallocation happens.
/// _In addition, some operations may cause deallocations as well._
#[derive(PartialEq)]
pub enum CleanupMode{
    /// Cleanup will be called when chunk fully read.
    ///
    /// In this mode memory will be freed ASAP - right in the end of reader consumption session.
    ///
    /// !! Not allowed for spmc !!
    OnChunkRead,
    /// Cleanup will be called when new chunk created.
    OnNewChunk,
    /// Cleanup will never be called. You should call `EventQueue::cleanup` manually.
    Never
}

pub trait Settings{
    const MIN_CHUNK_SIZE : u32;
    const MAX_CHUNK_SIZE : u32;
    const CLEANUP        : CleanupMode;

    // for spmc/mpmc
    /// Lock on new chunk cleanup event. Will dead-lock if already locked.
    const LOCK_ON_NEW_CHUNK_CLEANUP: bool;
    /// Call cleanup on unsubscribe?
    const CLEANUP_IN_UNSUBSCRIBE: bool;
}

pub struct List<T, S: Settings>{
    first: *mut DynamicChunk<T, S>,
    last : *mut DynamicChunk<T, S>,
    chunk_id_counter: usize,
    total_capacity: usize,

    readers_count: u32,

    /// 0 - means no penult
    penult_chunk_size: u32,

    #[cfg(feature = "double_buffering")]
    /// Biggest freed chunk
    free_chunk: Option<DynamicChunkRecycled<T, S>>,
}

pub struct EventQueue<T, S: Settings>{
    pub(crate) list  : Mutex<List<T, S>>,

    /// Separate lock from list::start_position_epoch, is safe, because start_point_epoch encoded in
    /// chunk's atomic len+epoch.
    // TODO: Make RWLock? Bench.
    // TODO: Optioned
    pub(crate) start_position: SpinMutex<Option<Cursor<T, S>>>,

    _pinned: PhantomPinned,
}

//unsafe impl<T, S: Settings> Send for EventQueue<T, S>{}
//unsafe impl<T, S: Settings> Sync for EventQueue<T, S>{}

impl<T, S: Settings> EventQueue<T, S>
{
    pub fn with_capacity(new_capacity: u32) -> Pin<Arc<Self>>{
        assert!(S::MIN_CHUNK_SIZE <= new_capacity && new_capacity <= S::MAX_CHUNK_SIZE);

        let this = Arc::pin(Self{
            list: Mutex::new(List{
                first: null_mut(),
                last: null_mut(),
                chunk_id_counter: 0,
                readers_count:0,
                total_capacity:new_capacity as usize,
                penult_chunk_size : 0,

                #[cfg(feature = "double_buffering")]
                free_chunk: None,
            }),
            start_position: SpinMutex::new(None),
            _pinned: PhantomPinned,
        });

        let node = DynamicChunk::<T, S>::construct(
            0, StartPositionEpoch::zero(), &*this, new_capacity as usize);

        unsafe {
            let event = &mut *(&*this as *const _ as *mut EventQueue<T, S>);
            event.list.get_mut().first = node;
            event.list.get_mut().last  = node;
        }

        this
    }

    #[inline]
    fn add_chunk_sized(&self, list: &mut List<T, S>, size: usize) -> &mut DynamicChunk<T, S>{
        let node = unsafe{&mut *list.last};
        let epoch = node.chunk_state(Ordering::Relaxed).epoch();

        // make new node
        list.chunk_id_counter += 1;

        #[cfg(not(feature = "double_buffering"))]
        let new_node = DynamicChunk::<T, S>::construct(list.chunk_id_counter, epoch, self, size);

        #[cfg(feature = "double_buffering")]
        let new_node = {
            let mut new_node: *mut DynamicChunk<T, S> = null_mut();

            if let Some(recycled_chunk) = &list.free_chunk {
                // Check if recycled_chunk have exact capacity.
                if recycled_chunk.capacity() == size {
                    // unwrap_unchecked()
                    new_node =
                    match list.free_chunk.take() {
                        Some(recycled_chunk) => {
                            unsafe { DynamicChunk::from_recycled(
                                recycled_chunk,
                                list.chunk_id_counter,
                                epoch) }
                        }, None => unsafe { std::hint::unreachable_unchecked() },
                    }
                } else {
                    // TODO: try free in cleanup somehow
                    list.free_chunk = None;
                }
            }

            if new_node.is_null(){
                new_node = DynamicChunk::<T, S>::construct(list.chunk_id_counter, epoch, self, size);
            }
            new_node
        };

        // connect
        node.set_next(new_node, Ordering::Release);
        list.last = new_node;
        list.penult_chunk_size = node.capacity() as u32;
        list.total_capacity += size;

        unsafe{&mut *new_node}
    }

    #[inline]
    fn on_new_chunk_cleanup(&self, list: &mut List<T, S>){
        if S::CLEANUP == CleanupMode::OnNewChunk{
            // this should acts as compile-time-if.
            if S::LOCK_ON_NEW_CHUNK_CLEANUP{
                let _lock = unsafe{ self.list.raw().lock() };
                self.cleanup_impl(list);
            } else {
                self.cleanup_impl(list);
            }
        }
    }

    #[inline]
    fn add_chunk(&self, list: &mut List<T, S>) -> &mut DynamicChunk<T, S>{
        let node = unsafe{&*list.last};

        self.on_new_chunk_cleanup(list);

        // Size pattern 4,4,8,8,16,16
        let new_size: usize = {
            if list.penult_chunk_size as usize == node.capacity(){
                std::cmp::min(node.capacity() * 2, S::MAX_CHUNK_SIZE as usize)
            } else {
                node.capacity()
            }
        };

        self.add_chunk_sized(list, new_size)
    }

    // Have 10% better performance. Observable in spmc.
    #[inline]
    pub fn push(&self, list: &mut List<T, S>, value: T){
        let mut node = unsafe{&mut *list.last};

        // Relaxed because we update only under lock
        let chunk_state = node.chunk_state(Ordering::Relaxed);
        let mut storage_len = chunk_state.len();

        if /*unlikely*/ storage_len == node.capacity() as u32{
            node = self.add_chunk(&mut *list);
            storage_len = 0;
        }

        unsafe { node.push_at(value, storage_len, chunk_state, Ordering::Release); }
    }

/*
    #[inline]
    pub fn push(&self, list: &mut List<T, S>, value: T){
        let node = unsafe{&mut *list.last};

        if let Err(err) = node.try_push(value, Ordering::Release){
            unsafe {
                self.add_chunk(&mut *list)
                    .push_unchecked(err.value, Ordering::Release);
            }
        }
    }
*/

    // Not an Extend trait, because Extend::extend(&mut self)
    #[inline]
    pub fn extend<I>(&self, list: &mut List<T, S>, iter: I)
        where I: IntoIterator<Item = T>
    {
        let mut node = unsafe{&mut *list.last};

        let mut iter = iter.into_iter();

        while node.extend(&mut iter, Ordering::Release).is_err(){
            match iter.next() {
                None => {return;}
                Some(value) => {
                    // add chunk and push value there
                    node = self.add_chunk(&mut *list);
                    unsafe{ node.push_unchecked(value, Ordering::Relaxed); }
                }
            };
        }
    }

    /// EventReader will start receive events from NOW.
    /// It will not see events that was pushed BEFORE subscription.
    pub fn subscribe(&self, list: &mut List<T, S>) -> EventReader<T, S>{
        if list.readers_count == 0{
            // Keep alive. Decrements in unsubscribe
            unsafe { Arc::increment_strong_count(self); }
        }
        list.readers_count += 1;

        let last_chunk = unsafe{&*list.last};
        let chunk_state = last_chunk.chunk_state(Ordering::Relaxed);

        // Enter chunk
        last_chunk.readers_entered().fetch_add(1, Ordering::AcqRel);

        EventReader{
            position: Cursor{chunk: last_chunk, index: chunk_state.len() as usize},
            start_position_epoch: chunk_state.epoch()
        }
    }

    // Called from EventReader Drop
    //
    // `this_ptr` instead of `&self`, because `&self` as reference should be valid during
    // function call. And we drop it sometimes.... through `Arc::decrement_strong_count`.
    pub(crate) fn unsubscribe(this_ptr: NonNull<Self>, event_reader: &EventReader<T, S>){
        let this = unsafe { this_ptr.as_ref() };
        let mut list = this.list.lock();

        // Exit chunk
        unsafe{&*event_reader.position.chunk}.read_completely_times().fetch_add(1, Ordering::AcqRel);

        if S::CLEANUP_IN_UNSUBSCRIBE && S::CLEANUP != CleanupMode::Never{
            if list.first as *const _ == event_reader.position.chunk {
                this.cleanup_impl(&mut *list);
            }
        }

        list.readers_count -= 1;
        if list.readers_count == 0{
            drop(list);
            // Safe to self-destruct
            unsafe { Arc::decrement_strong_count(this_ptr.as_ptr()); }
        }
    }

    unsafe fn free_chunk<const LOCK_ON_WRITE_START_POSITION: bool>(
        &self,
        chunk: *mut DynamicChunk<T, S>,
        list: &mut List<T, S>)
    {
        if let Some(start_position) = *self.start_position.as_mut_ptr(){
            if start_position.chunk == chunk{
                if LOCK_ON_WRITE_START_POSITION{
                    *self.start_position.lock() = None;
                } else {
                    *self.start_position.as_mut_ptr() = None;
                }
            }
        }

        list.total_capacity -= (*chunk).capacity();

        #[cfg(not(feature = "double_buffering"))]
        {
            DynamicChunk::destruct(chunk);
            std::mem::drop(list);   // just for use
        }

        #[cfg(feature = "double_buffering")]
        {
            if let Some(free_chunk) = &list.free_chunk {
                if free_chunk.capacity() >= (*chunk).capacity() {
                    // Discard - recycled chunk bigger then our
                    DynamicChunk::destruct(chunk);
                    return;
                }
            }
            // Replace free_chunk with our.
            list.free_chunk = Some(DynamicChunk::recycle(chunk));
        }
    }

    fn cleanup_impl(&self, list: &mut List<T, S>){
        unsafe {
            // using _ptr version, because with &chunk - reference should be valid during whole
            // lambda function call. (according to miri and some rust borrowing rules).
            // And we actually drop that chunk.
            foreach_chunk_ptr_mut(
                list.first,
                list.last,
                Ordering::Relaxed,      // we're under mutex
                |chunk_ptr| {
                    // Do not lock prev_chunk.chunk_switch_mutex because we traverse in order.
                    let chunk = &mut *chunk_ptr;
                    let chunk_readers = chunk.readers_entered().load(Ordering::Acquire);
                    let chunk_read_times = chunk.read_completely_times().load(Ordering::Acquire);
                    // Cleanup only in order
                    if chunk_readers != chunk_read_times {
                        return Break(());
                    }

                    let next_chunk_ptr = chunk.next(Ordering::Relaxed);
                    debug_assert!(!next_chunk_ptr.is_null());

                    debug_assert!(std::ptr::eq(chunk, list.first));
                    // Do not lock start_position permanently, because reader will
                    // never enter chunk before list.first
                    self.free_chunk::<true>(chunk, list);
                    list.first = next_chunk_ptr;

                    Continue(())
                }
            );
        }
        if list.first == list.last{
            list.penult_chunk_size = 0;
        }
    }

    /// This will traverse up to the start_point - and will free all unoccupied chunks. (out-of-order cleanup)
    /// This one slower then cleanup_impl.
    fn force_cleanup_impl(&self, list: &mut List<T, S>){
        self.cleanup_impl(list);

        // Lock start_position permanently, due to out of order chunk destruction.
        // Reader can try enter in the chunk in the middle of force_cleanup execution.
        let start_position = self.start_position.lock();
        let terminal_chunk = match &*start_position{
            None => { return; }
            Some(cursor) => {cursor.chunk}
        };
        if list.first as *const _ == terminal_chunk{
            return;
        }
        unsafe {
            // cleanup_impl dealt with first chunk before. Omit.
            let mut prev_chunk = list.first;
            // using _ptr version, because with &chunk - reference should be valid during whole
            // lambda function call. (according to miri and some rust borrowing rules).
            // And we actually drop that chunk.
            foreach_chunk_ptr_mut(
                (*list.first).next(Ordering::Relaxed),
                terminal_chunk,
                Ordering::Relaxed,      // we're under mutex
                |chunk| {
                    // We need to lock only `prev_chunk`, because it is impossible
                    // to get in `chunk` omitting chunk.readers_entered+1
                    let lock = (*prev_chunk).chunk_switch_mutex().write();
                        let chunk_readers = (*chunk).readers_entered().load(Ordering::Acquire);
                        let chunk_read_times = (*chunk).read_completely_times().load(Ordering::Acquire);
                        if chunk_readers != chunk_read_times {
                            prev_chunk = chunk;
                            return Continue(());
                        }

                        let next_chunk_ptr = (*chunk).next(Ordering::Relaxed);
                        debug_assert!(!next_chunk_ptr.is_null());

                        (*prev_chunk).set_next(next_chunk_ptr, Ordering::Release);
                    drop(lock);

                    self.free_chunk::<false>(chunk, list);
                    Continue(())
                }
            );
        }
    }

    pub fn cleanup(&self){
        self.cleanup_impl(&mut *self.list.lock());
    }

    #[inline]
    fn set_start_position(
        &self,
        list: &mut List<T, S>,
        new_start_position: Cursor<T, S>)
    {
        *self.start_position.lock() = Some(new_start_position);

        // update len_and_start_position_epoch in each chunk
        let first_chunk = unsafe{&mut *list.first};
        let new_epoch = first_chunk.chunk_state(Ordering::Relaxed).epoch().increment();
        unsafe {
            foreach_chunk_mut(
                first_chunk,
                null(),
                Ordering::Relaxed,      // we're under mutex
                |chunk| {
                    chunk.set_epoch(new_epoch, Ordering::Relaxed, Ordering::Release);
                    Continue(())
                }
            );
        }
    }

    pub fn clear(&self, list: &mut List<T, S>){
        let last_chunk = unsafe{ &*list.last };
        let last_chunk_len = last_chunk.chunk_state(Ordering::Relaxed).len() as usize;

        self.set_start_position(list, Cursor {
            chunk: last_chunk,
            index: last_chunk_len
        });

        self.force_cleanup_impl(list);
    }

    pub fn truncate_front(&self, list: &mut List<T, S>, len: usize) {
        // make chunks* array

        // TODO: subtract from total_capacity
        // TODO: use small_vec
        // TODO: loop if > 128?
        // there is no way we can have memory enough to hold > 2^64 bytes.
        let mut chunks : [*const DynamicChunk<T, S>; 128] = [null(); 128];
        let chunks_count=
            unsafe {
                let mut i = 0;
                foreach_chunk(
                    list.first,
                    null(),
                    Ordering::Relaxed,      // we're under mutex
                    |chunk| {
                        chunks[i] = chunk;
                        i+=1;
                        Continue(())
                    }
                );
                i
            };

        let mut total_len = 0;
        for i in (0..chunks_count).rev(){
            let chunk = unsafe{ &*chunks[i] };
            let chunk_len = chunk.chunk_state(Ordering::Relaxed).len() as usize;
            total_len += chunk_len;
            if total_len >= len{
                self.set_start_position(list, Cursor {
                    chunk: chunks[i],
                    index: total_len - len
                });
                self.force_cleanup_impl(list);
                return;
            }
        }

        // len is bigger then total_len.
        // do nothing.
    }

    pub fn change_chunk_capacity(&self, list: &mut List<T, S>, new_capacity: u32){
        assert!(S::MIN_CHUNK_SIZE <= new_capacity && new_capacity <= S::MAX_CHUNK_SIZE);
        self.on_new_chunk_cleanup(list);
        self.add_chunk_sized(&mut *list, new_capacity as usize);
    }

    pub fn total_capacity(&self, list: &List<T, S>) -> usize {
        list.total_capacity
    }

    pub fn chunk_capacity(&self, list: &List<T, S>) -> usize {
        unsafe { (*list.last).capacity() }
    }

/*
    // chunks_count can be atomic. But does that needed?
    pub fn chunks_count(&self) -> usize {
        let list = self.list.lock();
        unsafe{
            list.chunk_id_counter/*(*list.last).id*/ - (*list.first).id() + 1
        }
    }*/
}

impl<T, S: Settings> Drop for EventQueue<T, S>{
    fn drop(&mut self) {
        let list = self.list.get_mut();
        debug_assert!(list.readers_count == 0);
        unsafe{
            let mut node_ptr = list.first;
            while node_ptr != null_mut() {
                let node = &mut *node_ptr;
                node_ptr = node.next(Ordering::Relaxed);
                DynamicChunk::destruct(node);
            }
        }
    }
}

#[inline(always)]
pub(super) unsafe fn foreach_chunk<T, F, S: Settings>
(
    start_chunk_ptr : *const DynamicChunk<T, S>,
    end_chunk_ptr   : *const DynamicChunk<T, S>,
    load_ordering   : Ordering,
    mut func : F
)
    where F: FnMut(&DynamicChunk<T, S>) -> ControlFlow<()>
{
    foreach_chunk_mut(
        start_chunk_ptr as *mut _,
        end_chunk_ptr,
        load_ordering,
        |mut_chunk| func(mut_chunk)
    );
}

/// end_chunk_ptr may be null
#[inline(always)]
pub(super) unsafe fn foreach_chunk_mut<T, F, S: Settings>
(
    start_chunk_ptr : *mut DynamicChunk<T, S>,
    end_chunk_ptr   : *const DynamicChunk<T, S>,
    load_ordering   : Ordering,
    mut func : F
)
    where F: FnMut(&mut DynamicChunk<T, S>) -> ControlFlow<()>
{
    foreach_chunk_ptr_mut(
        start_chunk_ptr,
        end_chunk_ptr,
        load_ordering,
        |mut_chunk_ptr| func(&mut *mut_chunk_ptr)
    );
}

/// end_chunk_ptr may be null
#[inline(always)]
pub(super) unsafe fn foreach_chunk_ptr_mut<T, F, S: Settings>
(
    start_chunk_ptr : *mut DynamicChunk<T, S>,
    end_chunk_ptr   : *const DynamicChunk<T, S>,
    load_ordering   : Ordering,
    mut func : F
)
    where F: FnMut(*mut DynamicChunk<T, S>) -> ControlFlow<()>
{
    debug_assert!(!start_chunk_ptr.is_null());
    debug_assert!(
        end_chunk_ptr.is_null()
            ||
        std::ptr::eq((*start_chunk_ptr).event(), (*end_chunk_ptr).event())
    );
    debug_assert!(
        end_chunk_ptr.is_null()
            ||
        (*start_chunk_ptr).id() <= (*end_chunk_ptr).id()
    );

    let mut chunk_ptr = start_chunk_ptr;
    while !chunk_ptr.is_null(){
        if chunk_ptr as *const _ == end_chunk_ptr {
            break;
        }

        // chunk can be dropped inside `func`, so fetch `next` beforehand
        let next_chunk_ptr = (*chunk_ptr).next(load_ordering);

        let proceed = func(chunk_ptr);
        if proceed == Break(()) {
            break;
        }

        chunk_ptr = next_chunk_ptr;
    }
}