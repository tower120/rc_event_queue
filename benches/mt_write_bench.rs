use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use criterion::{Criterion, BenchmarkId, black_box, criterion_main, criterion_group};

const THREAD_COUNT: usize = 8;
const QUEUE_SIZE: usize = 100000;
const THREAD_QUEUE_SIZE: usize = QUEUE_SIZE / THREAD_COUNT;

pub fn bench_unordered_mpmc_push(iters: u64) -> Duration{
    type EventQueue = rc_event_queue::unordered_mpmc::EventQueue<usize>;

    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let event = Arc::new(EventQueue::new());

        // write
        let mut writer_threads = Vec::new();
        for thread_id in 0..THREAD_COUNT{
            let event = event.clone();
            let thread = Box::new(thread::spawn(move || {
                let from = thread_id * THREAD_QUEUE_SIZE;
                let to   = from + THREAD_QUEUE_SIZE;
                for i in from..to {
                    event.push(black_box(i));
                }
            }));
            writer_threads.push(thread);
        }

        // measure
        let start = Instant::now();

            for thread in writer_threads {
                thread.join().unwrap();
            }

        total += start.elapsed();
    }
    total
}

pub fn bench_mpmc_push(iters: u64) -> Duration{
    type EventQueue = rc_event_queue::mpmc::EventQueue<usize>;

    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let event = Arc::new(EventQueue::new());

        // write
        let mut writer_threads = Vec::new();
        for thread_id in 0..THREAD_COUNT{
            let event = event.clone();
            let thread = Box::new(thread::spawn(move || {
                let from = thread_id * THREAD_QUEUE_SIZE;
                let to   = from + THREAD_QUEUE_SIZE;
                for i in from..to {
                    event.push(black_box(i));
                }
            }));
            writer_threads.push(thread);
        }

        // measure
        let start = Instant::now();

            for thread in writer_threads {
                thread.join().unwrap();
            }

        total += start.elapsed();
    }
    total
}

pub fn write_event_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("mt write");
    group.bench_function("unordered_mpmc::EventQueue push", |b|b.iter_custom(|iters| bench_unordered_mpmc_push(iters)));
    group.bench_function("mpmc::EventQueue push", |b|b.iter_custom(|iters| bench_mpmc_push(iters)));
}

criterion_group!(benches, write_event_benchmark);
criterion_main!(benches);