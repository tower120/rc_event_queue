//! Thread safe events
//! [Event::clean] call manual, for performance reasons,
//!
//! Theoretically, T can be non-clonable, if we call [Event::clean] after all systems.

use std::marker::PhantomPinned;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering};
use std::ptr::{null_mut, null};
use std::mem::MaybeUninit;
use crate::chunk::ChunkStorage;
use std::pin::Pin;
use std::ops::DerefMut;

pub const CHUNK_SIZE : usize = 4 /* 512 */;

pub(super) struct Chunk<T>
    where T:Clone
{
    pub(super) storage: ChunkStorage<T, CHUNK_SIZE>,
    /// When == readers count, it is safe to delete this chunk.
    /// Chunk read completely if reader consumed CHUNK_SIZE'ed element.
    /// Last chunk always exists
    pub(super) read_completely_times : AtomicUsize,
    pub(super) next_chunk : AtomicPtr<Chunk<T>>,
    /// This needed to access [Event::readers_count]
    event : *mut Event<T>,
}

impl<T> Chunk<T>
    where T:Clone
{
    fn new(event : *mut Event<T>) -> Box<Self>{
        return Box::new(Self{
            storage : ChunkStorage::new(),
            read_completely_times: AtomicUsize::new(0),
            next_chunk : AtomicPtr::new(null_mut()),
            event : event,
        })
    }
}


struct Event<T>
    where T:Clone
{
    first_chunk : AtomicPtr<Chunk<T>>,
    last_chunk  : AtomicPtr<Chunk<T>>,
    readers_count : AtomicUsize,
    _marker: PhantomPinned
}

impl<T> Event<T>
    where T:Clone
{
    fn new() -> Pin<Box<Self>>{
        let mut chunk = Chunk::<T>::new(null_mut());
        let mut this = Box::new(Self{
            first_chunk: AtomicPtr::new(null_mut()),
            last_chunk : AtomicPtr::new(null_mut()),
            readers_count: AtomicUsize::new(0),
            _marker: PhantomPinned
        });
        chunk.event = &mut *this;
        let chunk_ptr = Box::into_raw(chunk);
        this.first_chunk = AtomicPtr::new(chunk_ptr);
        this.last_chunk  = AtomicPtr::new(chunk_ptr);
        Pin::<Box<Self>>::from(this)
    }

    fn push(&mut self, value: T){
        todo!()
    }

    fn clean(){
        unimplemented!()
    }
}

// TODO: Drop. Clean all chunks