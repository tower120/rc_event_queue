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

//use crate::chunk_storage::{ChunkStorage, CapacityError};
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
use crate::dynamic_chunk::DynamicChunk;

/*pub(super) struct Chunk<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>{
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
        })
    }
}
*/

pub trait Settings{
    const CHUNK_SIZE : usize;
    const AUTO_CLEANUP: bool;
}

pub struct DefaultSettings{}
impl Settings for DefaultSettings{
    const CHUNK_SIZE: usize = 512;
    const AUTO_CLEANUP: bool = true;
}

struct List<T, S: Settings>{
    first: *mut DynamicChunk<T, S>,
    last : *mut DynamicChunk<T, S>,
    chunk_id_counter: usize,
}

/// Defaults:
///
/// CHUNK_SIZE = 512
/// AUTO_CLEANUP = true
pub struct EventQueue<T, S: Settings>{
    list  : Mutex<List<T, S>>,

    /// All atomic op relaxed. Just to speed up [try_clean] check (opportunistic check).
    /// Mutated under list lock.
    pub(super) readers: AtomicUsize,

    /// Separate lock from list::start_position_epoch, is safe, because start_point_epoch encoded in
    /// chunk's atomic len+epoch.
    pub(super) start_position: SpinMutex<Cursor<T, S>>,

    _pinned: PhantomPinned,
}

unsafe impl<T, S: Settings> Send for EventQueue<T, S>{}
unsafe impl<T, S: Settings> Sync for EventQueue<T, S>{}


impl<T, S: Settings> EventQueue<T, S>
{
    pub fn new() -> Pin<Arc<Self>>{
        let this = Arc::pin(Self{
            list    : Mutex::new(List{first: null_mut(), last: null_mut(), chunk_id_counter: 0}),
            readers : AtomicUsize::new(0),
            start_position: SpinMutex::new(Cursor{chunk: null(), index:0}),
            _pinned: PhantomPinned,
        });

        let node = DynamicChunk::<T, S>::construct(
            0, 0, &*this, S::CHUNK_SIZE);

        unsafe {
            let event = &mut *(&*this as *const _ as *mut EventQueue<T, S>);
            event.list.get_mut().first = node;
            event.list.get_mut().last  = node;
            event.start_position.get_mut().chunk = node;
        }

        this
    }

    #[inline]
    fn add_chunk(&self, list: &mut List<T, S>) -> &mut DynamicChunk<T, S>{
        let node = unsafe{&mut *list.last};
        let epoch = node.len_and_epoch(Ordering::Relaxed).epoch();

        // make new node
        list.chunk_id_counter += 1;
        let new_node = DynamicChunk::<T, S>::construct(
            list.chunk_id_counter, epoch, self, S::CHUNK_SIZE);

        // connect
        node.next().store(new_node, Ordering::Release);
        list.last = new_node;

        unsafe{&mut *new_node}
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
        let list = self.list.lock();

        // -1 read_completely_times for each chunk that reader passed
        unsafe {
            foreach_chunk(
                list.first,
                event_reader.position.chunk,
                |chunk| {
                    debug_assert!(
                        chunk.len_and_epoch(Ordering::Acquire).len() as usize
                            ==
                        chunk.capacity()
                    );
                    chunk.read_completely_times().fetch_sub(1, Ordering::AcqRel);
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

    fn cleanup_impl(&self, mut list: MutexGuard<List<T, S>>){
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
                    DynamicChunk::destruct(list.first);
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
        list: MutexGuard<List<T, S>>,
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

    pub fn clear(&self){
        let list = self.list.lock();

        let last_chunk = unsafe{ &*list.last };
        let len_and_epoch = last_chunk.len_and_epoch(Ordering::Relaxed);

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

        let freed_chunks = chunk_id as usize - unsafe{(*list.first).id()};

        let mut start_chunk = null();
        unsafe {
            foreach_chunk(
                list.first,
                null(),
                |chunk| {
                    if chunk.id() == chunk_id as usize{
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
            list.chunk_id_counter/*(*list.last).id*/ - (*list.first).id() + 1
        }
    }

    // TODO: reuse chunks (double/triple buffering)
    // TODO: try non-fixed chunks
}

impl<T, S: Settings> Drop for EventQueue<T, S>{
    fn drop(&mut self) {
        let list = self.list.lock();
        debug_assert!(self.readers.load(Ordering::Relaxed) == 0);
        unsafe{
            let mut node_ptr = list.first;
            while node_ptr != null_mut() {
                let node = unsafe{&mut *node_ptr};
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