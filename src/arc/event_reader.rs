use crate::sync::Arc;
use std::pin::Pin;
use crate::event_reader::event_reader::EventReader as BaseEventReader;
use crate::event_queue::event_queue::EventQueue as BaseEventQueue;
use crate::event_reader::iter::Iter;
use std::mem::ManuallyDrop;
use crate::arc::event_queue::EventQueue;

pub struct EventReader<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool>{
    event_queue       : Pin<Arc<EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>>,
    base_event_reader : ManuallyDrop<BaseEventReader<T, CHUNK_SIZE, AUTO_CLEANUP>>,
}

impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>{
    #[inline]
    pub fn new(event: Pin<Arc<EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>>) -> Self{
        Self {
            base_event_reader: ManuallyDrop::new(BaseEventReader::new(
                unsafe { Pin::new_unchecked(&event.base) }
            )),
            event_queue: event
        }
    }

    #[inline]
    pub fn update_position(&mut self) {
        unsafe{
            self.base_event_reader.update_position_unchecked(
                Pin::new_unchecked(&self.event_queue.base)
            );
        }
    }

    #[inline]
    pub fn iter(&mut self) -> Iter<T, CHUNK_SIZE, AUTO_CLEANUP>{
        unsafe{
            self.base_event_reader.iter_unchecked(
                Pin::new_unchecked(&self.event_queue.base)
            )
        }
    }
}

impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Drop
    for EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>
{
    #[inline]
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::take(&mut self.base_event_reader)
                .unsubscribe_unchecked(Pin::new_unchecked(&self.event_queue.base));
        }
    }
}
