use itertools::assert_equal;
use crate::event_reader::LendingIterator;
use crate::spmc::{EventReader, Settings};
use crate::spmc::EventQueue;
use crate::sync::{Arc, AtomicBool, Ordering, thread};
use crate::tests::utils::consume_copies;

#[test]
fn basic_test(){
    let mut event = EventQueue::<usize>::new();
    let mut reader1 = EventReader::new(&mut event);

    event.push(1);
    event.extend(2..5);

    assert_equal( consume_copies(&mut reader1.iter()),  [1,2,3,4 as usize]);
}

#[test]
fn mt_write_read_test() {
for _ in 0..100{
    let queue_size = 10000;
    let readers_thread_count = 4;
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 32;
        const MAX_CHUNK_SIZE: u32 = 32;
    }

    let mut event = EventQueue::<[usize;4], S>::new();

    let mut readers = Vec::new();
    for _ in 0..readers_thread_count{
        readers.push(EventReader::new(&mut event));
    }

    // etalon
    let sum0: usize = (0..queue_size).map(|i|i+0).sum();
    let sum1: usize = (0..queue_size).map(|i|i+1).sum();
    let sum2: usize = (0..queue_size).map(|i|i+2).sum();
    let sum3: usize = (0..queue_size).map(|i|i+3).sum();

    // write
    let writer_thread =  Box::new(thread::spawn(move || {
        for i  in 0..queue_size{
            event.push([i, i+1, i+2, i+3]);
        }
    }));

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

    writer_thread.join().unwrap();
    readers_stop.store(true, Ordering::Release);
    for thread in reader_threads {
        thread.join().unwrap();
    }
}
}