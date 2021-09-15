use crate::event_reader::event_reader::EventReader;
use crate::cursor::Cursor;
use crate::sync::Ordering;
use std::ptr::null_mut;
use crate::event_queue::event_queue::EventQueue;

// Having separate chunk+index, allow us to postpone marking passed chunks as read, until the Iter destruction.
// This allows to return &T instead of T
pub struct Iter<'a, T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool>
    where EventReader<T, CHUNK_SIZE, AUTO_CLEANUP> : 'a
{
    position: Cursor<T, CHUNK_SIZE, AUTO_CLEANUP>,
    chunk_len : usize,
    event_reader : &'a mut EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>,
    event : &'a EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>,
}

impl<'a, T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Iter<'a, T, CHUNK_SIZE, AUTO_CLEANUP>{
    pub(super) fn new(
        event_reader: &'a mut EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>,
        event : &'a EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>,
    ) -> Self
    {
        let chunk = unsafe{&*event_reader.position.chunk};
        let chunk_len = event_reader.get_chunk_len_and_update_start_position(event, chunk);
        Self{
            position: event_reader.position,
            chunk_len,
            event_reader,
            event
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

            // Maybe 0, when new chunk is created, but item still not pushed.
            // It is possible rework `push`/`extend` in the way that this situation will not exists.
            // But for now, just have this check here.
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
        self.event_reader.set_forward_position::<AUTO_CLEANUP>(self.event, self.position);
    }
}