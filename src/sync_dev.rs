#[cfg(loom)]
pub(crate) use loom::thread;

#[cfg(loom)]
#[allow(unused_imports)]
pub(crate) use loom::sync::atomic::{AtomicBool};

#[cfg(loom)]
#[allow(dead_code)]
pub(crate) type SpinMutexGuard<'a, T> = loom::sync::MutexGuard<'a, T>;

// ==========================================================================================

#[cfg(not(loom))]
pub(crate) use std::thread;

#[cfg(not(loom))]
#[allow(unused_imports)]
pub(crate) use std::sync::atomic::{AtomicBool};

#[cfg(not(loom))]
#[allow(unused_imports)]
pub(crate) use spin::mutex::{SpinMutexGuard};
