use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::ptr::null_mut;
use crate::event::Chunk;

pub struct EventReader<T, const CHUNK_SIZE : usize>
{
    /// Always valid
    pub(super) event_chunk : *const Chunk<T, CHUNK_SIZE>,
    /// in-chunk index
    pub(super) index : usize,
}


impl<T, const CHUNK_SIZE : usize> EventReader<T, CHUNK_SIZE>
{
    /*
    // TODO: Try return scoped struct with 'a and iter over &T
    // TODO: make iter
    pub fn read(&mut self) -> Option<T>{
        let mut chunk = unsafe{&*self.event_chunk};
        if chunk.storage_len.load(Ordering::Acquire) == self.index as usize{

            // try next chunk?
            if self.index as usize == CHUNK_SIZE{
                let next_chunk = chunk.next_chunk.load(Ordering::Acquire);
                if next_chunk != null_mut(){
                    // mark that we done with that chunk only after switching chunk successfully
                    chunk.read_completely_times.fetch_add(1, Ordering::AcqRel);

                    self.event_chunk = next_chunk;
                    chunk = unsafe{&*next_chunk};
                } else {
                    return None;
                }
            }

            return None;
        }

        let value = &chunk.storage[self.index as usize];
        self.index += 1;

        Some(value.clone())
    }*/

    pub fn drain(&mut self){
        todo!()
    }
}


impl<T, const CHUNK_SIZE : usize> Drop for EventReader<T, CHUNK_SIZE>{
    fn drop(&mut self) {
        unsafe { (*(*self.event_chunk).event).unsubscribe(self); }
    }
}