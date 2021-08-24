//! Lock-free reading (absolutely no locks). Push under lock.
//! Grow only.
//!
//! Linked list of fix-sized Chunks.
//! Each Chunk have "read counter". Whenever "read counter" == "readers",
//! it is safe to delete that chunk.
//! "read counter" increases whenever reader reached end of the chunk.
//!
//! [Event] live, until [EventReader]s live.
//! No event "clean" by design.
//!
//! In order to "clean" - [drain] in each associated [EventReader].
//! In order to completely "free"/"drop" event - drop all associated [EventReader]s.
//!

use std::sync::atomic::{AtomicPtr, Ordering, AtomicIsize, AtomicUsize};
use crate::chunk::ChunkStorage;
use std::sync::{Mutex, MutexGuard, Arc};
use std::ptr::null_mut;
use crate::EventReader::EventReader;
use std::cell::UnsafeCell;

// TODO: hide CHUNK_SIZE
pub(super) struct Chunk<T, const CHUNK_SIZE : usize>{
    storage : ChunkStorage<T, CHUNK_SIZE>,
    next    : AtomicPtr<Self>,

    // -----------------------------------------
    // payload

    /// When == readers count, it is safe to delete this chunk.
    /// Chunk read completely if reader consumed CHUNK_SIZE'ed element.
    /// Last chunk always exists
    pub(super) read_completely_times : AtomicUsize,

    // This needed to access Event from EventReader.
    // Never changes.
    pub(super) event : *const Event<T, CHUNK_SIZE>,
}

impl<T, const CHUNK_SIZE : usize> Chunk<T, CHUNK_SIZE >
{
    fn new(event : *const Event<T, CHUNK_SIZE>) -> Box<Self>{
        Box::new(Self{
            storage : ChunkStorage::new(),
            next    : AtomicPtr::new(null_mut()),
            read_completely_times : AtomicUsize::new(0),
            event : event
        })
    }
}


pub struct List<T, const CHUNK_SIZE : usize>{
    first: *mut Chunk<T, CHUNK_SIZE>,
    last : *mut Chunk<T, CHUNK_SIZE>,
}

pub struct Event<T, const CHUNK_SIZE : usize>{
    list  : Mutex<List<T, CHUNK_SIZE>>,     // TODO: fast spin lock here
    /// Boxed to keep memory address fixed.
    /// Accessed rarely.
    /// All atomic op relaxed. Just to speed up [try_clean] check.
    readers: AtomicUsize,
}

impl<T, const CHUNK_SIZE : usize> Event<T, CHUNK_SIZE>
{
    pub fn new() -> Arc<Self>{
        let node = Chunk::<T, CHUNK_SIZE >::new(null_mut());
        let node_ptr = Box::into_raw(node);
        let this = Arc::new(Self{
            list    : Mutex::new(List{first: node_ptr, last:node_ptr}),
            readers : AtomicUsize::new(0),
        });
        unsafe {(*node_ptr).event = Arc::as_ptr(&this)};
        this
    }

    pub fn push(&self, value: T){
        let mut list = self.list.lock().unwrap();
        let mut node = unsafe{&mut *list.last};

        // Relaxed because we update only under lock
        if /*unlikely*/ node.storage.storage_len.load(Ordering::Relaxed) == CHUNK_SIZE{
            // make new node
            let new_node = Chunk::<T, CHUNK_SIZE >::new(self);
            let new_node_ptr = Box::into_raw(new_node);
            node.next = AtomicPtr::new(new_node_ptr);

            list.last = new_node_ptr;
            node = unsafe{&mut *new_node_ptr};
        }

        unsafe{node.storage.push_unchecked(value)};
    }

    pub fn subscribe(&self) -> EventReader<T, CHUNK_SIZE>{
        let list = self.list.lock().unwrap();

        let prev_readers = self.readers.fetch_add(1, Ordering::Relaxed);
        if prev_readers == 0{
            /// Keep alive. Decrements in unsubscribe
            unsafe { Arc::increment_strong_count(self); }
        }

        EventReader{
            event_chunk : list.first,
            index : 0,
        }
    }

    // Called from EventReader Drop
    pub(super) fn unsubscribe(&self, event_reader: &EventReader<T, CHUNK_SIZE>){
        let list = self.list.lock().unwrap();

        let prev_readers = self.readers.fetch_sub(1, Ordering::Relaxed);
        if prev_readers == 1{
            /// Safe to destruct
            unsafe { Arc::decrement_strong_count(self); }
        }


        // -1 read_completely_times for each chunk that reader passed
        let mut chunk_ptr = list.first;
        while !chunk_ptr.is_null(){
            let chunk = unsafe{&*chunk_ptr};

            if event_reader.event_chunk == chunk_ptr{
                if event_reader.index == CHUNK_SIZE{
                    chunk.read_completely_times.fetch_sub(1, Ordering::AcqRel);
                }
                break;
            }

            // We read chunks in order. If this is not active reader's chunk - reader already read it completely.
            debug_assert!(chunk.storage.len() == chunk.storage.capacity());
            chunk.read_completely_times.fetch_sub(1, Ordering::AcqRel);

            chunk_ptr = chunk.next.load(Ordering::Relaxed);
        }
    }


    // TODO: push_array / push_session

}


// TODO: impl Drop


// unsafe fn get_mut(cell : &UnsafeCell<*mut T>) -> &mut T{
//     &mut **cell.get()
//}




#[cfg(test)]
mod tests {
    use crate::event::Event;

    #[derive(Clone, Eq, PartialEq, Hash)]
    struct Data{
        id : usize,
        name: String
    }

    impl Data{
        fn from(i:usize) -> Self {
            Self{
                id : i,
                name: i.to_string(),
            }
        }
    }

    struct Payload{}

    // TODO: test Arc with readers
    // TODO: test read-completely unsubscribe

    #[test]
    fn test() {
        let chunk_list = Event::<Data, 4>::new();
        chunk_list.push(Data::from(0));
        chunk_list.push(Data::from(1));
        chunk_list.push(Data::from(2));
        chunk_list.push(Data::from(3));

        chunk_list.push(Data::from(4));

        // TODO: test validity with iterator

        // readers test

    }
}
