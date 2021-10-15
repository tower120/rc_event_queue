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

    /// Free all completely read chunks.
    ///
    /// Called automatically with [Settings::CLEANUP] != Never.
    #[inline]
    pub fn cleanup(&self){
        self.0.cleanup();
    }

    /// "Lazily move" all readers positions to the "end of the queue". From readers perspective,
    /// equivalent to conventional `clear`.
    ///
    /// Immediately free all **unoccupied** chunks.
    ///
    /// "End of the queue" - is the queue's end position at the moment of the `clear` call.
    ///
    /// "Lazy move" - means that reader actually change position and free occupied chunk,
    /// only when actual read starts.
    #[inline]
    pub fn clear(&self){
        let mut list = self.0.list.lock();
        self.0.clear(&mut list);
    }

    /// "Lazily move" all readers positions to the `len`-th element from the end of the queue.
    /// From readers perspective, equivalent to conventional `truncate` from the other side.
    ///
    /// Immediately free **unoccupied** chunks.
    ///
    /// "Lazy move" - means that reader actually change position and free occupied chunk,
    /// only when actual read starts.
    #[inline]
    pub fn truncate_front(&self, len: usize){
        let mut list = self.0.list.lock();
        self.0.truncate_front(&mut list, len);
    }

    /// Adds chunk with `new_capacity` capacity. All next writes will be on new chunk.
    ///
    /// If you configured [Settings::MAX_CHUNK_SIZE] to high value, use this, in conjunction
    /// with [clear](Self::clear) / [truncate_front](Self::truncate_front), to reduce
    /// memory pressure ASAP.
    ///
    /// Total capacity will be temporarily increased, until readers get to the new chunk.
    #[inline]
    pub fn change_chunk_capacity(&self, new_capacity: u32){
        let mut list = self.0.list.lock();
        self.0.change_chunk_capacity(&mut list, new_capacity);
    }

    /// Returns total chunks capacity.
    #[inline]
    pub fn total_capacity(&self) -> usize{
        let mut list = self.0.list.lock();
        self.0.total_capacity(&mut list)
    }

    /// Returns last/active chunk capacity
    #[inline]
    pub fn chunk_capacity(&self) -> usize{
        let mut list = self.0.list.lock();
        self.0.chunk_capacity(&mut list)
    }
}

unsafe impl<T, S: Settings> Send for EventQueue<T, S>{}
unsafe impl<T, S: Settings> Sync for EventQueue<T, S>{}