use crate::event_queue::{EventQueue, Settings};
use crate::event_reader::EventReader;
use crate::sync::{AtomicUsize, Ordering, AtomicBool, Arc, thread};
use itertools::{Itertools, assert_equal};
use std::borrow::BorrowMut;
use std::ops::Range;
use super::common::*;

//#[derive(Clone, Eq, PartialEq, Hash)]
struct Data<F: FnMut()>{
    id : usize,
    _name: String,
    on_destroy: F
}

impl<F: FnMut()> Data<F>{
    fn from(i:usize, on_destroy: F) -> Self {
        Self{
            id : i,
            _name: i.to_string(),
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

    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 4;
        const MAX_CHUNK_SIZE: u32 = 4;
        const AUTO_CLEANUP: bool = true;
    }

    let mut reader_option : Option<EventReader<_, S>> = Option::None;
    {
        let chunk_list = EventQueue::<_, S>::new();
        reader_option = Option::Some(chunk_list.subscribe());

        chunk_list.push(Data::from(0, on_destroy));
        chunk_list.push(Data::from(1, on_destroy));
        chunk_list.push(Data::from(2, on_destroy));
        chunk_list.push(Data::from(3, on_destroy));

        chunk_list.push(Data::from(4, on_destroy));

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
        struct S{} impl Settings for S{
            const MIN_CHUNK_SIZE: u32 = 4;
            const MAX_CHUNK_SIZE: u32 = 4;
            const AUTO_CLEANUP: bool = true;
        }

        let chunk_list = EventQueue::<_, S>::new();
        let mut reader = chunk_list.subscribe();

        chunk_list.push(Data::from(0, on_destroy));
        chunk_list.push(Data::from(1, on_destroy));
        chunk_list.push(Data::from(2, on_destroy));
        chunk_list.push(Data::from(3, on_destroy));

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

#[test]
fn huge_push_test() {
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 4;
        const MAX_CHUNK_SIZE: u32 = 4;
        const AUTO_CLEANUP: bool = true;
    }

    let event = EventQueue::<usize, S>::new();
    let mut reader = event.subscribe();

    for i in 0..100000{
        event.push(i);
    }

    for _ in reader.iter(){}
}

#[test]
fn extend_test() {
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 8;
        const MAX_CHUNK_SIZE: u32 = 8;
        const AUTO_CLEANUP: bool = true;
    }

    let event = EventQueue::<usize, S>::new();
    let mut reader = event.subscribe();

    let rng : Range<usize> = 0..100000;

    event.extend(rng.clone());

    assert_eq!(reader.iter().sum::<usize>(), rng.sum());
}


#[test]
fn clean_test() {
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 4;
        const MAX_CHUNK_SIZE: u32 = 4;
        const AUTO_CLEANUP: bool = true;
    }

    let event = EventQueue::<usize, S>::new();
    let mut reader = event.subscribe();

    event.push(0);
    event.push(1);
    event.push(2);
    event.push(3);

    event.clear();
    assert!(reader.iter().next().is_none());

    event.push(4);
    event.push(5);
    assert_equal(
        reader.iter(),
        Vec::<usize>::from([4, 5]).iter()
    );
}

#[test]
fn truncate_front_test1() {
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 4;
        const MAX_CHUNK_SIZE: u32 = 4;
        const AUTO_CLEANUP: bool = true;
    }

    let event = EventQueue::<usize, S>::new();

    event.extend(0..3);

    let truncated_chunks = event.truncate_front(1);
    assert_eq!(truncated_chunks, 0);
    assert_eq!(event.chunks_count(), 1);

    event.extend(3..6);
    assert_eq!(event.chunks_count(), 2);

    let truncated_chunks = event.truncate_front(1);
    assert_eq!(truncated_chunks, 1);
    assert_eq!(event.chunks_count(), 1);
}

#[test]
fn truncate_front_test2() {
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 4;
        const MAX_CHUNK_SIZE: u32 = 4;
        const AUTO_CLEANUP: bool = true;
    }

    let event = EventQueue::<usize, S>::new();
    let mut reader = event.subscribe();

    event.extend(0..40);
    assert_eq!(event.chunks_count(), 10);

    let truncated_chunks = event.truncate_front(2);
    assert_eq!(truncated_chunks, 8);

    assert_eq!(event.chunks_count(), 10);
    reader.update_position();
    assert_eq!(event.chunks_count(), 2);

    assert_equal(reader.iter().copied(), 32..40);
    assert_eq!(event.chunks_count(), 1);
}

#[test]
fn chunks_count_test() {
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 4;
        const MAX_CHUNK_SIZE: u32 = 4;
        const AUTO_CLEANUP: bool = true;
    }

    let event = EventQueue::<usize, S>::new();

    assert_eq!(event.chunks_count(), 1);
    event.push(0);
    event.push(1);
    event.push(2);
    event.push(3);
    assert_eq!(event.chunks_count(), 1);

    event.push(4);
    assert_eq!(event.chunks_count(), 2);
    event.push(5);
    assert_eq!(event.chunks_count(), 2);

    event.clear();
    assert_eq!(event.chunks_count(), 1);
}

#[test]
fn mt_read_test() {
    for _ in 0..10{
        struct S{} impl Settings for S{
            const MIN_CHUNK_SIZE: u32 = 512;
            const MAX_CHUNK_SIZE: u32 = 512;
            const AUTO_CLEANUP: bool = true;
        }
        mt_read_test_impl::<S>(4, 1000000);
    }
}

#[test]
fn mt_write_read_test() {
for _ in 0..100{
    let writer_chunk = 10000;
    let writers_thread_count = 2;
    let readers_thread_count = 4;
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 32;
        const MAX_CHUNK_SIZE: u32 = 32;
        const AUTO_CLEANUP: bool = true;
    }

    let event = EventQueue::<[usize;4], S>::new();

    let mut readers = Vec::new();
    for _ in 0..readers_thread_count{
        readers.push(event.subscribe());
    }

    // etalon
    let sum0: usize = (0..writers_thread_count*writer_chunk).map(|i|i+0).sum();
    let sum1: usize = (0..writers_thread_count*writer_chunk).map(|i|i+1).sum();
    let sum2: usize = (0..writers_thread_count*writer_chunk).map(|i|i+2).sum();
    let sum3: usize = (0..writers_thread_count*writer_chunk).map(|i|i+3).sum();

    // write
    let mut writer_threads = Vec::new();
    for thread_id in 0..writers_thread_count{
        let event = event.clone();
        let thread = Box::new(thread::spawn(move || {
            let from = thread_id*writer_chunk;
            let to = from+writer_chunk;

            for i  in from..to{
                event.push([i, i+1, i+2, i+3]);
            }
        }));
        writer_threads.push(thread);
    }

    // read
    let readers_stop = Arc::new(AtomicBool::new(false));
    let mut reader_threads = Vec::new();
    for mut reader in readers{
        let readers_stop = readers_stop.clone();
        let thread = Box::new(thread::spawn(move || {
            let mut local_sum0: usize = 0;
            let mut local_sum1: usize = 0;
            let mut local_sum2: usize = 0;
            let mut local_sum3: usize = 0;

            // do-while ensures that reader will try another round after stop,
            // to consume leftovers. Since iter's end/sentinel acquired at iter construction.
            loop{
                let stop = readers_stop.load(Ordering::Acquire);

                for [i0, i1,  i2, i3] in reader.iter(){
                    local_sum0 += i0;
                    local_sum1 += i1;
                    local_sum2 += i2;
                    local_sum3 += i3;
                }

                if stop{ break; }
                std::hint::spin_loop();
            }

            assert_eq!(local_sum0, sum0);
            assert_eq!(local_sum1, sum1);
            assert_eq!(local_sum2, sum2);
            assert_eq!(local_sum3, sum3);
        }));
        reader_threads.push(thread);
    }

    for thread in writer_threads {
        thread.join().unwrap();
    }
    readers_stop.store(true, Ordering::Release);
    for thread in reader_threads {
        thread.join().unwrap();
    }
}
}