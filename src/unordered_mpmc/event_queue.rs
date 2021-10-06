use std::pin::Pin;
use spin::mutex::SpinMutex;
use crate::event_queue::{EventQueue as BaseEventQueue, Settings};
use crate::sync::{Arc, AtomicUsize, Ordering};
use crate::unordered_mpmc::{CPU_COUNT, DefaultBaseSettings};

pub struct EventQueue<T>{
    // TODO: get rid of Arc here, and  make Pin<Arc<Self>>
    pub(crate) queue_array: [Pin<Arc<BaseEventQueue<T, DefaultBaseSettings>>>; CPU_COUNT],

    // TODO: try add writes_counter, to speed up empty queue check on EventReader side

    //rewind_forward_mutex : SpinMutex<()>
}

impl<T> EventQueue<T>{
    #[inline]
    pub fn new() -> Self {
        let queue_array: [Pin<Arc<BaseEventQueue<T, DefaultBaseSettings>>>; CPU_COUNT] = array_init::array_init(|_|{
            BaseEventQueue::<T, DefaultBaseSettings>::with_capacity(DefaultBaseSettings::MIN_CHUNK_SIZE)
        });

        Self{
            queue_array,
        }
    }

    // TODO: Delete this. Write through writers only
    #[inline]
    pub fn push(&self, value: T){
        let event_queue = unsafe {
            // THIS IS A MUST !!
            let cpu_id =  kernel32::GetCurrentProcessorNumber() as usize;
            self.queue_array.get_unchecked(cpu_id)
        };

        let mut list = event_queue.list.lock();
        event_queue.push(&mut list, value);
    }
}

unsafe impl<T> Send for EventQueue<T>{}
unsafe impl<T> Sync for EventQueue<T>{}