// Chunk's read_completely_times updated on Iter::Drop
//
// Chunk's iteration synchronization occurs around [ChunkStorage::storage_len] acquire/release access
//

use std::sync::atomic::Ordering;
use std::ptr::null_mut;
use crate::event_queue::{Chunk, EventQueue, foreach_chunk};
use std::ptr;
use std::ops::ControlFlow::{Continue, Break};
use crate::utils::U32Pair;
use crate::cursor::Cursor;

pub struct EventReader<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>
{
    pub(super) position: Cursor<T, CHUNK_SIZE, AUTO_CLEANUP>,
    pub(super) start_position_epoch: u32,
}

unsafe impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> Send for EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>{}

impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>
{
    pub(super) fn set_forward_position(
        &mut self,
        new_position: Cursor<T, CHUNK_SIZE, AUTO_CLEANUP>,
        try_cleanup : bool)
    {
        debug_assert!(new_position >= self.position);

        // 1. Mark passed chunks as read
        unsafe {
            foreach_chunk(
                self.position.chunk,
                new_position.chunk,
                |chunk| {
                    debug_assert!(
                        chunk.storage.len_and_epoch(Ordering::Acquire).len() as usize
                            ==
                        chunk.storage.capacity()
                    );
                    chunk.read_completely_times.fetch_add(1, Ordering::AcqRel);
                    Continue(())
                }
            );
        }

        // Cleanup (optional)
        if try_cleanup{
            let event = unsafe {&*(*self.position.chunk).event};

            // Only if current chunk fully read, next chunks can be fully read to.
            // So, as fast check look for active chunk only
            let readers_count = event.readers.load(Ordering::Relaxed);
            let chunk_read_times = unsafe{(*self.position.chunk).read_completely_times.load(Ordering::Relaxed)};

            // This is fast, opportunistic Relaxed - based check without mutex lock.
            // Should happen rarely. True check inside.
            if chunk_read_times>=readers_count{
                event.cleanup();
            }
        }

        // 2. Update EventReader chunk+index
        self.position = new_position;
    }

    // Have much better performance being non-inline. Occurs rarely.
    #[inline(never)]
    #[cold]
    fn do_update_start_position(&mut self){
        let event = unsafe{&*(*self.position.chunk).event};
        let new_start_position = event.start_position.lock().clone();
        if self.position < new_start_position {
            self.set_forward_position(new_start_position, AUTO_CLEANUP);
        }
    }

    fn get_chunk_len_and_update_start_position(&mut self, chunk: &Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>) -> usize{
        let len_and_epoch = chunk.storage.len_and_epoch(Ordering::Acquire);
        let len = len_and_epoch.len();
        let epoch = len_and_epoch.epoch();

        if epoch != self.start_position_epoch {
            self.do_update_start_position();
            self.start_position_epoch = epoch;
        }
        len as usize
    }


    /// Will move cursor to new start_position if necessary.
    /// Reader may point to already cleared part of queue, this will move it to the new begin, marking
    /// all chunks between current and new position as read.
    ///
    /// You need this only if you cleared/cut queue, and now want to force free memory.
    /// (When all readers mark chunk as read - it will be deleted)
    ///
    // This is basically the same as just calling `iter()` and drop it.
    // Do we actually need this as separate fn? Benchmark.
    pub fn update_position(&mut self) {
        self.get_chunk_len_and_update_start_position( unsafe{&*self.position.chunk});
    }

    // TODO: copy_iter() ?

    // TODO: rename to `read` ?
    pub fn iter(&mut self) -> Iter<T, CHUNK_SIZE, AUTO_CLEANUP>{
        Iter::new(self)
    }
}


impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Drop for EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>{
    fn drop(&mut self) {
        unsafe { (*(*self.position.chunk).event).unsubscribe(self); }
    }
}

// Having separate chunk+index, allow us to postpone marking passed chunks as read, until the Iter destruction.
// This allows to return &T instead of T
pub struct Iter<'a, T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool>
    where EventReader<T, CHUNK_SIZE, AUTO_CLEANUP> : 'a
{
    position: Cursor<T, CHUNK_SIZE, AUTO_CLEANUP>,
    chunk_len : usize,
    event_reader : &'a mut EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>,
}

impl<'a, T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Iter<'a, T, CHUNK_SIZE, AUTO_CLEANUP>{
    fn new(event_reader: &'a mut EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>) -> Self{
        let chunk = unsafe{&*event_reader.position.chunk};
        let chunk_len = event_reader.get_chunk_len_and_update_start_position(chunk);
        Self{
            position: event_reader.position,
            chunk_len : chunk_len,
            event_reader : event_reader,
        }
    }
}

impl<'a, T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Iterator for Iter<'a, T, CHUNK_SIZE, AUTO_CLEANUP>{
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if /*unlikely*/ self.position.index == self.chunk_len {
            // should try next chunk?
            if self.position.index != CHUNK_SIZE{
                return None;
            }

            let mut chunk = unsafe{&*self.position.chunk};

            // have next chunk?
            let next_chunk = chunk.next.load(Ordering::Acquire);
            if next_chunk == null_mut(){
                return None;
            }

            // switch chunk
            chunk = unsafe{&*next_chunk};

            self.position.chunk = chunk;
            self.position.index = 0;

            self.chunk_len = chunk.storage.len_and_epoch(Ordering::Acquire).len() as usize;

            // Maybe 0 when new chunk is created, but item still not pushed.
            // It is faster to have rare additional check here, then in `push`
            if self.chunk_len == 0 {
                return None;
            }
        }

        let chunk = unsafe{&*self.position.chunk};
        let value = unsafe { chunk.storage.get_unchecked(self.position.index) };
        self.position.index += 1;

        Some(value)
    }
}

impl<'a, T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Drop for Iter<'a, T, CHUNK_SIZE, AUTO_CLEANUP>{
    fn drop(&mut self) {
        self.event_reader.set_forward_position(self.position, AUTO_CLEANUP);
    }
}