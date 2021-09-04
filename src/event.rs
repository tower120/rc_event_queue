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

use std::sync::atomic::{AtomicPtr, Ordering, AtomicUsize, AtomicU64};
use crate::chunk::ChunkStorage;
use std::sync::{Mutex, MutexGuard, Arc};
use std::ptr::{null_mut, null};
use crate::EventReader::EventReader;
use std::cell::UnsafeCell;
use std::ops::ControlFlow;
use std::ops::ControlFlow::{Continue, Break};
use crate::utils::U32Pair;
use crate::cursor;
use std::marker::PhantomPinned;
use std::pin::Pin;

pub(super) struct Chunk<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>{
    /// Just to compare chunks by age/sequence fast. Brings order.
    /// Will overflow after years... So just ignore that possibility.
    pub(super) id      : usize,
    pub(super) storage : ChunkStorage<T, CHUNK_SIZE>,       // TODO: put last
    pub(super) next    : AtomicPtr<Self>,

    // -----------------------------------------
    // payload

    /// When == readers count, it is safe to delete this chunk.
    /// Chunk read completely if reader consumed CHUNK_SIZE'ed element.
    /// Last chunk always exists
    pub(super) read_completely_times : AtomicUsize,

    // This needed to access Event from EventReader.
    // Never changes.
    // TODO: reference?
    pub(super) event : *const Event<T, CHUNK_SIZE, AUTO_CLEANUP>,


    /// Same across all chunks. Updated in [Event::clean]
    pub(super) storage_len_and_start_point_epoch : AtomicU64
}

impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>
{
    fn new(id: usize, event : *const Event<T, CHUNK_SIZE, AUTO_CLEANUP>) -> Box<Self>{
        Box::new(Self{
            id      : id,
            storage : ChunkStorage::new(),
            next    : AtomicPtr::new(null_mut()),
            read_completely_times : AtomicUsize::new(0),
            event : event,
            storage_len_and_start_point_epoch : AtomicU64::new(0)
        })
    }
}


struct List<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>{
    first: *mut Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    last : *mut Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    chunk_id_counter: usize,
    start_point_epoch: usize,
}

// TODO: try replace with Cursor
pub struct StartPoint<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>{
    pub(super) event_chunk : *const Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    /// in-chunk index
    pub(super) index : usize,
}

/// Defaults:
///
/// CHUNK_SIZE = 512
/// AUTO_CLEANUP = true
pub struct Event<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool>{
    list  : Mutex<List<T, CHUNK_SIZE, AUTO_CLEANUP>>,     // TODO: fast spin lock here / parking_lot

    /// All atomic op relaxed. Just to speed up [try_clean] check (opportunistic check).
    /// Mutated under list lock.
    pub(super) readers: AtomicUsize,

    // TODO: start_cursor
    /// Separate lock from list::start_point_epoch, is safe, because start_point_epoch encoded in
    /// chunk's atomic len+epoch.
    pub(super) start_point : parking_lot::Mutex<StartPoint<T, CHUNK_SIZE, AUTO_CLEANUP>>,
}

// !Unpin 5% faster then PhantomPinned
impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> !Unpin for Event<T, CHUNK_SIZE, AUTO_CLEANUP>{}
unsafe impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> Send for Event<T, CHUNK_SIZE, AUTO_CLEANUP>{}
unsafe impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> Sync for Event<T, CHUNK_SIZE, AUTO_CLEANUP>{}


// TODO: rename to EventQueue
impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> Event<T, CHUNK_SIZE, AUTO_CLEANUP>
{
    pub fn new() -> Pin<Arc<Self>>{
        let node = Chunk::<T, CHUNK_SIZE, AUTO_CLEANUP>::new(0, null_mut());
        let node_ptr = Box::into_raw(node);
        let this = Arc::pin(Self{
            list    : Mutex::new(List{first: node_ptr, last: node_ptr, chunk_id_counter: 0, start_point_epoch: 0}),
            readers : AtomicUsize::new(0),

            start_point       : parking_lot::Mutex::new(StartPoint{event_chunk: node_ptr, index:0}),
        });
        unsafe {(*node_ptr).event = &*this};
        this
    }

    pub fn push(&self, value: T){
        let mut list = self.list.lock().unwrap();
        let mut node = unsafe{&mut *list.last};

        // Relaxed because we update only under lock
        if /*unlikely*/ node.storage.len(Ordering::Relaxed) == CHUNK_SIZE{
            // make new node
            list.chunk_id_counter += 1;
            let new_node = Chunk::<T, CHUNK_SIZE, AUTO_CLEANUP>::new(list.chunk_id_counter, self);
            let new_node_ptr = Box::into_raw(new_node);
            node.next = AtomicPtr::new(new_node_ptr);

            list.last = new_node_ptr;
            node = unsafe{&mut *new_node_ptr};
        }

        // TODO: rework
        unsafe{node.storage.push_unchecked(value)};

        node.storage_len_and_start_point_epoch.store(U32Pair::from_u32(
            node.storage.len(Ordering::Relaxed) as u32,
            list.start_point_epoch as u32
        ).as_u64(), Ordering::Release);
    }

    /// EventReader will start receive events from NOW.
    /// It will not see events that was pushed BEFORE subscription.
    pub fn subscribe(&self) -> EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>{
        let list = self.list.lock().unwrap();

        let prev_readers = self.readers.fetch_add(1, Ordering::Relaxed);
        if prev_readers == 0{
            // Keep alive. Decrements in unsubscribe
            unsafe { Arc::increment_strong_count(self); }
        }

        let mut event_reader = EventReader{
            event_chunk : list.first,
            index : 0,
            start_point_epoch : list.start_point_epoch
        };

        // TODO: ?? storage.get_last_index, then release list lock. try_cleanup = auto_cleanup

        // Move to an end. This will increment read_completely_times in all passed chunks correctly.
        event_reader.set_forward_position(
            list.last,
            unsafe{ (*list.last).storage.get_last_index(Ordering::Relaxed) },
            false);
        event_reader
    }

    // Called from EventReader Drop
    pub(super) fn unsubscribe(&self, event_reader: &EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>){
        let list = self.list.lock().unwrap();

        // -1 read_completely_times for each chunk that reader passed
        unsafe {
            foreach_chunk(
                list.first,
                event_reader.event_chunk,
                |chunk| {
                    debug_assert!(chunk.storage.len(Ordering::Acquire) == chunk.storage.capacity());
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

    // TODO: rename to shrink_to_fit
    /// Free all completely read chunks.
    /// Called automatically with AUTO_CLEANUP = true.
    pub fn free_read_chunks(&self){
        let mut list = self.list.lock().unwrap();

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

    pub fn clear(&self){
        let mut list = self.list.lock().unwrap();

        list.start_point_epoch += 1;

        {
            let chunk = unsafe{ &*list.last };
            let index = chunk.storage.len(Ordering::Relaxed);

            let mut start_point = self.start_point.lock();

            start_point.event_chunk = chunk;
            start_point.index = index;
        }

        // update storage_len_and_start_point_epoch in each chunk
        unsafe {
            foreach_chunk(
                list.first,
                null(),
                |chunk| {
                    let len_and_epoch = U32Pair::from_u64(chunk.storage_len_and_start_point_epoch.load(Ordering::Relaxed));
                    let len = len_and_epoch.first();

                    let new_len_and_epoch = U32Pair::from_u32(len, list.start_point_epoch as u32);
                    chunk.storage_len_and_start_point_epoch.store(new_len_and_epoch.as_u64(), Ordering::Release);

                    Continue(())
                }
            );
        }
    }


    // TODO: len in chunks
    // TODO: truncate
    // TODO: push_array / push_session

}

impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Drop for Event<T, CHUNK_SIZE, AUTO_CLEANUP>{
    fn drop(&mut self) {
        let list = self.list.lock().unwrap();
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

/// end_chunk_ptr may be null
pub(super) unsafe fn foreach_chunk<T, F, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>
(
    start_chunk_ptr : *const Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    end_chunk_ptr   : *const Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    mut func : F
)
    where F: FnMut(&Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>) -> ControlFlow<()>
{
    debug_assert!(!start_chunk_ptr.is_null());
    debug_assert!(
        end_chunk_ptr.is_null()
            ||
        (*start_chunk_ptr).event == (*end_chunk_ptr).event);

    let mut chunk_ptr = start_chunk_ptr;
    while !chunk_ptr.is_null(){
        if chunk_ptr == end_chunk_ptr {
            break;
        }

        let chunk = &*chunk_ptr;
        // chunk can be dropped inside `func`, so fetch `next` beforehand
        let next_chunk_ptr = chunk.next.load(Ordering::Acquire);

        let proceed = func(chunk);
        if proceed == Break(()) {
            break;
        }

        chunk_ptr = next_chunk_ptr;
    }
}