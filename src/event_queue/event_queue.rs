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

use crate::sync::{AtomicPtr, Ordering, AtomicUsize, AtomicU64};
use crate::sync::{Mutex, MutexGuard, Arc};
use crate::sync::{SpinMutex, SpinMutexGuard};

use crate::event_queue::chunk_storage::{ChunkStorage, CapacityError};
use std::ptr::{null_mut, null};
use crate::event_reader::EventReader;
use std::cell::UnsafeCell;
use std::ops::ControlFlow;
use std::ops::ControlFlow::{Continue, Break};
use crate::utils::U32Pair;
use crate::cursor;
use std::marker::PhantomPinned;
use std::pin::Pin;
use crate::cursor::Cursor;
use crate::len_and_epoch::LenAndEpoch;
use crate::event_queue::chunk::Chunk;
use crate::event_queue::list::List;

/// Defaults:
///
/// CHUNK_SIZE = 512
/// AUTO_CLEANUP = true
pub struct EventQueue<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool>{
    list  : Mutex<List<T, CHUNK_SIZE, AUTO_CLEANUP>>,

    /// All atomic op relaxed. Just to speed up [try_clean] check (opportunistic check).
    /// Mutated under list lock.
    pub(crate) readers: AtomicUsize,

    /// Separate lock from list::start_position_epoch, is safe, because start_point_epoch encoded in
    /// chunk's atomic len+epoch.
    pub(crate) start_position: SpinMutex<Cursor<T, CHUNK_SIZE, AUTO_CLEANUP>>,

    _pinned: PhantomPinned,
}

unsafe impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> Send for EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>{}
unsafe impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> Sync for EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>{}


impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>
{
    pub fn new() -> Pin<Arc<Self>>{
        let node = Chunk::<T, CHUNK_SIZE, AUTO_CLEANUP>::new(0, 0, null_mut());
        let node_ptr = Box::into_raw(node);
        let this = Arc::pin(Self{
            list    : Mutex::new(List{first: node_ptr, last: node_ptr, chunk_id_counter: 0}),
            readers : AtomicUsize::new(0),
            start_position: SpinMutex::new(Cursor{chunk: node_ptr, index:0}),
            _pinned: PhantomPinned,
        });
        unsafe {(*node_ptr).event = &*this};
        this
    }

    #[inline]
    fn add_chunk(&self, list: &mut List<T, CHUNK_SIZE, AUTO_CLEANUP>) -> &mut Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>{
        let node = unsafe{&mut *list.last};
        let epoch = node.storage.len_and_epoch(Ordering::Relaxed).epoch();

        // make new node
        list.chunk_id_counter += 1;
        let new_node = Chunk::<T, CHUNK_SIZE, AUTO_CLEANUP>::new(list.chunk_id_counter, epoch, self);
        let new_node_ptr = Box::into_raw(new_node);

        // connect
        node.next.store(new_node_ptr, Ordering::Release);
        list.last = new_node_ptr;

        unsafe{&mut *new_node_ptr}
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

        if let Err(err) = node.storage.try_push(value, Ordering::Release){
            unsafe {
                self.add_chunk(&mut *list)
                    .storage.push_unchecked(err.value, Ordering::Release);
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

        while node.storage.extend(&mut iter, Ordering::Release).is_err(){
            match iter.next() {
                None => {return;}
                Some(value) => {
                    // add chunk and push value there
                    node = self.add_chunk(&mut *list);
                    unsafe{ node.storage.push_unchecked(value, Ordering::Relaxed); }
                }
            };
        }
    }

    /// EventReader will start receive events from NOW.
    /// It will not see events that was pushed BEFORE subscription.
    pub fn subscribe(&self) -> EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>{
        let list = self.list.lock();

        let prev_readers = self.readers.fetch_add(1, Ordering::Relaxed);
        if prev_readers == 0{
            // Keep alive. Decrements in unsubscribe
            unsafe { Arc::increment_strong_count(self); }
        }

        let last_chunk = unsafe{&*list.last};
        let last_chunk_len_and_epoch = last_chunk.storage.len_and_epoch(Ordering::Relaxed);
        let last_chunk_len = last_chunk_len_and_epoch.len();
        let epoch = last_chunk_len_and_epoch.epoch();

        let mut event_reader = EventReader{
            position: Cursor{chunk: list.first, index: 0},
            start_position_epoch: epoch
        };

        // Move to an end. This will increment read_completely_times in all passed chunks correctly.
        event_reader.set_forward_position::<false>(
            Cursor{
                chunk: last_chunk,
                index: last_chunk_len as usize
            });
        event_reader
    }

    // Called from EventReader Drop
    pub(crate) fn unsubscribe(&self, event_reader: &EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>){
        let list = self.list.lock();

        // -1 read_completely_times for each chunk that reader passed
        unsafe {
            foreach_chunk(
                list.first,
                event_reader.position.chunk,
                |chunk| {
                    debug_assert!(
                        chunk.storage.len_and_epoch(Ordering::Acquire).len() as usize
                            ==
                        chunk.storage.capacity()
                    );
                    chunk.read_completely_times.fetch_sub(1, Ordering::AcqRel);
                    Continue(())
                }
            );
        }

        let prev_readers = self.readers.fetch_sub(1, Ordering::Relaxed);
        if prev_readers == 1{
            std::mem::drop(list);
            // Safe to self-destruct
            unsafe { Arc::decrement_strong_count(self); }
        }
    }

    fn cleanup_impl(&self, mut list: MutexGuard<List<T, CHUNK_SIZE, AUTO_CLEANUP>>){
        let readers_count = self.readers.load(Ordering::Relaxed);
        unsafe {
            foreach_chunk(
                list.first,
                list.last,
                |chunk| {
                    if chunk.read_completely_times.load(Ordering::Acquire) != readers_count{
                        return Break(());
                    }

                    let next_chunk_ptr = chunk.next.load(Ordering::Relaxed);
                    debug_assert!(!next_chunk_ptr.is_null());

                    debug_assert!(std::ptr::eq(chunk, list.first));
                    Box::from_raw(list.first);  // drop
                    list.first = next_chunk_ptr;

                    Continue(())
                }
            );
        }
    }

    /// Free all completely read chunks.
    /// Called automatically with AUTO_CLEANUP = true.
    pub fn cleanup(&self){
        self.cleanup_impl(self.list.lock());
    }

    #[inline]
    fn set_start_position(
        &self,
        list: MutexGuard<List<T, CHUNK_SIZE, AUTO_CLEANUP>>,
        new_start_position: Cursor<T, CHUNK_SIZE, AUTO_CLEANUP>)
    {
        *self.start_position.lock() = new_start_position;

        // update len_and_start_position_epoch in each chunk
        let new_epoch = unsafe{ (*list.first).storage.len_and_epoch(Ordering::Relaxed).epoch() } + 1;
        unsafe {
            foreach_chunk_mut(
                list.first,
                null(),
                |chunk| {
                    chunk.storage.set_epoch(new_epoch, Ordering::Relaxed, Ordering::Release);
                    Continue(())
                }
            );
        }

        if AUTO_CLEANUP {
            if self.readers.load(Ordering::Relaxed) == 0{
                self.cleanup_impl(list);
            }
        }
    }

    pub fn clear(&self){
        let list = self.list.lock();

        let last_chunk = unsafe{ &*list.last };
        let len_and_epoch = last_chunk.storage.len_and_epoch(Ordering::Relaxed);

        self.set_start_position(list, Cursor {
            chunk: last_chunk,
            index: len_and_epoch.len() as usize
        });
    }

    /// Shortens the `EventQueue`, keeping the last `chunks_count` chunks and dropping the first ones.
    /// At least one chunk always remains.
    /// Returns number of freed chunks
    pub fn truncate_front(&self, chunks_count: usize) -> usize {
        let list = self.list.lock();

        let chunk_id = list.chunk_id_counter as isize - chunks_count as isize + 1;
        if chunk_id <= 0{
            return 0;
        }

        let freed_chunks = chunk_id as usize - unsafe{(*list.first).id};

        let mut start_chunk = null();
        unsafe {
            foreach_chunk(
                list.first,
                null(),
                |chunk| {
                    if chunk.id == chunk_id as usize{
                        start_chunk = chunk;
                        return Break(());
                    }
                    Continue(())
                }
            );
        }

        self.set_start_position(list, Cursor {
            chunk: start_chunk,
            index: 0
        });

        return freed_chunks;
    }

    // chunks_count can be atomic. But does that needed?
    pub fn chunks_count(&self) -> usize {
        let list = self.list.lock();
        unsafe{
            list.chunk_id_counter/*(*list.last).id*/ - (*list.first).id + 1
        }
    }

    // TODO: reuse chunks (double/triple buffering)
    // TODO: try non-fixed chunks
}

impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Drop for EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>{
    fn drop(&mut self) {
        let list = self.list.lock();
        debug_assert!(self.readers.load(Ordering::Relaxed) == 0);
        unsafe{
            let mut node_ptr = list.first;
            while node_ptr != null_mut() {
                let node = Box::from_raw(node_ptr);    // destruct on exit
                node_ptr = node.next.load(Ordering::Relaxed);
            }
        }
    }
}

#[inline(always)]
pub(crate) unsafe fn foreach_chunk<T, F, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>
(
    start_chunk_ptr : *const Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    end_chunk_ptr   : *const Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    mut func : F
)
    where F: FnMut(&Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>) -> ControlFlow<()>
{
    foreach_chunk_mut(
        start_chunk_ptr as *mut _,
        end_chunk_ptr,
        |mut_chunk| func(mut_chunk)
    );
}

/// end_chunk_ptr may be null
#[inline(always)]
pub(crate) unsafe fn foreach_chunk_mut<T, F, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>
(
    start_chunk_ptr : *mut Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    end_chunk_ptr   : *const Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    mut func : F
)
    where F: FnMut(&mut Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>) -> ControlFlow<()>
{
    debug_assert!(!start_chunk_ptr.is_null());
    debug_assert!(
        end_chunk_ptr.is_null()
            ||
        (*start_chunk_ptr).event == (*end_chunk_ptr).event);

    let mut chunk_ptr = start_chunk_ptr;
    while !chunk_ptr.is_null(){
        if chunk_ptr as *const _ == end_chunk_ptr {
            break;
        }

        let chunk = &mut *chunk_ptr;
        // chunk can be dropped inside `func`, so fetch `next` beforehand
        let next_chunk_ptr = chunk.next.load(Ordering::Acquire);

        let proceed = func(chunk);
        if proceed == Break(()) {
            break;
        }

        chunk_ptr = next_chunk_ptr;
    }
}