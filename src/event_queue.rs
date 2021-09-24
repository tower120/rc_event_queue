//! Event queue. Multi consumers, multi producers.
//! Lock-free reading (almost as fast as slice read). Push under lock.
//!
//! Linked list of fix-sized Chunks.
//! Each Chunk have "read counter". Whenever "read counter" == "readers",
//! it is safe to delete that chunk.
//! "read counter" increases whenever reader reaches end of the chunk.
//!
//! [Event] live, until [EventReader]s live.
//!
//! In order to completely "free"/"drop" event - drop all associated [EventReader]s.
//!

/*use std::sync::atomic::{AtomicPtr, Ordering, AtomicUsize, AtomicU64};
use std::sync::{MutexGuard, Arc};
use spin::mutex::SpinMutex;
*/

#[cfg(test)]
mod test;

use crate::sync::{AtomicPtr, Ordering, AtomicUsize, AtomicU64};
use crate::sync::{Mutex, MutexGuard, Arc};
use crate::sync::{SpinMutex, SpinMutexGuard};

use std::ptr::{null_mut, null};
use crate::event_reader::EventReader;
use std::ops::ControlFlow;
use std::ops::ControlFlow::{Continue, Break};
use std::marker::PhantomPinned;
use std::pin::Pin;
use crate::cursor;
use crate::cursor::Cursor;
use crate::len_and_epoch::LenAndEpoch;
use crate::dynamic_chunk::{DynamicChunk, DynamicChunkRecycled};

pub trait Settings{
    const MIN_CHUNK_SIZE : u32;
    const MAX_CHUNK_SIZE : u32;
    const AUTO_CLEANUP: bool;
}

pub struct DefaultSettings{}
impl Settings for DefaultSettings{
    const MIN_CHUNK_SIZE: u32 = 4;
    const MAX_CHUNK_SIZE: u32 = u32::MAX / 4;
    const AUTO_CLEANUP: bool = true;
}

struct List<T, S: Settings>{
    first: *mut DynamicChunk<T, S>,
    last : *mut DynamicChunk<T, S>,
    chunk_id_counter: usize,

    /// 0 - means no penult
    penult_chunk_size: u32,

    #[cfg(feature = "double_buffering")]
    /// Biggest freed chunk
    free_chunk: Option<DynamicChunkRecycled<T, S>>,
}

pub struct EventQueue<T, S: Settings = DefaultSettings>{
    list  : Mutex<List<T, S>>,

    /// All atomic op relaxed. Just to speed up [try_clean] check (opportunistic check).
    /// Mutated under list lock.
    pub(crate) readers: AtomicUsize,

    /// Separate lock from list::start_position_epoch, is safe, because start_point_epoch encoded in
    /// chunk's atomic len+epoch.
    pub(crate) start_position: SpinMutex<Cursor<T, S>>,

    _pinned: PhantomPinned,
}

unsafe impl<T, S: Settings> Send for EventQueue<T, S>{}
unsafe impl<T, S: Settings> Sync for EventQueue<T, S>{}


impl<T, S: Settings> EventQueue<T, S>
{
    pub fn new() -> Pin<Arc<Self>>{
        let this = Arc::pin(Self{
            list: Mutex::new(List{
                first: null_mut(),
                last: null_mut(),
                chunk_id_counter: 0,

                penult_chunk_size : 0,

                #[cfg(feature = "double_buffering")]
                free_chunk: None,
            }),
            readers : AtomicUsize::new(0),
            start_position: SpinMutex::new(Cursor{chunk: null(), index:0}),
            _pinned: PhantomPinned,
        });

        let node = DynamicChunk::<T, S>::construct(
            0, 0, &*this, S::MIN_CHUNK_SIZE as usize);

        unsafe {
            let event = &mut *(&*this as *const _ as *mut EventQueue<T, S>);
            event.list.get_mut().first = node;
            event.list.get_mut().last  = node;
            event.start_position.get_mut().chunk = node;
        }

        this
    }

    #[inline]
    fn add_chunk_sized(&self, list: &mut List<T, S>, size: usize) -> &mut DynamicChunk<T, S>{
        let node = unsafe{&mut *list.last};
        let epoch = node.len_and_epoch(Ordering::Relaxed).epoch();

        // make new node
        list.chunk_id_counter += 1;

        #[cfg(not(feature = "double_buffering"))]
        let new_node = DynamicChunk::<T, S>::construct(list.chunk_id_counter, epoch, self, size);

        #[cfg(feature = "double_buffering")]
        let new_node = {
            let mut new_node: *mut DynamicChunk<T, S> = null_mut();

            if let Some(recycled_chunk) = &list.free_chunk {
                // Check if recycled_chunk have sufficient capacity. We never go down in capacity.
                if recycled_chunk.capacity() >= size {
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
                }
            }

            if new_node.is_null(){
                new_node = DynamicChunk::<T, S>::construct(list.chunk_id_counter, epoch, self, size);
            }
            new_node
        };

        // connect
        node.next().store(new_node, Ordering::Release);
        list.last = new_node;
        list.penult_chunk_size = node.capacity() as u32;

        unsafe{&mut *new_node}
    }

    #[inline]
    fn add_chunk(&self, list: &mut List<T, S>) -> &mut DynamicChunk<T, S>{
        let node = unsafe{&mut *list.last};

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

    // Leave this for a while. Have feeling that this one should be faster.
    // #[inline]
    // pub fn push(&self, value: T){
    //     let mut list = self.list.lock();
    //     let mut node = unsafe{&mut *list.last};
    //
    //     // Relaxed because we update only under lock
    //     let len_and_epoch: LenAndEpoch = node.storage.len_and_epoch(Ordering::Relaxed);
    //     let mut storage_len = len_and_epoch.len();
    //     let epoch = len_and_epoch.epoch();
    //
    //     if /*unlikely*/ storage_len as usize == CHUNK_SIZE{
    //         node = self.add_chunk(&mut *list);
    //         storage_len = 0;
    //     }
    //
    //     unsafe { node.storage.push_at(value, storage_len, epoch, Ordering::Release); }
    // }

    #[inline]
    pub fn push(&self, value: T){
        let mut list = self.list.lock();
        let node = unsafe{&mut *list.last};

        if let Err(err) = node.try_push(value, Ordering::Release){
            unsafe {
                self.add_chunk(&mut *list)
                    .push_unchecked(err.value, Ordering::Release);
            }
        }
    }

    // Not an Extend trait, because Extend::extend(&mut self)
    #[inline]
    pub fn extend<I>(&self, iter: I)
        where I: IntoIterator<Item = T>
    {
        let mut list = self.list.lock();
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
    pub fn subscribe(&self) -> EventReader<T, S>{
        let list = self.list.lock();

        let prev_readers = self.readers.fetch_add(1, Ordering::Relaxed);
        if prev_readers == 0{
            // Keep alive. Decrements in unsubscribe
            unsafe { Arc::increment_strong_count(self); }
        }

        let last_chunk = unsafe{&*list.last};
        let last_chunk_len_and_epoch = last_chunk.len_and_epoch(Ordering::Relaxed);
        let last_chunk_len = last_chunk_len_and_epoch.len();
        let epoch = last_chunk_len_and_epoch.epoch();

        let mut event_reader = EventReader{
            position: Cursor{chunk: list.first, index: 0},
            start_position_epoch: epoch
        };

        // Move to an end. This will increment read_completely_times in all passed chunks correctly.
        event_reader.set_forward_position(
            Cursor{
                chunk: last_chunk,
                index: last_chunk_len as usize
            },
            false);
        event_reader
    }

    // Called from EventReader Drop
    pub(super) fn unsubscribe(&self, event_reader: &EventReader<T, S>){
        let mut list = self.list.lock();

        // -1 read_completely_times for each chunk that reader passed
        unsafe {
            foreach_chunk(
                list.first,
                event_reader.position.chunk,
                |chunk| {
                    debug_assert!(
                        !chunk.next().load(Ordering::Acquire).is_null()
                    );
                    chunk.read_completely_times().fetch_sub(1, Ordering::AcqRel);
                    Continue(())
                }
            );
        }

        let prev_readers = self.readers.fetch_sub(1, Ordering::Relaxed);

        if S::AUTO_CLEANUP{
            self.cleanup_impl(&mut list);
        }

        if prev_readers == 1{
            std::mem::drop(list);
            // Safe to self-destruct
            unsafe { Arc::decrement_strong_count(self); }
        }
    }

    unsafe fn free_chunk(chunk: *mut DynamicChunk<T, S>, list: &mut MutexGuard<List<T, S>>){
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

    fn cleanup_impl(&self, list: &mut MutexGuard<List<T, S>>){
        let readers_count = self.readers.load(Ordering::Relaxed);
        unsafe {
            foreach_chunk(
                list.first,
                list.last,
                |chunk| {
                    if chunk.read_completely_times().load(Ordering::Acquire) != readers_count{
                        return Break(());
                    }

                    let next_chunk_ptr = chunk.next().load(Ordering::Relaxed);
                    debug_assert!(!next_chunk_ptr.is_null());

                    debug_assert!(std::ptr::eq(chunk, list.first));
                    Self::free_chunk(list.first, list);
                    list.first = next_chunk_ptr;

                    Continue(())
                }
            );
        }
        if list.first == list.last{
            list.penult_chunk_size = 0;
        }
    }

    /// Free all completely read chunks.
    /// Called automatically with AUTO_CLEANUP = true.
    pub fn cleanup(&self){
        self.cleanup_impl(&mut self.list.lock());
    }

    #[inline]
    fn set_start_position(
        &self,
        list: &mut MutexGuard<List<T, S>>,
        new_start_position: Cursor<T, S>)
    {
        *self.start_position.lock() = new_start_position;

        // update len_and_start_position_epoch in each chunk
        let new_epoch = unsafe{ (*list.first).len_and_epoch(Ordering::Relaxed).epoch() } + 1;
        unsafe {
            foreach_chunk_mut(
                list.first,
                null(),
                |chunk| {
                    chunk.set_epoch(new_epoch, Ordering::Relaxed, Ordering::Release);
                    Continue(())
                }
            );
        }

        if S::AUTO_CLEANUP {
            if self.readers.load(Ordering::Relaxed) == 0{
                self.cleanup_impl(list);
            }
        }
    }

    /// Clear queue for readers. Does not free memory by itself.
    ///
    /// This will move all readers on the next read to the "end of the queue", if they did not progress
    /// further. Where "end of the queue" - is the position at the moment of the `clear` call.
    pub fn clear(&self){
        let mut list = self.list.lock();

        let last_chunk = unsafe{ &*list.last };
        let len_and_epoch = last_chunk.len_and_epoch(Ordering::Relaxed);

        self.set_start_position(&mut list, Cursor {
            chunk: last_chunk,
            index: len_and_epoch.len() as usize
        });
    }

    /// Shortens the `EventQueue` for readers, keeping the last `len` elements and
    /// dropping the first ones. Does not free memory by itself.
    ///
    /// This will move all readers on the next read to the len-th element from the end, if they did
    /// not progress further.
    pub fn truncate_front(&self, len: usize) {
        let mut list = self.list.lock();

        // make chunks* array
        let chunks_count= unsafe {
            list.chunk_id_counter/*(*list.last).id*/ - (*list.first).id() + 1
        };

        // there is no way we can have memory enough to hold > 2^64 bytes.
        debug_assert!(chunks_count<=128);
        let mut chunks : [*const DynamicChunk<T, S>; 128] = [null(); 128];
        unsafe {
            let mut i = 0;
            foreach_chunk(
                list.first,
                null(),
                |chunk| {
                    chunks[i] = chunk;
                    i+=1;
                    Continue(())
                }
            );
        }

        let mut total_len = 0;
        for i in (0..chunks_count).rev(){
            let chunk = unsafe{ &*chunks[i] };
            let chunk_len = chunk.len_and_epoch(Ordering::Relaxed).len() as usize;
            total_len += chunk_len;
            if total_len >= len{
                self.set_start_position(&mut list, Cursor {
                    chunk: chunks[i],
                    index: total_len - len
                });
                return;
            }
        }

        // len is bigger then total_len
    }

    /// Adds chunk with capacity `new_len`. All next writes will be on new chunk.
    /// Use this in conjunction with clear/truncate_front, if you want to reduce memory pressure ASAP.
    /// Total capacity will be temporarily increased, until readers get to the new chunk.
    pub fn resize_chunk(&self, new_len: u32){
        assert!(S::MIN_CHUNK_SIZE <= new_len && new_len <= S::MAX_CHUNK_SIZE);

        let mut list = self.list.lock();
        #[cfg(feature = "double_buffering")]
        {
            list.free_chunk = None;
        }
        self.add_chunk_sized(&mut *list, new_len as usize);

        if S::AUTO_CLEANUP {
            if self.readers.load(Ordering::Relaxed) == 0{
                self.cleanup_impl(&mut list);
            }
        }
    }

    /// Returns total chunks capacity.
    pub fn total_capacity(&self) -> usize {
        let list = self.list.lock();
        let mut total = 0;
        unsafe {
            foreach_chunk(
                list.first,
                null(),
                |chunk| {
                    total += chunk.capacity();
                    Continue(())
                }
            );
        }
        total
    }

    /// last/active chunk capacity
    pub fn chunk_capacity(&self) -> usize {
        let list = self.list.lock();
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
        debug_assert!(self.readers.load(Ordering::Relaxed) == 0);
        unsafe{
            let mut node_ptr = list.first;
            while node_ptr != null_mut() {
                let node = &mut *node_ptr;
                node_ptr = node.next().load(Ordering::Relaxed);
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
    mut func : F
)
    where F: FnMut(&DynamicChunk<T, S>) -> ControlFlow<()>
{
    foreach_chunk_mut(
        start_chunk_ptr as *mut _,
        end_chunk_ptr,
        |mut_chunk| func(mut_chunk)
    );
}

/// end_chunk_ptr may be null
#[inline(always)]
pub(super) unsafe fn foreach_chunk_mut<T, F, S: Settings>
(
    start_chunk_ptr : *mut DynamicChunk<T, S>,
    end_chunk_ptr   : *const DynamicChunk<T, S>,
    mut func : F
)
    where F: FnMut(&mut DynamicChunk<T, S>) -> ControlFlow<()>
{
    debug_assert!(!start_chunk_ptr.is_null());
    debug_assert!(
        end_chunk_ptr.is_null()
            ||
        std::ptr::eq((*start_chunk_ptr).event(), (*end_chunk_ptr).event()));

    let mut chunk_ptr = start_chunk_ptr;
    while !chunk_ptr.is_null(){
        if chunk_ptr as *const _ == end_chunk_ptr {
            break;
        }

        let chunk = &mut *chunk_ptr;
        // chunk can be dropped inside `func`, so fetch `next` beforehand
        let next_chunk_ptr = chunk.next().load(Ordering::Acquire);

        let proceed = func(chunk);
        if proceed == Break(()) {
            break;
        }

        chunk_ptr = next_chunk_ptr;
    }
}