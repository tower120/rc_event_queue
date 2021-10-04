// Chunk's read_completely_times updated on Iter::Drop
//
// Chunk's iteration synchronization occurs around [ChunkStorage::storage_len] acquire/release access
//

use crate::sync::Ordering;
use std::ptr::null_mut;
use crate::event_queue::{CleanupMode, foreach_chunk, Settings};
use std::ops::ControlFlow::{Continue};
use crate::cursor::Cursor;
use std::mem::MaybeUninit;
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
    pub(super) fn set_forward_position(
        &mut self,
        new_position: Cursor<T, S>,
        try_cleanup: bool   /*should be generic*/
    ){
        debug_assert!(new_position >= self.position);

        let mut need_cleanup = false;
        let readers_count_min_1 =
            if try_cleanup {
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
                        !chunk.next(Ordering::Acquire).is_null()
                    );
                    let prev_read = chunk.read_completely_times().fetch_add(1, Ordering::AcqRel);

                    if try_cleanup {
                        if prev_read >= readers_count_min_1 {
                            need_cleanup = true;
                        }
                    }

                    Continue(())
                }
            );
        }

        // Cleanup (optional)
        if try_cleanup {
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
    fn do_update_start_position_and_get_chunk_state(&mut self) -> PackedChunkState {
        let event = unsafe{(*self.position.chunk).event()};
        let new_start_position = event.start_position.lock().clone();
        if self.position < new_start_position {
            self.set_forward_position(
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

        if epoch != self.start_position_epoch {
            self.start_position_epoch = epoch;
            self.do_update_start_position_and_get_chunk_state()
        } else {
            chunk_state
        }
    }

    /// Move cursor to the new position, if necessary.
    ///
    /// This will move reader to the new position, and mark all chunks between current
    /// and new position as "read".
    ///
    /// You need this only if you cleared/cut queue, and now want to force free memory.
    /// (When ALL readers mark chunk as read - it will be deleted)
    ///
    /// Functionally, this is the same as just calling `iter()` and drop it.
    // Do we actually need this as separate fn? Benchmark.
    #[inline]
    pub fn update_position(&mut self) {
        self.update_start_position_and_get_chunk_state();
    }

    // TODO: copy_iter() ?

    /// This is consuming iterator. Return references.
    /// Iterator items references should not outlive iterator.
    ///
    /// Read counters of affected chunks updated in [Iter::drop].
    #[inline]
    pub fn iter(&mut self) -> Iter<T, S>{
        Iter::new(self)
    }
}

impl<T, S: Settings> Drop for EventReader<T, S>{
    fn drop(&mut self) {
        unsafe { (*self.position.chunk).event().unsubscribe(self); }
    }
}

/// This should be rust GAT iterator. But it does not exists yet.
pub trait LendingIterator{
    type ItemValue;
    fn next(&mut self) -> Option<&Self::ItemValue>;
}

/// This is consuming iterator.
///
/// Return references. References have lifetime of Iter.
///
/// On [drop] `cleanup` may be called. See [Settings::CLEANUP].
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

            // have next chunk?
            let next_chunk = unsafe{&*self.position.chunk}.next(Ordering::Acquire);
            if next_chunk == null_mut(){
                return None;
            }

            // switch chunk
            self.position.chunk = next_chunk;
            self.position.index = 0;

            self.chunk_state = unsafe{&*next_chunk}.chunk_state(Ordering::Acquire);

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
        self.event_reader.set_forward_position(
            self.position,
            S::CLEANUP == CleanupMode::OnChunkRead
        );
    }
}