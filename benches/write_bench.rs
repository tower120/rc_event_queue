use criterion::{Criterion, BenchmarkId, black_box, criterion_main, criterion_group};
use std::time::{Duration, Instant};
use std::collections::VecDeque;
use events::event_queue::EventQueue;

const queue_size : usize = 100000;

fn bench_event_write_whole(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let event = EventQueue::<usize, 512, false>::new();
        let start = Instant::now();

        for i in 0..queue_size{
            event.push(black_box(i));
        }

        total += start.elapsed();
    }
    total
}

fn bench_vector_whole(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let mut vec = Vec::new();
        let start = Instant::now();

        for i in 0..queue_size{
            vec.push(black_box(i));
        }

        total += start.elapsed();
    }
    total
}

fn bench_deque_whole(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let mut vec = VecDeque::new();
        let start = Instant::now();

        for i in 0..queue_size{
            vec.push_back(black_box(i));
        }

        total += start.elapsed();
    }
    total
}

pub fn write_event_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("Write");
    // for read_session_size in [4, 8, 16, 32, 128, 512]{
    //     group.bench_with_input(
    //         BenchmarkId::new("EventReader", read_session_size),
    //         &read_session_size,
    //    |b, input| b.iter_custom(|iters| { bench_event_reader(iters, *input) }));
    // }
    group.bench_function("EventQueue", |b|b.iter_custom(|iters| bench_event_write_whole(iters)));
    group.bench_function("Vec", |b|b.iter_custom(|iters| bench_vector_whole(iters)));
    group.bench_function("Deque", |b|b.iter_custom(|iters| bench_deque_whole(iters)));
}

criterion_group!(benches, write_event_benchmark);
criterion_main!(benches);