// Chunk's read_completely_times updated on Iter::Drop
//
// Chunk's iteration synchronization occurs around [ChunkStorage::storage_len] acquire/release access
//

use crate::sync::Ordering;
use std::ptr::{NonNull};
use crate::event_queue::{CleanupMode, EventQueue, foreach_chunk, Settings};
use std::ops::ControlFlow::{Continue};
use crate::cursor::Cursor;
use crate::chunk_state::{PackedChunkState};
use crate::StartPositionEpoch;

pub struct EventReader<T, S: Settings>
{
    pub(super) position: Cursor<T, S>,
    pub(super) start_position_epoch: StartPositionEpoch,
}

unsafe impl<T, S: Settings> Send for EventReader<T, S>{}

impl<T, S: Settings> EventReader<T, S>
{
    #[inline]
    fn fast_forward(
        &mut self,
        new_position: Cursor<T, S>,
        try_cleanup: bool   /*should be generic const*/
    ){
        // 1. Enter new_position chunk
        let new_chunk = unsafe{&*new_position.chunk};
        new_chunk.readers_entered().fetch_add(1, Ordering::AcqRel);

        // 2. Mark current chunk read
        let chunk = unsafe{&*self.position.chunk};
        let prev_read = chunk.read_completely_times().fetch_add(1, Ordering::AcqRel);
        if try_cleanup {
            // MORE or equal, just in case (this MT...). This check is somewhat opportunistic.
            if prev_read+1 >= chunk.readers_entered().load(Ordering::Acquire){
                chunk.event().cleanup();
            }
        }

        // 3. Change position
        self.position = new_position;
    }

    // Have much better performance being non-inline. Occurs rarely.
    // This is the only reason this code - is a function.
    #[inline(never)]
    #[cold]
    fn do_update_start_position_and_get_chunk_state(&mut self) -> PackedChunkState {
        let event = unsafe{(*self.position.chunk).event()};
        let new_start_position = event.start_position.lock().clone();
        if self.position < new_start_position {
            self.fast_forward(
                new_start_position,
                S::CLEANUP == CleanupMode::OnChunkRead
            );
        }

        unsafe{&*self.position.chunk}.chunk_state(Ordering::Acquire)
    }

    // Returns len of actual self.position.chunk
    #[inline]
    fn update_start_position_and_get_chunk_state(&mut self) -> PackedChunkState {
        let chunk_state = unsafe{&*self.position.chunk}.chunk_state(Ordering::Acquire);
        let epoch = chunk_state.epoch();

        if /*unlikely*/ epoch != self.start_position_epoch {
            self.start_position_epoch = epoch;
            self.do_update_start_position_and_get_chunk_state()
        } else {
            chunk_state
        }
    }

    // Do we actually need this as separate fn? Benchmark.
    #[inline]
    pub fn update_position(&mut self) {
        self.update_start_position_and_get_chunk_state();
    }

    // TODO: copy_iter() ?

    #[inline]
    pub fn iter(&mut self) -> Iter<T, S>{
        Iter::new(self)
    }
}

impl<T, S: Settings> Drop for EventReader<T, S>{
    fn drop(&mut self) {
        unsafe {
            EventQueue::<T, S>::unsubscribe(
                NonNull::from((*self.position.chunk).event()),
                self
            );
        }
    }
}

/// This should be rust GAT iterator. But it does not exists yet.
pub trait LendingIterator{
    type ItemValue;
    fn next(&mut self) -> Option<&Self::ItemValue>;
}

// Having separate chunk+index, allow us to postpone marking passed chunks as read, until the Iter destruction.
// This allows to return &T instead of T
pub struct Iter<'a, T, S: Settings>
{
    position: Cursor<T, S>,
    chunk_state : PackedChunkState,
    // &mut to ensure that only one Iter for Reader can exists
    event_reader : &'a mut EventReader<T, S>,
}

impl<'a, T, S: Settings> Iter<'a, T, S>{
    #[inline]
    fn new(event_reader: &'a mut EventReader<T, S>) -> Self{
        let chunk_state = event_reader.update_start_position_and_get_chunk_state();
        Self{
            position: event_reader.position,
            chunk_state,
            event_reader,
        }
    }
}

impl<'a, T, S: Settings> LendingIterator for Iter<'a, T, S>{
    type ItemValue = T;

    #[inline]
    fn next(&mut self) -> Option<&Self::ItemValue> {
        if /*unlikely*/ self.position.index as u32 == self.chunk_state.len(){
            // should try next chunk?
            if !self.chunk_state.has_next(){
                return None;
            }

            // acquire next chunk
            let next_chunk = unsafe{
                let chunk = &*self.position.chunk;
                let _lock = chunk.chunk_switch_mutex().read();

                let next = chunk.next(Ordering::Acquire);
                debug_assert!(!next.is_null());

                (*next).readers_entered().fetch_add(1, Ordering::AcqRel);
                &*next
            };

            // iterator switch chunk
            self.position.chunk = next_chunk;
            self.position.index = 0;
            self.chunk_state = next_chunk.chunk_state(Ordering::Acquire);

            // Maybe 0, when new chunk is created, but item still not pushed.
            // It is possible rework `push`/`extend` in the way that this situation will not exists.
            // But for now, just have this check here.
            if self.chunk_state.len() == 0 {
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
    #[inline]
    fn drop(&mut self) {
        let try_cleanup = S::CLEANUP == CleanupMode::OnChunkRead;   // should be const

        debug_assert!(self.position >= self.event_reader.position);
        let mut first_chunk_readed = 0;

        // 1. Mark passed chunks as read
        unsafe {
            foreach_chunk(
                self.event_reader.position.chunk,
                self.position.chunk,
                Ordering::Acquire,
                |chunk| {
                    debug_assert!(
                        !chunk.next(Ordering::Acquire).is_null()
                    );
                    let prev_read = chunk.read_completely_times().fetch_add(1, Ordering::AcqRel);

                    if try_cleanup {
                        if first_chunk_readed == 0{
                            first_chunk_readed = prev_read+1;
                        }
                    }

                    Continue(())
                }
            );
        }

        // Cleanup (optional)
        if try_cleanup {
            if first_chunk_readed != 0{
                let first_chunk = unsafe{&*self.event_reader.position.chunk};
                let first_chunk_readers = first_chunk.readers_entered().load(Ordering::Acquire);
                // MORE or equal, just in case (this MT...). This check is somewhat opportunistic.
                if first_chunk_readed >= first_chunk_readers {
                    first_chunk.event().cleanup();
                }
            }
        }

        // 2. Update EventReader chunk+index
        self.event_reader.position = self.position;
    }
}