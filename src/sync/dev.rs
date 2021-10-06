#[cfg(loom)]
pub(crate) use loom::thread;

#[cfg(loom)]
pub(crate) use loom::sync::atomic::{AtomicBool};

#[cfg(loom)]
pub(crate) type SpinMutexGuard<'a, T> = loom::sync::MutexGuard<'a, T>;

#[cfg(loom)]
pub(crate) use loom::sync::{MutexGuard};

// ==========================================================================================

#[cfg(not(loom))]
pub(crate) use std::thread;

#[cfg(not(loom))]
pub(crate) use std::sync::atomic::{AtomicBool};

#[cfg(not(loom))]
pub(crate) use spin::mutex::{SpinMutexGuard};

#[cfg(not(loom))]
pub(crate) use parking_lot::{MutexGuard};

