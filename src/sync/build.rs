#[cfg(loom)]
pub(crate) use loom::sync::atomic::{AtomicPtr, AtomicUsize, AtomicU64, Ordering};

#[cfg(loom)]
pub(crate) use loom::sync::Arc;

#[cfg(loom)]
#[derive(Debug)]
pub(crate) struct Mutex<T>(loom::sync::Mutex<T>);

#[cfg(loom)]
impl<T> Mutex<T>{
    pub fn new(data: T) -> Self {
        Self(loom::sync::Mutex::new(data))
    }

    pub fn lock(&self) -> loom::sync::MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }

    pub fn get_mut(&mut self) -> &mut T {
        // There is no way to get without lock in loom
        unsafe{
            use std::ops::DerefMut;
            &mut *(self.0.lock().unwrap().deref_mut() as *mut T)
        }
    }

    pub fn data_ptr(&self) -> *mut T {
        // There is no way to get without lock in loom
        unsafe{
            use std::ops::DerefMut;
            self.0.lock().unwrap().deref_mut() as *mut T
        }
    }
}

#[cfg(loom)]
pub(crate) type SpinMutex<T> = Mutex<T>;

// ==========================================================================================

#[cfg(not(loom))]
pub(crate) use std::sync::atomic::{AtomicPtr, AtomicUsize, AtomicU64, Ordering};

#[cfg(not(loom))]
pub(crate) use std::sync::Arc;

#[cfg(not(loom))]
pub(crate) use parking_lot::{Mutex};

#[cfg(not(loom))]
pub(crate) use spin::mutex::{SpinMutex};
