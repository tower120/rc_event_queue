use events::event::{Event, EventSettings};
use std::sync::atomic::{AtomicUsize, Ordering};
use itertools::{Itertools, assert_equal};
use events::EventReader::EventReader;

//#[derive(Clone, Eq, PartialEq, Hash)]
struct Data<F: FnMut()>{
    id : usize,
    name: String,
    on_destroy: F
}

impl<F: FnMut()> Data<F>{
    fn from(i:usize, on_destroy: F) -> Self {
        Self{
            id : i,
            name: i.to_string(),
            on_destroy: on_destroy
        }
    }
}

impl<F: FnMut()> Drop for Data<F>{
    fn drop(&mut self) {
        (self.on_destroy)();
    }
}

#[test]
fn push_drop_test() {
    let destruct_counter = AtomicUsize::new(0);
    let destruct_counter_ref = &destruct_counter;
    let on_destroy = ||{destruct_counter_ref.fetch_add(1, Ordering::Relaxed);};


    let mut reader_option : Option<EventReader<_, 4>> = Option::None;
    {
        let chunk_list = Event::<_, 4>::new(Default::default());
        reader_option = Option::Some(chunk_list.subscribe());

        chunk_list.push(Data::from(0, on_destroy));
        chunk_list.push(Data::from(1, on_destroy));
        chunk_list.push(Data::from(2, on_destroy));
        chunk_list.push(Data::from(3, on_destroy));

        chunk_list.push(Data::from(4, on_destroy));

        let reader = reader_option.as_mut().unwrap();
        assert_equal(
            reader.iter().map(|data| &data.id),
            Vec::<usize>::from([0, 1, 2, 3, 4]).iter()
        );

        // Only first chunk should be freed
        assert!(destruct_counter.load(Ordering::Relaxed) == 4);
    }
    assert!(destruct_counter.load(Ordering::Relaxed) == 4);
    reader_option = None;
    assert!(destruct_counter.load(Ordering::Relaxed) == 5);
}

#[test]
fn read_on_full_chunk_test() {
    let destruct_counter = AtomicUsize::new(0);
    let destruct_counter_ref = &destruct_counter;
    let on_destroy = ||{destruct_counter_ref.fetch_add(1, Ordering::Relaxed);};

    {
        let chunk_list = Event::<_, 4>::new(Default::default());
        let mut reader = chunk_list.subscribe();

        chunk_list.push(Data::from(0, on_destroy));
        chunk_list.push(Data::from(1, on_destroy));
        chunk_list.push(Data::from(2, on_destroy));
        chunk_list.push(Data::from(3, on_destroy));

        assert_equal(
            reader.iter().map(|data| &data.id),
            Vec::<usize>::from([0, 1, 2, 3]).iter()
        );
        assert!(destruct_counter.load(Ordering::Relaxed) == 0);

        assert_equal(
            reader.iter().map(|data| &data.id),
            Vec::<usize>::from([]).iter()
        );
        assert!(destruct_counter.load(Ordering::Relaxed) == 0);
    }
    assert!(destruct_counter.load(Ordering::Relaxed) == 4);
}

#[test]
fn huge_push_test() {
    let event = Event::<usize, 4>::new(Default::default());
    for i in 0..100000{
        event.push(i);
    }

    let mut reader = event.subscribe();

    for i in reader.iter(){
    }
}


#[test]
fn clean_test() {
    let event = Event::<usize, 4>::new(Default::default());
    let mut reader = event.subscribe();

    event.push(0);
    event.push(1);
    event.push(2);
    event.push(3);

    event.clear();
    assert!(reader.iter().next().is_none());

    event.push(4);
    event.push(5);
    assert_equal(
        reader.iter(),
        Vec::<usize>::from([4, 5]).iter()
    );
}