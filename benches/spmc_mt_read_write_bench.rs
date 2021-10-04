use rc_event_queue::spmc::{EventQueue, EventReader, Settings};
use criterion::{Criterion, black_box, criterion_main, criterion_group, BenchmarkId};
use std::time::{Duration, Instant};
use std::thread;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::pin::Pin;
use rc_event_queue::{CleanupMode, LendingIterator};

const QUEUE_SIZE: usize = 100000;

struct S{} impl Settings for S{
    const MIN_CHUNK_SIZE: u32 = 512;
    const MAX_CHUNK_SIZE: u32 = 512;
    const CLEANUP: CleanupMode = CleanupMode::Never;
}
type Event = EventQueue<usize, S>;


/// We test high-contention read-write case.
fn bench_event_read_write<F>(iters: u64, writer_fn: F) -> Duration
    where F: Fn(&mut Event, usize, usize) -> () + Send + 'static + Clone
{
    let mut total = Duration::ZERO;
    let readers_thread_count = 4;

    for _ in 0..iters {
        let mut event = Event::new();

        let mut readers = Vec::new();
        for _ in 0..readers_thread_count{
            readers.push(EventReader::new(&mut event));
        }

        // write
        let writer_thread = {
            let writer_fn = writer_fn.clone();
            Box::new(thread::spawn(move || {
                writer_fn(&mut event, 0, QUEUE_SIZE);
            }))
        };

        // read
        let readers_stop = Arc::new(AtomicBool::new(false));
        let mut reader_threads = Vec::new();
        for mut reader in readers{
            let readers_stop = readers_stop.clone();
            let thread = Box::new(thread::spawn(move || {
                let mut local_sum0: usize = 0;

                // do-while ensures that reader will try another round after stop,
                // to consume leftovers. Since iter's end/sentinel acquired at iter construction.
                loop{
                    let stop = readers_stop.load(Ordering::Acquire);
                    let mut iter = reader.iter();
                    while let Some(i) = iter.next(){
                        local_sum0 += i;
                    }
                    if stop{ break; }
                }

                black_box(local_sum0);
            }));
            reader_threads.push(thread);
        }

        // await and measure
        let start = Instant::now();

            writer_thread.join().unwrap();
            readers_stop.store(true, Ordering::Release);
            for thread in reader_threads {
                thread.join().unwrap();
            }

        total += start.elapsed();
    }
    total
}


pub fn mt_read_write_event_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("spmc::EventQueue extend");
    for session_size in [4, 8, 16, 32, 128, 512 as usize]{
        group.bench_with_input(
            BenchmarkId::new("spmc::EventQueue extend", session_size),
            &session_size,
            |b, input| b.iter_custom(|iters| {
                let session_len = *input;
                let f = move |event: &mut Event, from: usize, to: usize|{
                    write_extend(session_len, event, from, to);
                };
                bench_event_read_write(iters, f)
            }));
    }

    #[inline(always)]
    fn write_push(event: &mut Event, from: usize, to: usize){
        for i  in from..to{
            event.push(black_box(i));
        }
    }
    #[inline(always)]
    fn write_extend(session_len: usize, event: &mut Event, from: usize, to: usize){
        let mut i = from;
        loop{
            let session_from = i;
            let session_to = session_from + session_len;
            if session_to>=to{
                return;
            }

            event.extend(black_box(session_from..session_to));

            i = session_to;
        }
    }

    group.bench_function("spmc::EventQueue push", |b|b.iter_custom(|iters| bench_event_read_write(iters, write_push)));
}

criterion_group!(benches, mt_read_write_event_benchmark);
criterion_main!(benches);