use criterion::{Criterion, BenchmarkId, black_box, criterion_main, criterion_group};
use std::time::{Duration, Instant};
use std::collections::VecDeque;
use rc_event_queue::{CleanupMode, mpmc, spmc};

const QUEUE_SIZE: usize = 100000;

macro_rules! event_queue_bench {
    ($mod_name:ident, $event_type:ty) => {
        #[allow(unused_mut)]
        mod $mod_name{
            use std::time::{Duration, Instant};
            use criterion::black_box;
            use crate::QUEUE_SIZE;

            pub fn bench_event_extend_session(iters: u64, session_len: usize) -> Duration{
                let mut total = Duration::ZERO;
                let sessions_count = QUEUE_SIZE / session_len;
                for _ in 0..iters {
                    let mut event = <$event_type>::new();
                    let start = Instant::now();

                    for session_id in 0..sessions_count{
                        let from = black_box(session_id) * session_len;
                        let to   = from + session_len;
                        event.extend(from..to);
                    }

                    total += start.elapsed();
                }
                total
            }

            pub fn bench_event_extend(iters: u64) -> Duration{
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let mut event = <$event_type>::new();
                    let start = Instant::now();

                    event.extend(black_box(0..QUEUE_SIZE));

                    total += start.elapsed();
                }
                total
            }

            pub fn bench_event_push(iters: u64) -> Duration{
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let mut event = <$event_type>::new();
                    let start = Instant::now();

                    for i in 0..QUEUE_SIZE {
                        event.push(black_box(i));
                    }

                    total += start.elapsed();
                }
                total
            }
        }
    }
}

struct MPMCEventQueueSettings{}
impl mpmc::Settings for MPMCEventQueueSettings{
    const MIN_CHUNK_SIZE: u32 = 512;
    const MAX_CHUNK_SIZE: u32 = 512;
    const CLEANUP: CleanupMode = CleanupMode::Never;
}
event_queue_bench!(mpmc_bench, crate::mpmc::EventQueue<usize, crate::MPMCEventQueueSettings>);

struct SPMCEventQueueSettings{}
impl spmc::Settings for SPMCEventQueueSettings{
    const MIN_CHUNK_SIZE: u32 = 512;
    const MAX_CHUNK_SIZE: u32 = 512;
    const CLEANUP: CleanupMode = CleanupMode::Never;
}
event_queue_bench!(spmc_bench, crate::spmc::EventQueue<usize, crate::SPMCEventQueueSettings>);

fn bench_vector_push(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let mut vec = Vec::new();
        let start = Instant::now();

        for i in 0..QUEUE_SIZE {
            vec.push(black_box(i));
        }

        total += start.elapsed();
    }
    total
}

fn bench_vector_extend(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let mut vec = Vec::new();
        let start = Instant::now();

            vec.extend(black_box(0..QUEUE_SIZE));

        total += start.elapsed();
    }
    total
}


fn bench_deque_push(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let mut vec = VecDeque::new();
        let start = Instant::now();

        for i in 0..QUEUE_SIZE {
            vec.push_back(black_box(i));
        }

        total += start.elapsed();
    }
    total
}

fn bench_deque_extend(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let mut vec = VecDeque::new();
        let start = Instant::now();

            vec.extend(black_box(0..QUEUE_SIZE));

        total += start.elapsed();
    }
    total
}


pub fn write_event_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("Write");
    // -------------------------- mpmc ---------------------------------------
    for session_size in [1, 4, 8, 16, 32, 128, 512]{
        group.bench_with_input(
            BenchmarkId::new("mpmc::EventQueue::extend session", session_size),
            &session_size,
            |b, input| b.iter_custom(|iters| { mpmc_bench::bench_event_extend_session(iters, *input) }));
    }
    group.bench_function("mpmc::EventQueue::extend", |b|b.iter_custom(|iters| mpmc_bench::bench_event_extend(iters)));
    group.bench_function("mpmc::EventQueue::push", |b|b.iter_custom(|iters| mpmc_bench::bench_event_push(iters)));

    // -------------------------- spmc ---------------------------------------
    for session_size in [1, 4, 8, 16, 32, 128, 512]{
        group.bench_with_input(
            BenchmarkId::new("spmc::EventQueue::extend session", session_size),
            &session_size,
            |b, input| b.iter_custom(|iters| { spmc_bench::bench_event_extend_session(iters, *input) }));
    }
    group.bench_function("spmc::EventQueue::extend", |b|b.iter_custom(|iters| spmc_bench::bench_event_extend(iters)));
    group.bench_function("spmc::EventQueue::push", |b|b.iter_custom(|iters| spmc_bench::bench_event_push(iters)));

    // -------------------------- std ---------------------------------------
    group.bench_function("Vec::push", |b|b.iter_custom(|iters| bench_vector_push(iters)));
    group.bench_function("Vec::extend", |b|b.iter_custom(|iters| bench_vector_extend(iters)));
    group.bench_function("Deque::push", |b|b.iter_custom(|iters| bench_deque_push(iters)));
    group.bench_function("Deque::extend", |b|b.iter_custom(|iters| bench_deque_extend(iters)));
}

criterion_group!(benches, write_event_benchmark);
criterion_main!(benches);