use crate::event_queue::{CleanupMode, DefaultSettings, EventQueue, Settings};
use crate::sync::{Arc, thread, Mutex};
use super::common::*;
use loom::sync::Condvar;

#[test]
fn loom_mt_read_test(){
    loom::model(|| {
        struct S{} impl Settings for S{
            const MIN_CHUNK_SIZE: u32 = 4;
            const MAX_CHUNK_SIZE: u32 = 4;
            const CLEANUP: CleanupMode = DefaultSettings::CLEANUP;
        }
        mt_read_test_impl::<S>(3, 7);
    });
}

#[test]
fn loom_mt_write_read_test(){
    // Use Condvar, instead of AtomicBool flag.
    // Not the same, but at least loom can handle it.
    loom::model(|| {
        let writer_chunk: usize = 3;
        let writers_thread_count: usize = 1;    //should be 2 writers, instead of 1, but loom does not support >4 threads
        let readers_thread_count: usize = 2;

        struct S{} impl Settings for S{
            const MIN_CHUNK_SIZE: u32 = 4;
            const MAX_CHUNK_SIZE: u32 = 4;
            const CLEANUP: CleanupMode = DefaultSettings::CLEANUP;
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

        let readers_stop = Arc::new(
            (Mutex::new(false), Condvar::new())
        );

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

                let (lock, cvar) = &*readers_stop;
                let mut stopped = lock.lock();

                loop {
                    for [i0, i1,  i2, i3] in reader.iter(){
                        local_sum0 += i0;
                        local_sum1 += i1;
                        local_sum2 += i2;
                        local_sum3 += i3;
                    }

                    if *stopped { break; }
                    stopped = cvar.wait(stopped).unwrap();
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

        {
            let (lock, cvar) = &*readers_stop;
            let mut stopped = lock.lock();
            *stopped = true;
            cvar.notify_all();
        }

        for thread in reader_threads {
            thread.join().unwrap();
        }
    });
}