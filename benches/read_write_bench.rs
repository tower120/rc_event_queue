use criterion::{Criterion, black_box, criterion_main, criterion_group, BenchmarkId};
use std::time::{Duration, Instant};
use events::event_queue::EventQueue;
use std::thread;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::pin::Pin;

const queue_size : usize = 100000;

type Event = EventQueue<usize, 512, false>;
type ArcEvent = Pin<Arc<Event>>;


/// We test high-contention read-write case.
fn bench_event_read_write<F>(iters: u64, writer_fn: F) -> Duration
    where F: Fn(&ArcEvent, usize, usize) -> () + Send + 'static + Clone
{
    let mut total = Duration::ZERO;

    let writers_thread_count = 2;
    let readers_thread_count = 4;


    for _ in 0..iters {
        let event = Event::new();

        let mut readers = Vec::new();
        for _ in 0..readers_thread_count{
            readers.push(event.subscribe());
        }

        // write
        let mut writer_threads = Vec::new();
        let writer_chunk = queue_size / writers_thread_count;
        for thread_id in 0..writers_thread_count{
            let event = event.clone();
            let writer_fn = writer_fn.clone();
            let thread = Box::new(thread::spawn(move || {
                let from = thread_id*writer_chunk;
                let to = from+writer_chunk;
                writer_fn(&event, from, to);
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

                // do-while ensures that reader will try another round after stop,
                // to consume leftovers. Since iter's end/sentinel acquired at iter construction.
                loop{
                    let stop = readers_stop.load(Ordering::Acquire);
                    for i in reader.iter(){
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

            for thread in writer_threads {
                thread.join().unwrap();
            }
            readers_stop.store(true, Ordering::Release);
            for thread in reader_threads {
                thread.join().unwrap();
            }

        total += start.elapsed();
    }
    total
}


pub fn read_write_event_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("EventQueue extend");
    for session_size in [4, 8, 16, 32, 128, 512 as usize]{
        group.bench_with_input(
            BenchmarkId::new("EventQueue extend", session_size),
            &session_size,
            |b, input| b.iter_custom(|iters| {
                let session_len = *input;
                let f = move |event: &ArcEvent, from: usize, to: usize|{
                    write_extend(session_len, event, from, to);
                };
                bench_event_read_write(iters, f)
            }));
    }

    #[inline(always)]
    fn write_push(event: &ArcEvent, from: usize, to: usize){
        for i  in from..to{
            event.push(black_box(i));
        }
    }
    #[inline(always)]
    fn write_extend(session_len: usize, event: &ArcEvent, from: usize, to: usize){
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

    group.bench_function("EventQueue push", |b|b.iter_custom(|iters| bench_event_read_write(iters, write_push)));
}

criterion_group!(benches, read_write_event_benchmark);
criterion_main!(benches);