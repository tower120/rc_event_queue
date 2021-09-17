use std::pin::Pin;
use crate::event_reader::event_reader::EventReader as BaseEventReader;
use crate::event_queue::event_queue::EventQueue as BaseEventQueue;
use crate::event_reader::iter::Iter;
use crate::safe::event_queue::EventQueue;

pub struct EventReader<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool>{
    base_event_reader: BaseEventReader<T, CHUNK_SIZE, AUTO_CLEANUP>,
    event_queue_id: usize
}

impl<T, const CHUNK_SIZE: usize, const AUTO_CLEANUP: bool> EventReader<T, CHUNK_SIZE, AUTO_CLEANUP>{
    #[inline]
    pub fn new(event: Pin<&EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>) -> Self{
        Self {
            base_event_reader: BaseEventReader::new(
                unsafe { Pin::new_unchecked(&event.base) }
            ),
            event_queue_id : event.id
        }
    }

    #[inline]
    pub fn update_position(&mut self, event: Pin<&EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>) {
        assert!(self.event_queue_id == event.id, "Wrong EventQueue used for EventReader!");
        unsafe{
            self.base_event_reader.update_position_unchecked(Pin::new_unchecked(&event.base));
        }
    }

    #[inline]
    pub fn iter<'a>(&'a mut self, event: Pin<&'a EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>) -> Iter<T, CHUNK_SIZE, AUTO_CLEANUP>{
        assert!(self.event_queue_id == event.id, "Wrong EventQueue used for EventReader!");
        unsafe{
            self.base_event_reader.iter_unchecked(Pin::new_unchecked(&event.get_ref().base))
        }
    }

    #[inline]
    pub fn unsubscribe(self, event: Pin<&EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>){
        assert!(self.event_queue_id == event.id, "Wrong EventQueue used for EventReader!");
        unsafe{
            self.base_event_reader.unsubscribe_unchecked(
                Pin::new_unchecked(&event.get_ref().base)
            )
        }
    }

    /// Returns true, if this is the event, that reader listens to.
    #[inline]
    pub fn event_match(&self, event: Pin<&EventQueue<T, CHUNK_SIZE, AUTO_CLEANUP>>) -> bool {
        self.event_queue_id == event.id
    }

    // TODO: forward _unchecked methods
}