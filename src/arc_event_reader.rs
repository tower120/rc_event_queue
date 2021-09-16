use crate::event_queue::event_queue::EventQueue;
use crate::sync::Arc;
use std::pin::Pin;
use crate::event_reader::event_reader::EventReader;
use crate::event_reader::iter::Iter;
use std::mem::ManuallyDrop;

pub struct ArcEventReader<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool>{
    event_queue : Pin<Arc<EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>>,
    event_reader: ManuallyDrop<EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>>,
}

impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> ArcEventReader<T, CHUNK_SIZE, AUTO_CLEANUP>{
    pub fn new(event: Pin<Arc<EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>>) -> Self{
        Self{
            event_reader: ManuallyDrop::new(EventReader::new(event.as_ref())),
            event_queue : event
        }
    }

    pub fn update_position(&mut self) {
        unsafe{
            self.event_reader.update_position_unchecked(self.event_queue.as_ref());
        }
    }

    pub fn iter(&mut self) -> Iter<T, CHUNK_SIZE, AUTO_CLEANUP>{
        unsafe{
            self.event_reader.iter_unchecked(self.event_queue.as_ref())
        }
    }
}

impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> Drop
    for ArcEventReader<T, CHUNK_SIZE, AUTO_CLEANUP>
{
   fn drop(&mut self) {
        unsafe {
            ManuallyDrop::take(&mut self.event_reader)
                .unsubscribe_unchecked(self.event_queue.as_ref());
        }
    }
}
