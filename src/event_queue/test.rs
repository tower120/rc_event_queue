use crate::event_queue::{EventQueue, foreach_chunk, Settings, List};
use std::ptr::null;
use std::ops::ControlFlow::Continue;
use itertools::assert_equal;
use std::ops::Deref;
use crate::sync::Ordering;

struct S{} impl Settings for S{
    const MIN_CHUNK_SIZE: u32 = 4;
    const MAX_CHUNK_SIZE: u32 = u32::MAX;
    const AUTO_CLEANUP: bool = true;
}

fn get_chunk_capacities<T, S: Settings>(list: &List<T, S>) -> Vec<usize> {
    let mut chunk_capacities = Vec::new();
    unsafe{
        foreach_chunk(list.first, null(), |chunk|{
            chunk_capacities.push( chunk.capacity() );
            Continue(())
        });
    }
    chunk_capacities
}

fn get_chunk_lens<T, S: Settings>(list: &List<T, S>) -> Vec<usize> {
    let mut chunk_lens = Vec::new();
    unsafe{
        foreach_chunk(list.first, null(), |chunk|{
            chunk_lens.push( chunk.len_and_epoch(Ordering::Relaxed).len() as usize );
            Continue(())
        });
    }
    chunk_lens
}


#[test]
fn chunks_size_test(){
    let event = EventQueue::<usize, S>::new();
    event.extend(0..32);

    let list = event.list.lock();
    assert_equal(get_chunk_capacities(&*list), [4,4,8,8,16]);
}

#[cfg(feature = "double_buffering")]
#[test]
fn double_buffering_test(){
    let event = EventQueue::<usize, S>::new();
    let mut reader = event.subscribe();
    let list = unsafe{ &*(event.list.lock().deref() as *const List<usize, S>) };

    event.extend(0..24);
    assert_equal(get_chunk_capacities(&*list), [4,4,8,8]);

    reader.iter().last();
    assert_eq!(list.free_chunk.as_ref().unwrap().capacity(), 8);
    assert_equal(get_chunk_capacities(list), [8]);

    event.extend(0..32);
    assert!(list.free_chunk.is_none());
    assert_equal(get_chunk_capacities(list), [8, 8, 16, 16]);
}

#[test]
fn resize_test(){
    let event = EventQueue::<usize, S>::new();
    let mut reader = event.subscribe();
    let list = unsafe{ &*(event.list.lock().deref() as *const List<usize, S>) };

    event.extend(0..32);
    assert_equal(get_chunk_capacities(&*list), [4,4,8,8,16]);

    event.resize_chunk(6);
    assert_equal(get_chunk_capacities(&*list), [4,4,8,8,16,6]);
    assert_equal(get_chunk_lens(&*list), [4,4,8,8,8,0]);

    event.push(32);
    assert_equal(get_chunk_capacities(&*list), [4,4,8,8,16,6]);
    assert_equal(get_chunk_lens(&*list), [4,4,8,8,8,1]);

    reader.iter().last();
    assert_equal(get_chunk_capacities(&*list), [6]);
    assert_equal(get_chunk_lens(&*list), [1]);

    event.extend(0..6);
    assert_equal(get_chunk_capacities(&*list), [6,6]);
    assert_equal(get_chunk_lens(&*list), [6, 1]);
}

#[test]
fn truncate_front_test(){
    let event = EventQueue::<usize, S>::new();
    let list = unsafe{ &*(event.list.lock().deref() as *const List<usize, S>) };
    let mut reader = event.subscribe();

    event.extend(0..26);
    assert_equal(get_chunk_capacities(&*list), [4,4,8,8,16]);

    // basic
    event.truncate_front(4);
    reader.update_position();
    assert_equal(get_chunk_capacities(&*list), [8,16]);
    assert_equal(reader.iter().copied(), [22, 23, 24, 25]);


    // more then queue
    event.extend(0..5);
    event.truncate_front(10);
    assert_equal(reader.iter().copied(), 0..5 as usize);

    // clear all queue
    event.extend(0..5);
    event.truncate_front(0);
    assert_equal(reader.iter().copied(), []);
}