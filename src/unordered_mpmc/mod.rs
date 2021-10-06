//! Experimental false-sharing. Also, Writers MOSTLY does not block each other!
//! Write speed grows linearly with writer threads count!
//!
//! Use this for highly concurrent write environment. Otherwise, overhead from using unordered_mpmc
//! instead of mpmc may not be justified.
//!
//! `unordered_mpmc::EventQueue` consists from N EventQueue's. Each [EventWriter] tries
//! to write to its own queue. Reader have cursor to each queue, and read them one-by-one.
//! So there is some overhead, both writer-wise and reader-wise, in compare to original mpmc.
//!

use crate::CleanupMode;
use crate::event_queue::Settings as BaseSettings;

mod event_queue;
mod event_reader;
mod event_writer;


pub use event_queue::*;
pub use event_reader::*;
pub use event_writer::*;

pub(crate) struct DefaultBaseSettings {}
impl BaseSettings for DefaultBaseSettings {
    const MIN_CHUNK_SIZE: u32 = 4;
    const MAX_CHUNK_SIZE: u32 = 4096;
    const CLEANUP: CleanupMode = CleanupMode::OnChunkRead;
    const LOCK_ON_NEW_CHUNK_CLEANUP: bool = false;
    const CLEANUP_IN_UNSUBSCRIBE: bool = true;
}

const CPU_COUNT: usize = 8;

#[cfg(test)]
mod test;
