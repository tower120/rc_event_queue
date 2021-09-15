use crate::event_reader::EventReader;
use crate::sync::{AtomicUsize, Ordering, AtomicBool, Arc, thread};
use itertools::{Itertools, assert_equal};
use crate::event_queue::event_queue::EventQueue;

pub(crate) fn mt_read_test_impl<const CHUNK_SIZE: usize>(threads_count: usize, len: usize) {
    let event = EventQueue::<usize, CHUNK_SIZE, true>::new();

    let mut readers = Vec::new();
    for _ in 0..threads_count{
        readers.push(event.subscribe());
    }

    let mut sum = 0;
    for i in 0..len{
        event.push(i);
        sum += i;
    }

    // read
    let mut threads = Vec::new();
    for mut reader in readers{
        let thread = Box::new(thread::spawn(move || {
            // some work here
            let local_sum: usize = reader.iter().sum();
            assert!(local_sum == sum);
        }));
        threads.push(thread);
    }

    for thread in threads{
        thread.join().unwrap();
    }
}