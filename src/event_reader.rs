// Chunk's read_completely_times updated on Iter::Drop
//
// Chunk's iteration synchronization occurs around [ChunkStorage::storage_len] acquire/release access
//

use crate::sync::Ordering;
use std::ptr::null_mut;
use crate::event_queue::{EventQueue, foreach_chunk, Settings};
use std::ptr;
use std::ops::ControlFlow::{Continue, Break};
use crate::utils::U32Pair;
use crate::cursor::Cursor;
use crate::dynamic_chunk::DynamicChunk;
use std::mem::MaybeUninit;

pub struct EventReader<T, S: Settings>
{
    pub(super) position: Cursor<T, S>,
    pub(super) start_position_epoch: u32,
}

unsafe impl<T, S: Settings> Send for EventReader<T, S>{}

impl<T, S: Settings> EventReader<T, S>
{
    #[inline]
    pub(super) fn set_forward_position(
        &mut self,
        new_position: Cursor<T, S>,
        TRY_CLEANUP: bool   /*should be generic*/
    ){
        debug_assert!(new_position >= self.position);

        let mut need_cleanup = false;
        let readers_count_min_1 =
            if TRY_CLEANUP {
                let event = unsafe {&*(*new_position.chunk).event()};
                // TODO: bench acquire
                event.readers.load(Ordering::Relaxed) - 1
            } else {
                unsafe { MaybeUninit::uninit().assume_init() }
            };

        // 1. Mark passed chunks as read
        unsafe {
            foreach_chunk(
                self.position.chunk,
                new_position.chunk,
                |chunk| {
                    debug_assert!(
                        !chunk.next().load(Ordering::Acquire).is_null()
                    );
                    let prev_read = chunk.read_completely_times().fetch_add(1, Ordering::AcqRel);

                    if TRY_CLEANUP {
                        if prev_read >= readers_count_min_1 {
                            need_cleanup = true;
                        }
                    }

                    Continue(())
                }
            );
        }

        // Cleanup (optional)
        if TRY_CLEANUP {
            if need_cleanup{
                let event = unsafe {(*new_position.chunk).event()};
                event.cleanup();
            }
        }

        // 2. Update EventReader chunk+index
        self.position = new_position;
    }

    // Have much better performance being non-inline. Occurs rarely.
    // This is the only reason this code - is a function.
    #[inline(never)]
    #[cold]
    fn do_update_start_position_and_get_len(&mut self) -> usize /*new len*/{
        let event = unsafe{(*self.position.chunk).event()};
        let new_start_position = event.start_position.lock().clone();
        if self.position < new_start_position {
            self.set_forward_position(new_start_position, S::AUTO_CLEANUP);
        }

        // Relaxed, because under lock
        unsafe{&*self.position.chunk}.len_and_epoch(Ordering::Relaxed).len() as usize
    }

    // Returns len of actual self.position.chunk
    fn update_start_position_and_get_len(&mut self) -> usize{
        let len_and_epoch = unsafe{&*self.position.chunk}.len_and_epoch(Ordering::Acquire);
        let len = len_and_epoch.len();
        let epoch = len_and_epoch.epoch();

        if epoch != self.start_position_epoch {
            self.start_position_epoch = epoch;
            self.do_update_start_position_and_get_len()
        } else {
            len as usize
        }
    }

    /// Will move cursor to new start_position if necessary.
    /// This will move reader to the new begin, and mark all chunks between current
    /// and new position "read".
    ///
    /// You need this only if you cleared/cut queue, and now want to force free memory.
    /// (When ALL readers mark chunk as read - it will be deleted)
    ///
    /// Functionally, this is the same as just calling `iter()` and drop it.
    // Do we actually need this as separate fn? Benchmark.
    pub fn update_position(&mut self) {
        self.update_start_position_and_get_len();
    }

    // TODO: copy_iter() ?

    // TODO: rename to `read` ?
    pub fn iter(&mut self) -> Iter<T, S>{
        Iter::new(self)
    }
}


impl<T, S: Settings> Drop for EventReader<T, S>{
    fn drop(&mut self) {
        unsafe { (*self.position.chunk).event().unsubscribe(self); }
    }
}

// Having separate chunk+index, allow us to postpone marking passed chunks as read, until the Iter destruction.
// This allows to return &T instead of T
pub struct Iter<'a, T, S: Settings>
    where EventReader<T, S> : 'a
{
    position: Cursor<T, S>,
    chunk_len : usize,
    event_reader : &'a mut EventReader<T, S>,
}

impl<'a, T, S: Settings> Iter<'a, T, S>{
    fn new(event_reader: &'a mut EventReader<T, S>) -> Self{
        let chunk_len = event_reader.update_start_position_and_get_len();
        Self{
            position: event_reader.position,
            chunk_len : chunk_len,
            event_reader : event_reader,
        }
    }
}

impl<'a, T, S: Settings> Iterator for Iter<'a, T, S>{
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if /*unlikely*/ self.position.index == self.chunk_len {
            let mut chunk = unsafe{&*self.position.chunk};

            // TODO: store have_next bit in len_and_epoch. And probe here
            // should try next chunk?
            // if self.position.index != chunk.capacity(){
            //     return None;
            // }

            // have next chunk?
            let next_chunk = chunk.next().load(Ordering::Acquire);
            if next_chunk == null_mut(){
                return None;
            }

            // TODO: have_next bit in len_and_epoch.
            if chunk.len_and_epoch(Ordering::Acquire).len() as usize != self.chunk_len{
                return None;
            }

            // switch chunk
            chunk = unsafe{&*next_chunk};

            self.position.chunk = chunk;
            self.position.index = 0;

            self.chunk_len = chunk.len_and_epoch(Ordering::Acquire).len() as usize;

            // Maybe 0, when new chunk is created, but item still not pushed.
            // It is possible rework `push`/`extend` in the way that this situation will not exists.
            // But for now, just have this check here.
            if self.chunk_len == 0 {
                return None;
            }
        }

        let chunk = unsafe{&*self.position.chunk};
        let value = unsafe { chunk.get_unchecked(self.position.index) };
        self.position.index += 1;

        Some(value)
    }
}

impl<'a, T, S: Settings> Drop for Iter<'a, T, S>{
    fn drop(&mut self) {
        self.event_reader.set_forward_position(self.position, S::AUTO_CLEANUP);
    }
}