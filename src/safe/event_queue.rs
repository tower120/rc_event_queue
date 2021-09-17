use crate::event_queue::event_queue::EventQueue as BaseEventQueue;
use std::pin::Pin;
use crate::sync::{Arc, AtomicUsize, Ordering};

// TODO: if this is not safe with dyn linkage - change to random id
static EVENT_QUEUE_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct EventQueue<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool>
{
    pub(crate) base: BaseEventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>,
    pub(crate) id: usize
}

impl<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>
    EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>
{
    #[inline]
    pub fn new() -> Self {
        Self{
            base: BaseEventQueue::new(),
            id : EVENT_QUEUE_COUNTER.fetch_add(1, Ordering::AcqRel)
        }
    }

    #[inline]
    pub fn push(&self, value: T){
        self.base.push(value);
    }

    #[inline]
    pub fn extend<I>(&self, iter: I)
        where I: IntoIterator<Item = T>
    {
        self.base.extend(iter);
    }

    #[inline]
    pub fn cleanup(&self){
        self.base.cleanup();
    }

    #[inline]
    pub fn clear(&self){
        self.base.clear();
    }

    #[inline]
    pub fn truncate_front(&self, chunks_count: usize) -> usize{
        self.base.truncate_front(chunks_count)
    }

    #[inline]
    pub fn chunks_count(&self) -> usize {
        self.base.chunks_count()
    }
}