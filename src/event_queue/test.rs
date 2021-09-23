use crate::event_queue::{EventQueue, foreach_chunk, Settings, List};
use std::ptr::null;
use std::ops::ControlFlow::Continue;
use itertools::assert_equal;
use std::ops::Deref;

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