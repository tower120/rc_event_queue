use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use parking_lot::lock_api::{Mutex, RawMutex};
use crate::BoolType;

pub trait CondLockable<R: RawMutex, T: ?Sized>{
    fn cond_lock<LOCK: BoolType>(&self) -> CondMutexGuard<R, T, LOCK>;
}

impl<R: RawMutex, T: ?Sized> CondLockable<R, T> for Mutex<R, T>{
    fn cond_lock<'a, LOCK: BoolType>(&'a self) -> CondMutexGuard<'a, R, T, LOCK> {
        if LOCK::VALUE {
            unsafe { self.raw().lock(); }
        }
        CondMutexGuard::<'a, R, T, LOCK>{mutex : self, _phantom : PhantomData}
    }
}

#[must_use]
pub struct CondMutexGuard<'a, R: RawMutex, T: ?Sized, LOCK: BoolType>{
    mutex: &'a Mutex<R, T>,
    _phantom : PhantomData<LOCK>
}
impl<'a, R: RawMutex, T: ?Sized, LOCK: BoolType> Drop for  CondMutexGuard<'a, R, T, LOCK>{
    fn drop(&mut self) {
        if LOCK::VALUE {
            unsafe { self.mutex.raw().unlock(); }
        }
    }
}
impl<'a, R: RawMutex, T: ?Sized, LOCK: BoolType> Deref for  CondMutexGuard<'a, R, T, LOCK>{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data_ptr() }
    }
}
impl<'a, R: RawMutex, T: ?Sized, LOCK: BoolType> DerefMut for  CondMutexGuard<'a, R, T, LOCK>{
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data_ptr() }
    }
}


#[cfg(test)]
mod test{
    use parking_lot::{Mutex};
    use crate::conditional_mutex::CondLockable;
    use crate::True;

    #[test]
    fn test(){
        let l = Mutex::new(1000);
        let l = l.cond_lock::<True>();
        assert_eq!(*l, 1000);
    }
}