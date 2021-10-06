use std::cell::{Cell, UnsafeCell};
use crate::event_reader::{EventReader as BaseEventReader, LendingIterator};
use crate::event_reader::Iter as BaseIter;
use crate::unordered_mpmc::{EventQueue, DefaultBaseSettings, CPU_COUNT};

pub struct EventReader<T>
{
    // TODO: move out start_position_epoch from BaseEventReader
    readers_array: [BaseEventReader<T,DefaultBaseSettings>; CPU_COUNT],
    reader_index: usize
}

impl<T> EventReader<T>{
    #[inline]
    pub fn new(event_queue: &EventQueue<T>) -> Self {
        Self{
            readers_array: array_init::array_init(|i|{
                unsafe{
                    let queue = event_queue.queue_array.get_unchecked(i);
                    queue.subscribe(&mut queue.list.lock())
                }
            }),
            reader_index: 0
        }
    }

/*
    #[inline]
    pub fn update_position(&mut self){
        self.0.update_position();
    }
*/

    /// May falsely return None. Use in loops.
    /// Calling this CPU_COUNT times is guaranteed to return all items.
    #[inline]
    pub fn relaxed_iter(&mut self) -> RelaxedIter<T>{
        let self_ptr: *mut _ = &mut *self;
        RelaxedIter {
            0: unsafe{self.readers_array.get_unchecked_mut(self.reader_index)}.iter(),
            1: self_ptr
        }
    }

    #[inline]
    pub fn iter(&mut self) -> Iter<T>{
        let self_ptr: *mut _ = &mut *self;
        Iter {
            0: unsafe{self.readers_array.get_unchecked_mut(self.reader_index)}.iter(),
            1: self_ptr
        }
    }
}

pub struct Iter<'a, T> (BaseIter<'a, T, DefaultBaseSettings>, *mut EventReader<T>);
impl <'a, T> LendingIterator for Iter<'a, T>{
    type ItemValue = T;

    #[inline]
    fn next(&mut self) -> Option<&Self::ItemValue> {
        // All raw pointers here due to troubles with NLL.
        // Polonius borrow checker should allow that without pointer hacks.
        let base_iter_ptr: *mut _ = &mut self.0;

        if let Some(value) = unsafe{&mut *base_iter_ptr}.next(){
            return Some(value);
        }

        let event_reader = unsafe{&mut *self.1};
        let initial_reader_index = event_reader.reader_index;
        loop {
            event_reader.reader_index += 1;
            if event_reader.reader_index >= CPU_COUNT {
                event_reader.reader_index = 0;
            }

            // we pass all queues? Stay on the same then.
            if event_reader.reader_index == initial_reader_index {
                return None;
            }

            *unsafe{&mut*base_iter_ptr} = unsafe{&mut *self.1}.readers_array[event_reader.reader_index].iter();
            if let Some(item) = unsafe{&mut *base_iter_ptr}.next(){
                return Some(item)
            }
        }
    }
}

pub struct RelaxedIter<'a, T> (BaseIter<'a, T, DefaultBaseSettings>, *mut EventReader<T>);
impl <'a, T> LendingIterator for RelaxedIter<'a, T>{
    type ItemValue = T;

    #[inline]
    fn next(&mut self) -> Option<&Self::ItemValue> {
        match self.0.next(){
            None => /*unlikely*/ {
                // TODO: move to drop?
                let event_reader = unsafe{&mut *self.1};
                event_reader.reader_index += 1;
                if event_reader.reader_index >= CPU_COUNT{
                    event_reader.reader_index = 0;
                }
                None
            }
            Some(value) => {Some(value)}
        }
    }
}