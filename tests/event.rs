use events::event::{EventQueue};
use std::sync::atomic::{AtomicUsize, Ordering, AtomicBool};
use itertools::{Itertools, assert_equal};
use events::EventReader::EventReader;
use std::thread;
use std::borrow::BorrowMut;
use std::sync::Arc;

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


    let mut reader_option : Option<EventReader<_, 4, true>> = Option::None;
    {
        let chunk_list = EventQueue::<_, 4, true>::new();
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
        let chunk_list = EventQueue::<_, 4, true>::new();
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
    let event = EventQueue::<usize, 4, true>::new();
    for i in 0..100000{
        event.push(i);
    }

    let mut reader = event.subscribe();

    for _ in reader.iter(){}
}


#[test]
fn clean_test() {
    let event = EventQueue::<usize, 4, true>::new();
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

#[test]
fn mt_read_test() {
for _ in 0..10{
    let threads_count = 40;
    let event = EventQueue::<usize, 512, true>::new();

    let mut readers = Vec::new();
    for _ in 0..threads_count{
        readers.push(event.subscribe());
    }

    let mut sum = 0;
    for i in 0..1000000{
        event.push(i);
        sum += i;
    }

    // read
    let mut threads = Vec::new();
    for mut reader in readers{
        let thread = Box::new(thread::spawn(move || {
            // some work here
            let local_sum: usize = reader.iter().sum();
            assert!(local_sum == sum);
        }));
        threads.push(thread);
    }

    for thread in threads{
        thread.join().unwrap();
    }
}
}

#[test]
fn mt_write_read_test() {
for _ in 0..10{
    let writer_chunk = 10000;
    let writers_thread_count = 2;
    let readers_thread_count = 4;
    let event = EventQueue::<usize, 32, true>::new();

    let mut readers = Vec::new();
    for _ in 0..readers_thread_count{
        readers.push(event.subscribe());
    }

    // write
    let mut writer_threads = Vec::new();
    for thread_id in 0..writers_thread_count{
        let event = event.clone();
        let thread = Box::new(thread::spawn(move || {
            let from = thread_id*writer_chunk;
            let to = from+writer_chunk;

            for i  in from..to{
                event.push(i);
            }
        }));
        writer_threads.push(thread);
    }

    let sum: usize = (0..writers_thread_count*writer_chunk).sum();

    // read
    let readers_stop = Arc::new(AtomicBool::new(false));
    let mut reader_threads = Vec::new();
    for mut reader in readers{
        let readers_stop = readers_stop.clone();
        let thread = Box::new(thread::spawn(move || {
            let mut local_sum: usize = 0;
            // do-while ensures that reader will try another round after stop,
            // to consume leftovers. Since iter's end/sentinel acquired at iter construction.
            loop{
                let stop = readers_stop.load(Ordering::Acquire);

                local_sum += reader.iter().sum::<usize>();

                if stop{ break; }
            }
            assert_eq!(local_sum, sum);
        }));
        reader_threads.push(thread);
    }

    for thread in writer_threads {
        thread.join().unwrap();
    }
    readers_stop.store(true, Ordering::Release);
    for thread in reader_threads {
        thread.join().unwrap();
    }
}
}