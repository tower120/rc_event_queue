/// Basically the same as mpmc version

use std::sync::atomic::Ordering;
use crate::sync::{Arc, AtomicBool, thread};
use crate::unordered_mpmc::{EventQueue, EventReader};
use crate::prelude::*;

#[test]
fn mt_write_read_test() {
for _ in 0..100{
    let writer_chunk = 10000;
    let writers_thread_count = 2;
    let readers_thread_count = 4;

    let event = Arc::new(EventQueue::<[usize;4]>::new());

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