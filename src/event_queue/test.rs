use crate::mpmc::{EventQueue, EventReader, Settings, DefaultSettings};
use crate::{CleanupMode, LendingIterator};
use std::ptr::null;
use std::ops::ControlFlow::Continue;
use std::ops::Deref;
use itertools::assert_equal;
use rand::Rng;
use crate::event_queue::{foreach_chunk, List};
use crate::sync::Ordering;
use crate::tests::utils::{consume_copies, skip};

struct S{} impl Settings for S{
    const MIN_CHUNK_SIZE: u32 = 4;
    const MAX_CHUNK_SIZE: u32 = u32::MAX;
    const CLEANUP: CleanupMode = DefaultSettings::CLEANUP;
}

fn get_chunks_capacities<T, S: Settings>(event_queue: &EventQueue<T, S>) -> Vec<usize> {
    let list = &event_queue.0.list.lock();
    let mut chunk_capacities = Vec::new();
    unsafe{
        foreach_chunk(list.first, null(), Ordering::Acquire,
            |chunk|{
            chunk_capacities.push( chunk.capacity() );
            Continue(())
        });
    }
    chunk_capacities
}

fn get_chunks_lens<T, S: Settings>(event_queue: &EventQueue<T, S>) -> Vec<usize> {
    let list = &event_queue.0.list.lock();
    let mut chunk_lens = Vec::new();
    unsafe{
        foreach_chunk(list.first, null(), Ordering::Acquire,
        |chunk|{
            chunk_lens.push( chunk.chunk_state(Ordering::Relaxed).len() as usize );
            Continue(())
        });
    }
    chunk_lens
}

fn factual_capacity<T, S: Settings>(event_queue: &EventQueue<T, S>) -> usize {
    let list = &event_queue.0.list.lock();
    let mut total = 0;
    unsafe {
        foreach_chunk(
            list.first,
            null(),
            Ordering::Relaxed,      // we're under mutex
            |chunk| {
                total += chunk.capacity();
                Continue(())
            }
        );
    }
    total
}


#[test]
fn chunks_size_test(){
    let event = EventQueue::<usize, S>::new();
    event.extend(0..32);

    assert_equal(get_chunks_capacities(&event), [4,4,8,8,16]);
}

#[cfg(feature = "double_buffering")]
#[test]
fn double_buffering_test(){
    let event = EventQueue::<usize, S>::new();
    let mut reader = EventReader::new(&event);

    event.extend(0..24);
    assert_equal(get_chunks_capacities(&event), [4,4,8,8]);

    consume_copies(&mut reader.iter());
    assert_eq!(event.0.list.lock().free_chunk.as_ref().unwrap().capacity(), 8);
    assert_equal(get_chunks_capacities(&event), [8]);

    event.extend(0..32);
    assert!(event.0.list.lock().free_chunk.is_none());
    assert_equal(get_chunks_capacities(&event), [8, 8, 16, 16]);
}

#[test]
fn resize_test(){
    let event = EventQueue::<usize, S>::new();
    let mut reader = EventReader::new(&event);

    event.extend(0..32);
    assert_equal(get_chunks_capacities(&event), [4,4,8,8,16]);

    event.change_chunk_capacity(6);
    assert_equal(get_chunks_capacities(&event), [4,4,8,8,16,6]);
    assert_equal(get_chunks_lens(&event), [4,4,8,8,8,0]);

    event.push(32);
    assert_equal(get_chunks_capacities(&event), [4,4,8,8,16,6]);
    assert_equal(get_chunks_lens(&event), [4,4,8,8,8,1]);

    consume_copies(&mut reader.iter());
    assert_equal(get_chunks_capacities(&event), [6]);
    assert_equal(get_chunks_lens(&event), [1]);

    event.extend(0..6);
    assert_equal(get_chunks_capacities(&event), [6,6]);
    assert_equal(get_chunks_lens(&event), [6, 1]);
}

#[test]
fn truncate_front_test(){
    let event = EventQueue::<usize, S>::new();
    let mut reader = EventReader::new(&event);

    event.extend(0..26);
    assert_equal(get_chunks_capacities(&event), [4,4,8,8,16]);

    // basic
    event.truncate_front(4);
    reader.update_position();
    assert_equal(get_chunks_capacities(&event), [8,16]);
    assert_equal(consume_copies(&mut reader.iter()), [22, 23, 24, 25]);

    // more then queue
    event.extend(0..5);
    event.truncate_front(10);
    assert_equal(consume_copies(&mut reader.iter()), 0..5 as usize);

    // clear all queue
    event.extend(0..5);
    event.truncate_front(0);
    assert_equal(consume_copies(&mut reader.iter()), []);
}

#[test]
fn force_cleanup_test(){
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 4;
        const MAX_CHUNK_SIZE: u32 = 4;
    }
    // clear force cleanup effect
    {
        let event = EventQueue::<usize, S>::new();
        let mut _reader = EventReader::new(&event);

        event.extend(0..16);
        assert_equal(get_chunks_capacities(&event), [4,4,4,4]);

        event.clear();
        // first - because occupied by reader, last - because tip of the queue
        assert_equal(get_chunks_capacities(&event), [4, 4]);
    }
    // truncate force cleanup effect
    {
        let event = EventQueue::<usize, S>::new();
        let mut _reader = EventReader::new(&event);

        event.extend(0..20);
        assert_equal(get_chunks_capacities(&event), [4,4,4,4,4]);

        event.truncate_front(6);
        // first + 2 last
        assert_equal(get_chunks_capacities(&event), [4,4,4]);
    }
}

#[test]
fn capacity_test(){
    let event = EventQueue::<usize, S>::new();

    event.extend(0..26);
    assert_equal(get_chunks_capacities(&event), [4,4,8,8,16]);

    assert_eq!(event.chunk_capacity(), 16);
    assert_eq!(event.total_capacity(), get_chunks_capacities(&event).iter().sum());
}

#[test]
fn fuzzy_capacity_size_test(){
    use rand::Rng;
    let mut rng = rand::thread_rng();
    for _ in 0..100{
        let size = rng.gen_range(0..100000 as usize);
        let event = EventQueue::<usize, S>::new();
        let mut reader = EventReader::new(&event);
        event.extend(0..size);
        {
            let mut iter = reader.iter();
            for _ in 0..rng.gen_range(0..1000){
                iter.next();
            }
        }

        assert_eq!(event.total_capacity(), factual_capacity(&event));
    }
}

#[test]
#[allow(non_snake_case)]
fn CleanupMode_OnNewChunk_test(){
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 4;
        const MAX_CHUNK_SIZE: u32 = 4;
        const CLEANUP: CleanupMode = CleanupMode::OnNewChunk;
    }
    let event = EventQueue::<usize, S>::new();
    let mut reader = EventReader::new(&event);

    event.extend(0..16);
    assert_equal(get_chunks_capacities(&event), [4,4,4,4]);

    // 8 - will stop reader on the very last element of 2nd chunk. And will not leave it. So use 9
    skip(&mut reader.iter(), 9);
    assert_equal(get_chunks_capacities(&event), [4,4,4,4]);

    event.push(100);
    assert_equal(get_chunks_capacities(&event), [4,4,4]);
}

#[test]
#[allow(non_snake_case)]
fn CleanupMode_Never_test(){
    struct S{} impl Settings for S{
        const MIN_CHUNK_SIZE: u32 = 4;
        const MAX_CHUNK_SIZE: u32 = 4;
        const CLEANUP: CleanupMode = CleanupMode::Never;
    }
    let event = EventQueue::<usize, S>::new();
    let mut reader = EventReader::new(&event);

    event.extend(0..12);
    assert_equal(get_chunks_capacities(&event), [4,4,4]);

    skip(&mut reader.iter(), 5);
    assert_equal(get_chunks_capacities(&event), [4,4,4]);

    event.push(100);
    assert_equal(get_chunks_capacities(&event), [4,4,4,4]);

    consume_copies(&mut reader.iter());
    assert_equal(get_chunks_capacities(&event), [4,4,4,4]);

    event.cleanup();
    assert_equal(get_chunks_capacities(&event), [4]);
}