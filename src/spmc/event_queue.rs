use std::pin::Pin;
use crate::sync::Arc;
use crate::event_queue::{EventQueue as BaseEventQueue, List};
use crate::spmc::{BS, DefaultSettings, Settings};
use crate::CleanupMode;

/// See [mpmc](crate::mpmc::EventQueue) documentation.
///
/// Only [cleanup](EventQueue::cleanup) and `unsubscribe`(on `EventReader::drop`) are synchronized.
/// Everything else - overhead free.
///
/// Insert performance in the `std::vec::Vec` league.
pub struct EventQueue<T, S: Settings = DefaultSettings>(
    pub(crate) Arc<BaseEventQueue<T, BS<S>>>
);

impl<T, S: Settings> EventQueue<T, S>{
    #[inline]
    pub fn new() -> Self {
        Self::with_capacity(S::MIN_CHUNK_SIZE)
    }

    // Hide for a while.
    #[inline]
    fn with_capacity(new_capacity: u32) -> Self {
        assert!(S::CLEANUP!=CleanupMode::OnChunkRead, "CleanupMode::OnChunkRead is not valid mode for spmc");
        let base = BaseEventQueue::<T, BS<S>>::with_capacity(new_capacity);
        unsafe {
            let base_arc = Pin::into_inner_unchecked(base);
            Self{0: base_arc}
        }
    }

    // without lock
    #[inline]
    pub(crate) fn get_list(&self) -> &List<T, BS<S>> {
        unsafe{ &*self.0.list.data_ptr() }
    }
    // should be &mut self ... But... self-references comes later...
    #[inline]
    pub(crate) fn get_list_mut(&self) -> &mut List<T, BS<S>> {
        unsafe{ &mut *self.0.list.data_ptr() }
    }

    #[inline]
    pub fn push(&mut self, value: T){
        let list = self.get_list_mut();
        self.0.push(list, value);
    }

    #[inline]
    pub fn extend<I>(&mut self, iter: I)
        where I: IntoIterator<Item = T>
    {
        self.0.extend(self.get_list_mut(), iter);
    }

    #[inline]
    pub fn cleanup(&mut self){
        self.0.cleanup();
    }

    #[inline]
    pub fn clear(&mut self){
        self.0.clear(self.get_list_mut());
    }

    #[inline]
    pub fn truncate_front(&mut self, len: usize){
        self.0.truncate_front(self.get_list_mut(), len);
    }

    #[inline]
    pub fn change_chunk_capacity(&mut self, new_capacity: u32){
        self.0.change_chunk_capacity(self.get_list_mut(), new_capacity);
    }

    #[inline]
    pub fn total_capacity(&self) -> usize{
        self.0.total_capacity(self.get_list())
    }

    #[inline]
    pub fn chunk_capacity(&self) -> usize{
        self.0.chunk_capacity(self.get_list())
    }
}

unsafe impl<T, S: Settings> Send for EventQueue<T, S>{}
