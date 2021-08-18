use std::sync::Arc;
use crate::Event::{Chunk, CHUNK_SIZE};
use std::sync::atomic::Ordering;
use std::ptr::null_mut;

struct EventReader<T>
    where T:Clone
{
    event_chunk : *mut Chunk<T>,
    /// in-chunk index
    index : usize,
}


impl<T> EventReader<T>
    where T:Clone
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
}

// TODO: Drop -1 event.readers_count