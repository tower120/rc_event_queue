#[test]
fn basic_test(){
    type EventQueue = super::EventQueue<usize>;

    let event_queue = EventQueue::new();
    event_queue.push(1);
}