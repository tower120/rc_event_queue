use crate::mpmc::{DefaultSettings, EventQueue, EventReader, Settings};
use crate::event_queue::{CleanupMode};
use crate::sync::{AtomicUsize, Ordering, AtomicBool, Arc, thread};
use itertools::{Itertools, assert_equal};
use std::borrow::BorrowMut;
use std::ops::Range;
use crate::tests::utils::{consume_copies, consume_mapped};
use crate::event_reader::LendingIterator;
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
            on_destroy
        }
    }
}

impl<F: FnMut()> Drop for Data<F>{
    fn drop(&mut self) {
        (self.on_destroy)();
    }
}

#[test]
#[allow(unused_assignments)]
fn push_drop_test() {
    let destruct_counter = AtomicUsize::new(0);
    let destruct_counter_ref = &destruct_counter;
    let on_destroy = ||{destruct_counter_ref.fetch_add(1, Ordering::Relaxed);};

    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 4;
        const MAX_CHUNK_SIZE: u32 = 4;
        const CLEANUP: CleanupMode = DefaultSettings::CLEANUP;
    }

    let mut reader_option : Option<_> = None;
    {
        let chunk_list = EventQueue::<_, S>::new();
        reader_option = Option::Some(EventReader::new(&chunk_list));

        chunk_list.push(Data::from(0, on_destroy));
        chunk_list.push(Data::from(1, on_destroy));
        chunk_list.push(Data::from(2, on_destroy));
        chunk_list.push(Data::from(3, on_destroy));

        chunk_list.push(Data::from(4, on_destroy));

        let reader = reader_option.as_mut().unwrap();
        assert_equal(
            consume_mapped(&mut reader.iter(), |data| data.id),
            [0, 1, 2, 3, 4]
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
            const CLEANUP: CleanupMode = DefaultSettings::CLEANUP;
        }

        let chunk_list = EventQueue::<_, S>::new();
        let mut reader = EventReader::new(&chunk_list);

        chunk_list.push(Data::from(0, on_destroy));
        chunk_list.push(Data::from(1, on_destroy));
        chunk_list.push(Data::from(2, on_destroy));
        chunk_list.push(Data::from(3, on_destroy));

        assert_equal(
            consume_mapped(&mut reader.iter(), |data| data.id),
            [0, 1, 2, 3]
        );
        assert!(destruct_counter.load(Ordering::Relaxed) == 0);

        assert_equal(
            consume_mapped(&mut reader.iter(), |data| data.id),
            []
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
        const CLEANUP: CleanupMode = DefaultSettings::CLEANUP;
    }

    let event = EventQueue::<usize, S>::new();
    let mut reader = EventReader::new(&event);

    let len =
        if cfg!(miri){ 1000 } else { 100000 };

    for i in 0..len{
        event.push(i);
    }

    consume_copies(&mut reader.iter());
}

#[test]
fn extend_test() {
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 8;
        const MAX_CHUNK_SIZE: u32 = 8;
        const CLEANUP: CleanupMode = DefaultSettings::CLEANUP;
    }

    let event = EventQueue::<usize, S>::new();
    let mut reader = EventReader::new(&event);

    let len =
        if cfg!(miri){ 1000 } else { 100000 };
    let rng : Range<usize> = 0..len;

    event.extend(rng.clone());

    assert_eq!(
        consume_copies(&mut reader.iter()).iter().sum::<usize>(),
        rng.sum()
    );
}

#[test]
fn clear_test() {
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 4;
        const MAX_CHUNK_SIZE: u32 = 4;
        const CLEANUP: CleanupMode = DefaultSettings::CLEANUP;
    }

    let event = EventQueue::<usize, S>::new();
    let mut reader = EventReader::new(&event);

    event.push(0);
    event.push(1);
    event.push(2);
    event.push(3);

    event.clear();
    assert!(reader.iter().next().is_none());

    event.push(4);
    event.push(5);
    assert_equal(
        consume_copies(&mut reader.iter()),
        [4, 5 as usize]
    );
}

#[test]
#[cfg(any(not(miri), target_os = "linux"))]
fn mt_read_test() {
    for _ in 0..10{
        struct S{} impl Settings for S{
            const MIN_CHUNK_SIZE: u32 = 512;
            const MAX_CHUNK_SIZE: u32 = 512;
            const CLEANUP: CleanupMode = DefaultSettings::CLEANUP;
        }
        mt_read_test_impl::<S>(4, if cfg!(miri){ 1000 } else { 1000000 });
    }
}

#[test]
#[cfg(any(not(miri), target_os = "linux"))]
fn mt_write_read_test() {
for _ in 0..100{
    let writer_chunk = if cfg!(miri){ 1000 } else { 10000 };
    let writers_thread_count = 2;
    let readers_thread_count = 4;
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 32;
        const MAX_CHUNK_SIZE: u32 = 32;
        const CLEANUP: CleanupMode = DefaultSettings::CLEANUP;
    }

    let event = EventQueue::<[usize;4], S>::new();

    let mut readers = Vec::new();
    for _ in 0..readers_thread_count{
        readers.push(EventReader::new(&event));
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

                let mut reader = reader.iter();
                while let Some([i0, i1,  i2, i3]) = reader.next() {
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