use criterion::{Criterion, BenchmarkId, black_box, criterion_main, criterion_group};
use std::time::{Duration, Instant};
use std::collections::VecDeque;
use rc_event_queue::event_queue::EventQueue;

const queue_size : usize = 100000;


fn bench_event_extend_session(iters: u64, session_len: usize) -> Duration{
    let mut total = Duration::ZERO;
    let sessions_count = queue_size / session_len;
    for _ in 0..iters {
        let event = EventQueue::<usize, 512, false>::new();
        let start = Instant::now();
        let mut vec = Vec::<usize>::new();

        for session_id in 0..sessions_count{
            let from = black_box(session_id) * session_len;
            let to   = from + session_len;
            event.extend(from..to);
        }

        total += start.elapsed();
    }
    total
}

fn bench_event_extend(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let event = EventQueue::<usize, 512, false>::new();
        let start = Instant::now();

        event.extend(black_box(0..queue_size));

        total += start.elapsed();
    }
    total
}

fn bench_event_push(iters: u64) -> Duration{
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
    for session_size in [1, 4, 8, 16, 32, 128, 512]{
        group.bench_with_input(
            BenchmarkId::new("EventQueue::extend session", session_size),
            &session_size,
            |b, input| b.iter_custom(|iters| { bench_event_extend_session(iters, *input) }));
    }
    group.bench_function("EventQueue::push", |b|b.iter_custom(|iters| bench_event_push(iters)));
    group.bench_function("EventQueue::extend", |b|b.iter_custom(|iters| bench_event_extend(iters)));
    group.bench_function("Vec", |b|b.iter_custom(|iters| bench_vector_whole(iters)));
    group.bench_function("Deque", |b|b.iter_custom(|iters| bench_deque_whole(iters)));
}

criterion_group!(benches, write_event_benchmark);
criterion_main!(benches);