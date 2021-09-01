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
use crate::utils::U32Pair;
use crate::cursor::Cursor;

pub struct EventReader<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>
{

    // TODO : chunk+index = position ?

    /// Always valid
    pub(super) event_chunk : *const Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    /// in-chunk index
    ///


    // TODO : u32 both
    pub(super) index : usize,

    pub(super) start_point_epoch : usize,
}


impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>
{
    pub(super) fn set_forward_position(
        &mut self,
        new_event_chunk : *const Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
        new_index : usize,
        try_cleanup : bool)
    {
        debug_assert!(
            Cursor{chunk: new_event_chunk, index: new_index}
            >=
            Cursor{chunk: self.event_chunk, index: self.index}
        );

        // 1. Mark passed chunks as read
        unsafe {
            foreach_chunk(
                self.event_chunk,
                new_event_chunk,
                |chunk| {
                    debug_assert!(chunk.storage.len(Ordering::Acquire) == chunk.storage.capacity());
                    chunk.read_completely_times.fetch_add(1, Ordering::AcqRel);
                    Continue(())
                }
            );
        }

        // Cleanup (optional)
        if try_cleanup{
            let event = unsafe {&*(*self.event_chunk).event};
            // Only if current chunk fully read, next chunks can be fully read to.
            // So, as fast check look for active chunk only
            let readers_count = event.readers.load(Ordering::Relaxed);
            let chunk_read_times = unsafe{(*self.event_chunk).read_completely_times.load(Ordering::Relaxed)};

            // This is fast, opportunistic Relaxed - based check without mutex lock.
            // Should happen rarely. True check inside.
            if chunk_read_times>=readers_count{
                event.free_read_chunks();
            }
        }

        // 2. Update EventReader chunk+index
        self.event_chunk = new_event_chunk;
        self.index = new_index;
    }

    // Have much better performance being non-inline. Occurs rarely.
    #[inline(never)]
    fn do_update_start_point(&mut self, event: &Event<T, CHUNK_SIZE, AUTO_CLEANUP>){
        // TODO: Copy, instead of lock!
        let new_start_point = event.start_point.lock();
        if (Cursor{chunk: self.event_chunk, index: self.index} < Cursor{chunk: new_start_point.event_chunk, index: new_start_point.index}){
            self.set_forward_position(new_start_point.event_chunk, new_start_point.index, AUTO_CLEANUP);
        }
    }

    pub fn update_position(&mut self){
        let event = unsafe { &*(*self.event_chunk).event };
        let epoch = event.start_point_epoch.load(Ordering::Acquire);
        if epoch != self.start_point_epoch{
            self.do_update_start_point(event);
            self.start_point_epoch = epoch;
        }
    }

    // TODO: rename to `read` ?
    pub fn iter(&mut self) -> Iter<T, CHUNK_SIZE, AUTO_CLEANUP>{
        self.update_position();
        Iter::new(self)
    }
}


impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Drop for EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>{
    fn drop(&mut self) {
        unsafe { (*(*self.event_chunk).event).unsubscribe(self); }
    }
}

// Having separate chunk+index, allow us to postpone marking passed chunks as read, until the Iter destruction.
// This allows to return &T instead of T
pub struct Iter<'a, T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool>
    where EventReader<T, CHUNK_SIZE, AUTO_CLEANUP> : 'a
{
    event_reader : &'a mut EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>,
    event_chunk  : &'a Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    index : usize,
    chunk_len : usize
}

impl<'a, T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Iter<'a, T, CHUNK_SIZE, AUTO_CLEANUP>{

    // #[inline(never)]
    // fn do_lock(event_reader: &'a mut EventReader<T, CHUNK_SIZE>){
    //     let event = (unsafe { &*(*event_reader.event_chunk).event });
    //     let new_start_point = event.start_point.lock();
    // }

    fn new(event_reader: &'a mut EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>) -> Self{
        let chunk = unsafe{&*event_reader.event_chunk};

        let t = chunk.storage_len_and_start_point_epoch.load(Ordering::Acquire);
        let pair = U32Pair::from_u64(t as u64);
        let chunk_len = pair.first();
         // let epoch = pair.second();
         //  if epoch as usize != event_reader.start_point_epoch{
         //      Iter::do_lock(event_reader);
         //  }


        let index = event_reader.index;
        Self{
            event_reader : event_reader,
            event_chunk  : chunk,
            index : index,
            //chunk_len : chunk.storage.storage_len.load(Ordering::Acquire)
            chunk_len : chunk_len as usize
        }
    }
}

impl<'a, T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Iterator for Iter<'a, T, CHUNK_SIZE, AUTO_CLEANUP>{
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
            self.chunk_len = chunk.storage.len(Ordering::Acquire);
            if self.chunk_len == 0 {
                return None;
            }
        }

        let value = unsafe { chunk.storage.get_unchecked(self.index) };
        self.index += 1;

        Some(value)
    }
}

impl<'a, T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Drop for Iter<'a, T, CHUNK_SIZE, AUTO_CLEANUP>{
    fn drop(&mut self) {
        self.event_reader.set_forward_position(self.event_chunk, self.index, AUTO_CLEANUP);
    }
}