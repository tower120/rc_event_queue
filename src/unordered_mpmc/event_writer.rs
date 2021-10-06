use parking_lot::lock_api::MutexGuard;
use crate::unordered_mpmc::{CPU_COUNT, EventQueue};

// TODO : work with Arc instead of &
pub struct EventWriter<'a, T>{
    event_queue: &'a EventQueue<T>,
    queue_index: usize
}
impl<'a, T> EventWriter<'a, T>{
    #[inline]
    pub fn new(event_queue: &'a EventQueue<T>) -> EventWriter<'a, T>{
        Self{
            event_queue: event_queue,
            queue_index: 0
        }
    }

    #[inline]
    pub fn push(&mut self, value: T){
        let mut list_lock = || -> MutexGuard<_,_> {
            let initial_queue_index = self.queue_index;
            loop{
                if let Some(list) =
                    self.event_queue.queue_array[self.queue_index].list.try_lock()
                {
                    return list;
                }

                self.queue_index += 1;
                if self.queue_index >= CPU_COUNT {
                    self.queue_index = 0;
                }

                // we pass all queues? Stay on the same then.
                if self.queue_index == initial_queue_index {
                    return self.event_queue.queue_array[self.queue_index].list.lock();
                }
            }
        }();

        let event_queue = unsafe{ self.event_queue.queue_array.get_unchecked(self.queue_index) };
        event_queue.push(&mut list_lock, value);
    }
}