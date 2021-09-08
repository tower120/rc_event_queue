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
use crate::chunk::{ChunkStorage, CapacityError};
use std::sync::{Mutex, MutexGuard, Arc};
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
use spin::mutex::SpinMutex;
use std::borrow::BorrowMut;

pub(super) struct Chunk<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>{
    /// Just to compare chunks by age/sequence fast. Brings order.
    /// Will overflow after years... So just ignore that possibility.
    pub(super) id      : usize,
    pub(super) next    : AtomicPtr<Self>,

    /// When == readers count, it is safe to delete this chunk.
    /// Chunk read completely if reader consumed CHUNK_SIZE'ed element.
    /// Last chunk always exists
    pub(super) read_completely_times : AtomicUsize,

    // This needed to access Event from EventReader.
    // Never changes.
    pub(super) event : *const EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>,

    /// Same across all chunks. Updated in [Event::clear]
    //pub(super) len_and_start_position_epoch: AtomicU64,

    // Keep last
    pub(super) storage : ChunkStorage<T, CHUNK_SIZE>,
}

impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>
{
    fn new(id: usize, epoch: u32, event : *const EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>) -> Box<Self>{
        Box::new(Self{
            id   : id,
            next : AtomicPtr::new(null_mut()),
            read_completely_times : AtomicUsize::new(0),
            event : event,
            storage : ChunkStorage::new(epoch),
            //len_and_start_position_epoch: AtomicU64::new(0)
        })
    }
}


struct List<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>{
    first: *mut Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    last : *mut Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    chunk_id_counter: usize,
}

/// Defaults:
///
/// CHUNK_SIZE = 512
/// AUTO_CLEANUP = true
pub struct EventQueue<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool>{
    list  : parking_lot::Mutex<List<T, CHUNK_SIZE, AUTO_CLEANUP>>,

    /// All atomic op relaxed. Just to speed up [try_clean] check (opportunistic check).
    /// Mutated under list lock.
    pub(super) readers: AtomicUsize,

    /// Separate lock from list::start_position_epoch, is safe, because start_point_epoch encoded in
    /// chunk's atomic len+epoch.
    pub(super) start_position: SpinMutex<Cursor<T, CHUNK_SIZE, AUTO_CLEANUP>>,
}

// !Unpin 5% faster then PhantomPinned
impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> !Unpin for EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>{}
unsafe impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> Send for EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>{}
unsafe impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> Sync for EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>{}


impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>
{
    pub fn new() -> Pin<Arc<Self>>{
        let node = Chunk::<T, CHUNK_SIZE, AUTO_CLEANUP>::new(0, 0, null_mut());
        let node_ptr = Box::into_raw(node);
        let this = Arc::pin(Self{
            list    : parking_lot::Mutex::new(List{first: node_ptr, last: node_ptr, chunk_id_counter: 0/*, start_position_epoch: 0*/}),
            readers : AtomicUsize::new(0),
            start_position: SpinMutex::new(Cursor{chunk: node_ptr, index:0}),
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

    // Leave this for a while. Have filling that this one should be faster.
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
    //         node = self.add_chunk(list.borrow_mut());
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
            let res = self.add_chunk(list.borrow_mut())
                .storage.try_push(err.value, Ordering::Release);
            debug_assert!(res.is_ok());
        }
    }

    #[inline]
    pub fn extend<I>(&self, iter: I)
        where I: IntoIterator<Item = T>
    {
        let mut list = self.list.lock();
        let mut node = unsafe{&mut *list.last};

        let mut iter = iter.into_iter();

        while node.storage.extend(&mut iter, Ordering::Release).is_err(){
            node = self.add_chunk(list.borrow_mut());
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
        event_reader.set_forward_position(
            Cursor{
                chunk: last_chunk,
                index: last_chunk_len as usize
            },
            false);
        event_reader
    }

    // Called from EventReader Drop
    pub(super) fn unsubscribe(&self, event_reader: &EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>){
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

    /// Free all completely read chunks.
    /// Called automatically with AUTO_CLEANUP = true.
    pub fn cleanup(&self){
        let mut list = self.list.lock();

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
        let mut list = self.list.lock();

        let last_chunk = unsafe{ &*list.last };
        let len_and_epoch = last_chunk.storage.len_and_epoch(Ordering::Relaxed);
        *self.start_position.lock() = Cursor {
            chunk: last_chunk,
            index: len_and_epoch.len() as usize
        };

        // update len_and_start_position_epoch in each chunk
        let new_epoch = len_and_epoch.epoch() + 1;
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
    }


    // TODO: len in chunks
    // TODO: truncate
    // TODO: push_array / push_session
    // TODO: reuse chunks (double/triple buffering)
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
pub(super) unsafe fn foreach_chunk<T, F, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>
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
pub(super) unsafe fn foreach_chunk_mut<T, F, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>
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