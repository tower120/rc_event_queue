// Chunk's read_completely_times updated on Iter::Drop
//
// Chunk's iteration synchronization occurs around [ChunkStorage::storage_len] acquire/release access
//

use crate::sync::Ordering;
use std::ptr::null_mut;
use std::ptr;
use std::ops::ControlFlow::{Continue, Break};
use crate::utils::U32Pair;
use crate::cursor::Cursor;
use crate::event_queue::event_queue::{foreach_chunk, EventQueue};
use crate::event_queue::chunk::Chunk;
use crate::event_reader::iter::Iter;
use std::pin::Pin;
use std::marker::PhantomData;

// align 32 works best according to benchmarks
#[repr(align(32))]
pub struct EventReader<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>
{
    pub(crate) position: Cursor<T, CHUNK_SIZE>,
    pub(crate) start_position_epoch: u32,
    pub(crate) event_queue_id: u32,
}

unsafe impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool> Send for EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>{}

/// unchecked versions - does not check if provided event is correct one
impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>
{
    pub fn new(event: Pin<&EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>) -> Self{
        event.subscribe()
    }

    pub(crate) fn set_forward_position<const TRY_CLEANUP: bool>(
        &mut self,
        event: &EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>,
        new_position: Cursor<T, CHUNK_SIZE>)
    {
        debug_assert!(new_position >= self.position);

        let mut need_cleanup = false;
        let readers_count_min_1 =
            if TRY_CLEANUP {
                // TODO: bench acquire
                event.readers.load(Ordering::Relaxed) - 1
            } else {
                0
            };

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
                    let prev_read = chunk.read_completely_times.fetch_add(1, Ordering::AcqRel);

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
                event.cleanup();
            }
        }

        // 2. Update EventReader chunk+index
        self.position = new_position;
    }

    // Have much better performance being non-inline. Occurs rarely.
    #[inline(never)]
    #[cold]
    fn do_update_start_position(&mut self, event: &EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>){
        //let event = unsafe{&*(*self.position.chunk).event};
        let new_start_position = event.start_position.lock().clone();
        if self.position < new_start_position {
            self.set_forward_position::<AUTO_CLEANUP>(event, new_start_position);
        }
    }

    pub(super) fn get_chunk_len_and_update_start_position(
        &mut self,
        event: &EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>,
        chunk: &Chunk<T, CHUNK_SIZE>,
    ) -> usize {
        let len_and_epoch = chunk.storage.len_and_epoch(Ordering::Acquire);
        let len = len_and_epoch.len();
        let epoch = len_and_epoch.epoch();

        if epoch != self.start_position_epoch {
            self.do_update_start_position(event);
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
    pub fn update_position(&mut self, event: Pin<&EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>) {
        assert!(self.event_queue_id == event.id, "Wrong EventQueue used for EventReader!");
        unsafe{
            self.update_position(event);
        }
    }

    pub unsafe fn update_position_unchecked(&mut self, event: Pin<&EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>) {
        self.get_chunk_len_and_update_start_position( &*event, unsafe{&*self.position.chunk});
    }

    // TODO: copy_iter() ?

    // TODO: rename to `read` ?
    pub fn iter<'a>(&'a mut self, event: Pin<&'a EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>) -> Iter<T, CHUNK_SIZE, AUTO_CLEANUP>{
        assert!(self.event_queue_id == event.id, "Wrong EventQueue used for EventReader!");
        unsafe{
            self.iter_unchecked(event)
        }
    }

    pub unsafe fn iter_unchecked<'a>(&'a mut self, event: Pin<&'a EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>) -> Iter<T, CHUNK_SIZE, AUTO_CLEANUP>{
        Iter::new(self, event.get_ref())
    }

    pub fn unsubscribe(self, event: Pin<&EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>){
        assert!(self.event_queue_id == event.id, "Wrong EventQueue used for EventReader!");
        unsafe {
            self.unsubscribe_unchecked(event);
        }
    }

    pub unsafe fn unsubscribe_unchecked(self, event: Pin<&EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>){
        event.unsubscribe(&self);
        std::mem::forget(self);
    }

    /// Returns true, if this is the event, that reader listens to.
    pub fn event_match(&self, event: Pin<&EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>) -> bool {
        self.event_queue_id == event.id
    }
}


impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Drop for EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>{
    fn drop(&mut self) {
        panic!("EventReader must be unsubscribed!");
    }
}