// This is canonical variant.

use std::pin::Pin;
use crate::sync::Arc;
use crate::event_queue::{EventQueue as BaseEventQueue};
use crate::mpmc::{BS, DefaultSettings, Settings};

#[repr(transparent)]
pub struct EventQueue<T, S: Settings = DefaultSettings>(
    pub(crate) BaseEventQueue<T, BS<S>>
);

impl<T, S: Settings> EventQueue<T, S>{
    #[inline]
    pub fn new() -> Pin<Arc<Self>> {
        Self::with_capacity(S::MIN_CHUNK_SIZE)
    }

    // Hide for a while.
    #[inline]
    fn with_capacity(new_capacity: u32) -> Pin<Arc<Self>> {
        let base = BaseEventQueue::<T, BS<S>>::with_capacity(new_capacity);
        unsafe {
            let base_ptr = Arc::into_raw(Pin::into_inner_unchecked(base));
            Pin::new_unchecked(
                Arc::from_raw(base_ptr as *const Self)
            )
        }
    }

    #[inline]
    pub fn push(&self, value: T){
        let mut list = self.0.list.lock();
        self.0.push(&mut list, value);
    }

    #[inline]
    pub fn extend<I>(&self, iter: I)
        where I: IntoIterator<Item = T>
    {
        let mut list = self.0.list.lock();
        self.0.extend(&mut list, iter);
    }

    #[inline]
    pub fn cleanup(&self){
        self.0.cleanup();
    }

    #[inline]
    pub fn clear(&self){
        let mut list = self.0.list.lock();
        self.0.clear(&mut list);
    }

    #[inline]
    pub fn truncate_front(&self, len: usize){
        let mut list = self.0.list.lock();
        self.0.truncate_front(&mut list, len);
    }

    #[inline]
    pub fn change_chunk_capacity(&self, new_capacity: u32){
        let mut list = self.0.list.lock();
        self.0.change_chunk_capacity(&mut list, new_capacity);
    }

    #[inline]
    pub fn total_capacity(&self) -> usize{
        let mut list = self.0.list.lock();
        self.0.total_capacity(&mut list)
    }

    #[inline]
    pub fn chunk_capacity(&self) -> usize{
        let mut list = self.0.list.lock();
        self.0.chunk_capacity(&mut list)
    }
}

unsafe impl<T, S: Settings> Send for EventQueue<T, S>{}
unsafe impl<T, S: Settings> Sync for EventQueue<T, S>{}