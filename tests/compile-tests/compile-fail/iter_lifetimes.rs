extern crate rc_event_queue;

use rc_event_queue::mpmc::{EventQueue, EventReader, Iter};

fn main() {
    let event = EventQueue::<usize>::new();
    let mut reader = event.subscribe();

    event.extend(0..10);

    let v = 100;
    let mut i: &usize = &v;
    {
        let mut iter = reader.iter();
        let item = iter.next().unwrap(); //~ ERROR `iter` does not live long enough
        i = item;
    }
    assert_eq!(*i, 100);
}