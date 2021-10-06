//! mpmc and spmc are the same.
//!
//! Chunk size dependence test.

use criterion::{Criterion, criterion_group, criterion_main, BenchmarkId, black_box, BenchmarkGroup};
use rc_event_queue::mpmc::{EventQueue, EventReader, Settings};
use rc_event_queue::prelude::*;
use std::thread;
use std::time::{Duration, Instant};
use criterion::measurement::WallTime;

const QUEUE_SIZE: usize = 100000;

fn read_bench<S: 'static + Settings>(
    readers_start_offset_step: usize,
    read_session_size: usize,
    readers_thread_count: usize
) -> Duration {
    let event = EventQueue::<usize, S>::new();

    let mut readers = Vec::new();
    let mut queue_n = 0;
    for _ in 0..readers_thread_count {
        event.extend(queue_n.. queue_n+ readers_start_offset_step);
        readers.push(EventReader::new(&event));
        queue_n += readers_start_offset_step;
    }
    event.extend(queue_n..QUEUE_SIZE);

    // read
    let mut threads = Vec::new();
    for mut reader in readers{
        let thread = Box::new(thread::spawn(move || {
            // simulate "read sessions"
            'outer: loop{
                let mut iter = reader.iter();
                for _ in 0..read_session_size {
                    let next = iter.next();
                    match next{
                        None => {break 'outer;}
                        Some(i) => {black_box(i);}
                    }
                }
            }
        }));
        threads.push(thread);
    }

    let start = Instant::now();
    for thread in threads{
        thread.join().unwrap();
    }
    start.elapsed()
}

pub fn mt_read_event_benchmark(c: &mut Criterion) {
    fn bench(group: &mut BenchmarkGroup<WallTime>, id: &str, mut f: impl FnMut() -> Duration) {
        group.bench_function(id, |b| b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                total += f();
            }
            total
        }));
    }

    let mut test_group = |name: &str, readers_start_offset_step: usize, read_session_size: usize, threads_count: usize|{
        let mut group = c.benchmark_group(name);

        bench(&mut group, "chunk:32", ||{
            struct S{} impl Settings for S{
                const MIN_CHUNK_SIZE: u32 = 32;
                const MAX_CHUNK_SIZE: u32 = 32;
                const CLEANUP: CleanupMode = CleanupMode::Never;
            }
            read_bench::<S>(readers_start_offset_step, read_session_size, threads_count)
        });
        bench(&mut group, "chunk:128", ||{
            struct S{} impl Settings for S{
                const MIN_CHUNK_SIZE: u32 = 128;
                const MAX_CHUNK_SIZE: u32 = 128;
                const CLEANUP: CleanupMode = CleanupMode::Never;
            }
            read_bench::<S>(readers_start_offset_step, read_session_size, threads_count)
        });
        bench(&mut group, "chunk:512", ||{
            struct S{} impl Settings for S{
                const MIN_CHUNK_SIZE: u32 = 512;
                const MAX_CHUNK_SIZE: u32 = 512;
                const CLEANUP: CleanupMode = CleanupMode::Never;
            }
            read_bench::<S>(readers_start_offset_step, read_session_size, threads_count)
        });
        bench(&mut group, "chunk:2048", ||{
            struct S{} impl Settings for S{
                const MIN_CHUNK_SIZE: u32 = 2048;
                const MAX_CHUNK_SIZE: u32 = 2048;
                const CLEANUP: CleanupMode = CleanupMode::Never;
            }
            read_bench::<S>(readers_start_offset_step, read_session_size, threads_count)
        });
    };

    // thread count dependency bench
    test_group("mt read 1 thread", 0, 8096, 1);
    test_group("mt read 2 threads", 0, 8096, 2);
    test_group("mt read 4 threads", 0, 8096, 4);
    test_group("mt read 8 threads", 0, 8096, 8);

    // read session size dependency bench
/*    test_group(0, 8, 8);
    test_group(1000, 8, 8);
    test_group(0, 64, 8);
    test_group(1000, 64, 8);
    test_group(0, 8096, 8);
    test_group(1000, 8096, 8);*/
}

criterion_group!(benches, mt_read_event_benchmark);
criterion_main!(benches);