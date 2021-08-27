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

use std::sync::atomic::{AtomicPtr, Ordering, AtomicIsize, AtomicUsize};
use crate::chunk::ChunkStorage;
use std::sync::{Mutex, MutexGuard, Arc};
use std::ptr::null_mut;
use crate::EventReader::EventReader;
use std::cell::UnsafeCell;
use std::ops::ControlFlow;
use std::ops::ControlFlow::{Continue, Break};

// TODO: hide CHUNK_SIZE
pub(super) struct Chunk<T, const CHUNK_SIZE : usize>{
    pub(super) storage : ChunkStorage<T, CHUNK_SIZE>,
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

    /// All atomic op relaxed. Just to speed up [try_clean] check (opportunistic check).
    /// Mutated under list lock.
    pub(super) readers: AtomicUsize,

    pub(super) auto_cleanup: bool
}

pub struct EventSettings{
    auto_cleanup: bool
}

impl Default for EventSettings{
    fn default() -> Self {
        Self{
            auto_cleanup: true
        }
    }
}

impl<T, const CHUNK_SIZE : usize> Event<T, CHUNK_SIZE>
{
    // TODO: return Pin
    pub fn new(settings: EventSettings) -> Arc<Self>{
        let node = Chunk::<T, CHUNK_SIZE >::new(null_mut());
        let node_ptr = Box::into_raw(node);
        let this = Arc::new(Self{
            list    : Mutex::new(List{first: node_ptr, last:node_ptr}),
            readers : AtomicUsize::new(0),
            auto_cleanup : settings.auto_cleanup
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
            // Keep alive. Decrements in unsubscribe
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

        // -1 read_completely_times for each chunk that reader passed
        unsafe {
            foreach_chunk(
                list.first,
                event_reader.event_chunk,
                |chunk| {
                    debug_assert!(chunk.storage.len() == chunk.storage.capacity());
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
    pub(super) fn free_read_chunks(&self){
        let mut list = self.list.lock().unwrap();

        // This should not be possible
        debug_assert!(list.first != list.last);

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

    pub fn clean(){
        todo!()
    }


    // TODO: push_array / push_session

}

impl<T, const CHUNK_SIZE : usize> Drop for Event<T, CHUNK_SIZE>{
    fn drop(&mut self) {
        let list = self.list.lock().unwrap();
        debug_assert!(self.readers.load(Ordering::Relaxed) == 0);
        unsafe{
            let mut node = list.first;
            while node != null_mut() {
                let _ = Box::from_raw(node);    // destruct with box destructor
                node = (*node).next.load(Ordering::Relaxed);
            }
        }
    }
}

pub(super) unsafe fn foreach_chunk<T, F, const CHUNK_SIZE : usize>
(
    start_chunk_ptr : *const Chunk<T, CHUNK_SIZE>,
    end_chunk_ptr   : *const Chunk<T, CHUNK_SIZE>,
    mut func : F
)
    where F: FnMut(&Chunk<T, CHUNK_SIZE>) -> ControlFlow<()>
{
    debug_assert!(!start_chunk_ptr.is_null());
    debug_assert!(!end_chunk_ptr.is_null());
    debug_assert!((*start_chunk_ptr).event == (*end_chunk_ptr).event);

    let mut chunk_ptr = start_chunk_ptr;
    while !chunk_ptr.is_null(){
        if chunk_ptr == end_chunk_ptr {
            break;
        }

        let chunk = &*chunk_ptr;
        let proceed = func(chunk);
        if proceed == Break(()) {
            break;
        }

        chunk_ptr = chunk.next.load(Ordering::Acquire);
    }
}


#[cfg(test)]
mod tests {
    use crate::event::{Event, EventSettings};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use itertools::{Itertools, assert_equal};
    use crate::EventReader::EventReader;

    //#[derive(Clone, Eq, PartialEq, Hash)]
    struct Data<F: FnMut()>{
        id : usize,
        name: String,
        on_destroy: F
    }

    impl<F: FnMut()> Data<F>{
        fn from(i:usize, on_destroy: F) -> Self {
            Self{
                id : i,
                name: i.to_string(),
                on_destroy: on_destroy
            }
        }
    }

    impl<F: FnMut()> Drop for Data<F>{
        fn drop(&mut self) {
            (self.on_destroy)();
        }
    }

    #[test]
    fn push_drop_test() {
        let destruct_counter = AtomicUsize::new(0);
        let destruct_counter_ref = &destruct_counter;
        let on_destroy = ||{destruct_counter_ref.fetch_add(1, Ordering::Relaxed);};


        let mut reader_option : Option<EventReader<_, 4>> = Option::None;
        {
            let chunk_list = Event::<_, 4>::new(Default::default());
            chunk_list.push(Data::from(0, on_destroy));
            chunk_list.push(Data::from(1, on_destroy));
            chunk_list.push(Data::from(2, on_destroy));
            chunk_list.push(Data::from(3, on_destroy));

            chunk_list.push(Data::from(4, on_destroy));


            reader_option = Option::Some(chunk_list.subscribe());
            let reader = reader_option.as_mut().unwrap();
            assert_equal(
                reader.iter().map(|data| &data.id),
                Vec::<usize>::from([0, 1, 2, 3, 4]).iter()
            );

            // Only first chunk should be freed
            assert!(destruct_counter.load(Ordering::Relaxed) == 4);
        }
        assert!(destruct_counter.load(Ordering::Relaxed) == 4);
        reader_option = None;
        assert!(destruct_counter.load(Ordering::Relaxed) == 5);
    }

    #[test]
    fn read_on_full_chunk_test() {
        let destruct_counter = AtomicUsize::new(0);
        let destruct_counter_ref = &destruct_counter;
        let on_destroy = ||{destruct_counter_ref.fetch_add(1, Ordering::Relaxed);};

        {
            let chunk_list = Event::<_, 4>::new(Default::default());
            chunk_list.push(Data::from(0, on_destroy));
            chunk_list.push(Data::from(1, on_destroy));
            chunk_list.push(Data::from(2, on_destroy));
            chunk_list.push(Data::from(3, on_destroy));

            let mut reader = chunk_list.subscribe();
            assert_equal(
                reader.iter().map(|data| &data.id),
                Vec::<usize>::from([0, 1, 2, 3]).iter()
            );
            assert!(destruct_counter.load(Ordering::Relaxed) == 0);

            assert_equal(
                reader.iter().map(|data| &data.id),
                Vec::<usize>::from([]).iter()
            );
            assert!(destruct_counter.load(Ordering::Relaxed) == 0);
        }
        assert!(destruct_counter.load(Ordering::Relaxed) == 4);
    }

}
