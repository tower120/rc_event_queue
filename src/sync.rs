#[cfg(loom)]
pub(crate) use loom::thread;
#[cfg(loom)]
pub(crate) use loom::sync::atomic::{AtomicPtr, AtomicUsize, AtomicU64, AtomicBool, Ordering};
#[cfg(loom)]
pub(crate) use loom::sync::Arc;

#[cfg(loom)]
#[derive(Debug)]
pub(crate) struct Mutex<T>(loom::sync::Mutex<T>);
#[cfg(loom)]
impl<T> Mutex<T>{
    pub(crate) fn new(data: T) -> Self {
        Self(loom::sync::Mutex::new(data))
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}
#[cfg(loom)]
pub(crate) use loom::sync::{MutexGuard};


#[cfg(loom)]
pub(crate) type SpinMutex<T> = Mutex<T>;
#[cfg(loom)]
pub(crate) type SpinMutexGuard<'a, T> = MutexGuard<'a, T>;


#[cfg(not(loom))]
pub(crate) use std::thread;
#[cfg(not(loom))]
pub(crate) use std::sync::atomic::{AtomicPtr, AtomicUsize, AtomicU64, AtomicBool, Ordering};
#[cfg(not(loom))]
pub(crate) use std::sync::Arc;
#[cfg(not(loom))]
pub(crate) use parking_lot::{Mutex, MutexGuard};
#[cfg(not(loom))]
pub(crate) use spin::mutex::{SpinMutex, SpinMutexGuard};
