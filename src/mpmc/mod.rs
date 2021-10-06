//! Multi-producer multi-consumer.
//!
//! Lock-free reading. Write under lock.

mod event_queue;
mod event_reader;

use crate::CleanupMode;
use crate::event_queue::Settings as BaseSettings;
use std::marker::PhantomData;

pub use event_queue::*;
pub use event_reader::*;

pub trait Settings{
    const MIN_CHUNK_SIZE : u32 = 4;
    const MAX_CHUNK_SIZE : u32 = 4096;
    const CLEANUP: CleanupMode = CleanupMode::OnChunkRead;
}

pub struct DefaultSettings{}
impl Settings for DefaultSettings{}

/// mpmc::Settings -> event_queue::Settings
pub(crate) struct BS<S: Settings>{
    _phantom: PhantomData<S>
}
impl<S: Settings> BaseSettings for BS<S>{
    const MIN_CHUNK_SIZE : u32 = S::MIN_CHUNK_SIZE;
    const MAX_CHUNK_SIZE : u32 = S::MAX_CHUNK_SIZE;
    const CLEANUP: CleanupMode = S::CLEANUP;
    const LOCK_ON_NEW_CHUNK_CLEANUP: bool = false;
    const CLEANUP_IN_UNSUBSCRIBE: bool = true;
}