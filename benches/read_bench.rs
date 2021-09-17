use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::time::{Instant, Duration};
use std::collections::VecDeque;
use rc_event_queue::arc::event_queue::EventQueue;
use rc_event_queue::arc::event_reader::EventReader;

const QUEUE_SIZE: usize = 100000;

fn bench_event_reader(iters: u64, read_session_size: usize) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let event = EventQueue::<usize, 512, false>::new();
        let mut reader = EventReader::new(event.clone());
        for i in 0..QUEUE_SIZE {
            event.push(i);
        }

        let start = Instant::now();
        'outer: loop{
            // simulate "read sessions"
            // Testing this, because constructing iterator _and switching chunk_
            // is the only potentially "heavy" operations

            let mut iter = reader.iter();
            for n in 0..read_session_size {
                let next = iter.next();
                match next{
                    None => {break 'outer;}
                    Some(i) => {black_box(i);}
                }
            }
        }
        total += start.elapsed();
    }
    total
}

fn bench_event_reader_whole(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let event = EventQueue::<usize, 512, false>::new();
        let mut reader = EventReader::new(event.clone());
        for i in 0..QUEUE_SIZE {
            event.push(i);
        }

        let start = Instant::now();
        for i in reader.iter(){
            black_box(i);
        }
        total += start.elapsed();
    }
    total
}

fn bench_vector_whole(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let mut vec = Vec::new();
        for i in 0..QUEUE_SIZE {
            vec.push(i);
        }

        let start = Instant::now();
        for i in vec.iter(){
            black_box(i);
        }
        total += start.elapsed();
    }
    total
}

fn bench_deque_whole(iters: u64) -> Duration{
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let mut deque = VecDeque::new();
        for i in 0..QUEUE_SIZE {
            deque.push_back(i);
        }

        let start = Instant::now();
        for i in deque.iter(){
            black_box(i);
        }
        total += start.elapsed();
    }
    total
}



pub fn read_event_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("Read");
    for read_session_size in [4, 8, 16, 32, 128, 512]{
        group.bench_with_input(
            BenchmarkId::new("EventReader", read_session_size),
            &read_session_size,
       |b, input| b.iter_custom(|iters| { bench_event_reader(iters, *input) }));
    }
    group.bench_function("EventReader/Whole", |b|b.iter_custom(|iters| bench_event_reader_whole(iters)));
    group.bench_function("Vec", |b|b.iter_custom(|iters| bench_vector_whole(iters)));
    group.bench_function("Deque", |b|b.iter_custom(|iters| bench_deque_whole(iters)));
}

criterion_group!(benches, read_event_benchmark);
criterion_main!(benches);