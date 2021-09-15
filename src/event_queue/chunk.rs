use crate::sync::{AtomicPtr, AtomicUsize};
use crate::event_queue::chunk_storage::{ChunkStorage};
use crate::event_queue::event_queue::EventQueue;
use std::ptr::null_mut;

pub(crate) struct Chunk<T, const CHUNK_SIZE : usize>{
    /// Just to compare chunks by age/sequence fast. Brings order.
    /// Will overflow after years... So just ignore that possibility.
    pub(crate) id      : usize,
    pub(crate) next    : AtomicPtr<Self>,

    /// When == readers count, it is safe to delete this chunk.
    /// Chunk read completely if reader consumed CHUNK_SIZE'ed element.
    /// Last chunk always exists
    pub(crate) read_completely_times : AtomicUsize,

    // Keep last
    pub(crate) storage : ChunkStorage<T, CHUNK_SIZE>,
}

impl<T, const CHUNK_SIZE: usize> Chunk<T, CHUNK_SIZE>
{
    pub(crate) fn new(id: usize, epoch: u32) -> Box<Self>{
        Box::new(Self{
            id   : id,
            next : AtomicPtr::new(null_mut()),
            read_completely_times : AtomicUsize::new(0),
            storage : ChunkStorage::new(epoch),
        })
    }
}