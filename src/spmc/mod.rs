//! Single-producer multi-consumer.
//!
//! Same as [mpmc](crate::mpmc), but writes without lock.
//!
//! [CleanupMode::OnChunkRead] is not available for spmc! _Since there is no more lock - reader can not
//! safely call cleanup._

mod event_queue;
mod event_reader;

use std::marker::PhantomData;
use crate::event_queue::Settings as BaseSettings;
use crate::CleanupMode;

pub use event_queue::*;
pub use event_reader::*;

pub trait Settings{
    const MIN_CHUNK_SIZE : u32 = 4;
    const MAX_CHUNK_SIZE : u32 = 4096;
    const CLEANUP: CleanupMode = CleanupMode::OnNewChunk;
}

pub struct DefaultSettings{}
impl Settings for DefaultSettings{}

/// spmc::Settings -> event_queue::Settings
pub(crate) struct BS<S: Settings>{
    _phantom: PhantomData<S>
}
impl<S: Settings> BaseSettings for BS<S>{
    const MIN_CHUNK_SIZE : u32 = S::MIN_CHUNK_SIZE;
    const MAX_CHUNK_SIZE : u32 = S::MAX_CHUNK_SIZE;
    const CLEANUP: CleanupMode = S::CLEANUP;
    const LOCK_ON_NEW_CHUNK_CLEANUP: bool = true;
    const CLEANUP_IN_UNSUBSCRIBE: bool = false;
}