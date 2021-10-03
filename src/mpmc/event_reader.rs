// new-type EventReader, mostly to hide `BS`

use crate::event_reader::{EventReader as BaseEventReader, LendingIterator};
use crate::event_reader::Iter as BaseIter;
use crate::mpmc::{BS, EventQueue, Settings};

pub struct EventReader<T, S: Settings>(BaseEventReader<T, BS<S>>);
impl<T, S: Settings> EventReader<T, S>{
    #[inline]
    pub fn new(event_queue: &EventQueue<T, S>) -> Self {
        Self{0: event_queue.0.subscribe(&mut event_queue.0.list.lock())}
    }

    #[inline]
    pub fn update_position(&mut self){
        self.0.update_position();
    }

    #[inline]
    pub fn iter(&mut self) -> Iter<T, S>{
        Iter{ 0: self.0.iter() }
    }
}

pub struct Iter<'a, T, S: Settings> (BaseIter<'a, T, BS<S>>);
impl <'a, T, S: Settings> LendingIterator for Iter<'a, T, S>{
    type ItemValue = T;

    fn next(&mut self) -> Option<&Self::ItemValue> {
        self.0.next()
    }
}