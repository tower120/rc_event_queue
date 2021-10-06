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

    /// Move cursor to the new position, if necessary.
    ///
    /// This will move reader to the new position, and mark all chunks between current
    /// and new position as "read".
    ///
    /// You need this only if you cleared/cut queue, and now want to force free memory.
    /// (When ALL readers mark chunk as read - it will be deleted)
    ///
    /// Functionally, this is the same as just calling `iter()` and drop it.
    #[inline]
    pub fn update_position(&mut self){
        self.0.update_position();
    }

    /// This is consuming iterator. Return references.
    /// Iterator items references should not outlive iterator.
    ///
    /// Read counters of affected chunks updated in [Iter::drop].
    #[inline]
    pub fn iter(&mut self) -> Iter<T, S>{
        Iter{ 0: self.0.iter() }
    }
}

/// This is consuming iterator.
///
/// Return references. References have lifetime of Iter.
///
/// On [drop] `cleanup` may be called. See [Settings::CLEANUP].
pub struct Iter<'a, T, S: Settings> (BaseIter<'a, T, BS<S>>);
impl <'a, T, S: Settings> LendingIterator for Iter<'a, T, S>{
    type ItemValue = T;

    #[inline]
    fn next(&mut self) -> Option<&Self::ItemValue> {
        self.0.next()
    }
}