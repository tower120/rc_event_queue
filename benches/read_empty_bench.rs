/// This is special benchmark, to measure empty queue test overhead.

use rc_event_queue::mpmc::{EventQueue, Settings};
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::time::{Instant, Duration};
use std::collections::VecDeque;

struct EventQueueSettings{}
impl Settings for EventQueueSettings{
    const MIN_CHUNK_SIZE: u32 = 512;
    const MAX_CHUNK_SIZE: u32 = 512;
    const AUTO_CLEANUP: bool = false;
}

fn bench_event_reader(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let event = EventQueue::<usize, EventQueueSettings>::new();
        let mut reader = event.subscribe();
        let start = Instant::now();
        for i in reader.iter(){
            black_box(i);
        }
        total += start.elapsed();
    }
    total
}

fn bench_vector(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let vec = Vec::<usize>::new();

        let start = Instant::now();
        for i in vec.iter(){
            black_box(i);
        }
        total += start.elapsed();
    }
    total
}

fn bench_deque(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let deque = VecDeque::<usize>::new();

        let start = Instant::now();
        for i in deque.iter(){
            black_box(i);
        }
        total += start.elapsed();
    }
    total
}



pub fn read_empty_event_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("Read empty");
    group.bench_function("EventReader", |b|b.iter_custom(|iters| bench_event_reader(iters)));
    group.bench_function("Vec", |b|b.iter_custom(|iters| bench_vector(iters)));
    group.bench_function("Deque", |b|b.iter_custom(|iters| bench_deque(iters)));
}

criterion_group!(benches, read_empty_event_benchmark);
criterion_main!(benches);