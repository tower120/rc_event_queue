use crate::mpmc::{EventQueue, EventReader, Settings};
use crate::sync::{thread};
use crate::tests::utils::consume_copies;

pub(crate) fn mt_read_test_impl<S: 'static + Settings>(threads_count: usize, len: usize) {
    let event = EventQueue::<usize, S>::new();

    let mut readers = Vec::new();
    for _ in 0..threads_count{
        readers.push(EventReader::new(&event));
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
            let local_sum: usize = consume_copies(&mut reader.iter()).iter().sum();
            assert!(local_sum == sum);
        }));
        threads.push(thread);
    }

    for thread in threads{
        thread.join().unwrap();
    }
}