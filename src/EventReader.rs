// Chunk's read_completely_times updated on Iter::Drop
//
// Chunk's iteration synchronization occurs around [ChunkStorage::storage_len] acquire/release access
//

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::ptr::null_mut;
use crate::event::{Chunk, Event, foreach_chunk};
use std::ptr;
use std::ops::ControlFlow::{Continue, Break};

pub struct EventReader<T, const CHUNK_SIZE : usize>
{
    /// Always valid
    pub(super) event_chunk : *const Chunk<T, CHUNK_SIZE>,
    /// in-chunk index
    pub(super) index : usize,
}


impl<T, const CHUNK_SIZE : usize> EventReader<T, CHUNK_SIZE>
{
    // TODO: rename to `read` ?
    pub fn iter(&mut self) -> Iter<T, CHUNK_SIZE>{
        // TODO: generational start check
        // {
        //     // generation usize atomic read with Acquire
        //     (*self.event).start_chunk != event_chunk
        //         ||
        //     (*self.event).start_index < index
        // }
        Iter::new(self)
    }
}


impl<T, const CHUNK_SIZE : usize> Drop for EventReader<T, CHUNK_SIZE>{
    fn drop(&mut self) {
        unsafe { (*(*self.event_chunk).event).unsubscribe(self); }
    }
}

// Having separate chunk+index, allow us to postpone marking passed chunks as read, until the Iter destruction.
// This allows to return &T instead of T
pub struct Iter<'a, T, const CHUNK_SIZE: usize>
    where EventReader<T, CHUNK_SIZE> : 'a
{
    event_reader : &'a mut EventReader<T, CHUNK_SIZE>,
    event_chunk  : &'a Chunk<T, CHUNK_SIZE>,
    index : usize,
    chunk_len : usize
}

impl<'a, T, const CHUNK_SIZE: usize> Iter<'a, T, CHUNK_SIZE>{
    fn new(event_reader: &'a mut EventReader<T, CHUNK_SIZE>) -> Self{
        let chunk = unsafe{&*event_reader.event_chunk};
        Self{
            event_reader : event_reader,
            event_chunk  : chunk,
            index : 0,
            chunk_len : chunk.storage.storage_len.load(Ordering::Acquire)
        }
    }
}

impl<'a, T, const CHUNK_SIZE: usize> Iterator for Iter<'a, T, CHUNK_SIZE>{
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let mut chunk = self.event_chunk;

        if /*unlikely*/ self.chunk_len == self.index {
            // should try next chunk?
            if self.index != CHUNK_SIZE{
                return None;
            }

            // have next chunk?
            let next_chunk = chunk.next.load(Ordering::Acquire);
            if next_chunk == null_mut(){
                return None;
            }

            // switch chunk
            chunk = unsafe{&*next_chunk};
            self.event_chunk = chunk;

            self.index = 0;
            self.chunk_len = chunk.storage.storage_len.load(Ordering::Acquire);
            if self.chunk_len == 0 {
                return None;
            }
        }

        let value = unsafe { chunk.storage.get_unchecked(self.index) };
        self.index += 1;

        Some(value)
    }
}

impl<'a, T, const CHUNK_SIZE: usize> Drop for Iter<'a, T, CHUNK_SIZE>{
    fn drop(&mut self) {
        // 1. Mark passed chunks as read
        unsafe {
            foreach_chunk(
                self.event_reader.event_chunk,
                self.event_chunk,
                |chunk| {
                    debug_assert!(chunk.storage.len() == chunk.storage.capacity());
                    chunk.read_completely_times.fetch_add(1, Ordering::AcqRel);
                    Continue(())
                }
            );
        }

        // Cleanup
        let event = unsafe {&*self.event_chunk.event};
        if event.auto_cleanup{
            let readers_count = event.readers.load(Ordering::Relaxed);
            let chunk_read_times = unsafe{(*self.event_reader.event_chunk).read_completely_times.load(Ordering::Relaxed)};

            // This is fast, opportunistic Relaxed - based check without mutex lock.
            // Should happen rarely. True check inside.
            if chunk_read_times>=readers_count{
                event.free_read_chunks();
            }
        }

        // 2. Update EventReader chunk+index
        self.event_reader.event_chunk = self.event_chunk;
        self.event_reader.index = self.index;
    }
}