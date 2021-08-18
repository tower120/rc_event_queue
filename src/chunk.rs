//! thread_safe_grow_only_arrayvec
//! Lock-free
//!
//! Elements thread-safe mutable access is not guaranteed. Read only is always thread-safe.

// TODO: remove mutable access. Remove Send, Sync bounds.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::mem::MaybeUninit;
use std::ptr;
use std::cell::UnsafeCell;

pub struct ChunkStorage<T, const CHUNK_SIZE: usize>{
    storage : UnsafeCell<[MaybeUninit<T>; CHUNK_SIZE]>,
    index_counter: AtomicUsize,
    storage_len  : AtomicUsize,

    // write in progress counter. As soon as becomes 0 - storage_len is safe to materialize
    real_len  : AtomicUsize,
    write_in_progress  : AtomicUsize,
}

unsafe impl<T, const CHUNK_SIZE: usize> Send for ChunkStorage<T, CHUNK_SIZE>
    where T: Send {}
unsafe impl<T, const CHUNK_SIZE: usize> Sync for ChunkStorage<T, CHUNK_SIZE>
    where T: Sync {}

impl<T, const CHUNK_SIZE: usize> ChunkStorage<T, CHUNK_SIZE> {
    pub fn new() -> Self {
        Self{
            storage : unsafe { MaybeUninit::uninit().assume_init() },
            index_counter: AtomicUsize::new(0),
            storage_len: AtomicUsize::new(0),

            real_len: AtomicUsize::new(0),
            write_in_progress: AtomicUsize::new(0),
        }
    }

    #[inline(always)]
    unsafe fn get_storage(&self) -> &mut [MaybeUninit<T>; CHUNK_SIZE]{
        &mut *self.storage.get()
    }

    /// Presumably, thread-safe. Lock-free
    pub fn try_push(&self, value: T) -> Result<(), ()>{
        let index = self.index_counter.fetch_add(1, Ordering::AcqRel);
        if /*unlikely*/ index >= CHUNK_SIZE{
            self.index_counter.fetch_sub(1, Ordering::AcqRel);    // undone
            return Err(());
        }

        self.write_in_progress.fetch_add(1, Ordering::Relaxed);
            unsafe {
                *self.get_storage().get_unchecked_mut(index)
                    = MaybeUninit::new(value);
            }

            let prev_len = self.storage_len.fetch_add(1, Ordering::AcqRel);
        let prev_wip = self.write_in_progress.fetch_sub(1, Ordering::AcqRel);

        if prev_wip == 1{
            self.real_len.fetch_max(prev_len+1, Ordering::AcqRel);
        }

        Ok(())
    }

    // TODO: remove. Need synchronization.
    #[inline(always)]
    pub unsafe fn get_unchecked(&self, index: usize) -> &T{
        self.get_storage().get_unchecked(index).assume_init_ref()
    }

    // TODO: remove. Need synchronization.
    #[inline(always)]
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T{
        self.get_storage().get_unchecked_mut(index).assume_init_mut()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T>{
        // synchronization through storage_len Acquire
        let len = self.len();
        // TODO: benchmark slice vs [0..len].iter(). Maybe custom iterator? or "unchecked slice"?
        unsafe { self.get_storage()[0..len].iter().map(|i| i.assume_init_ref()) }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.storage_len.load(Ordering::Acquire)
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        CHUNK_SIZE
    }
}

impl<T, const CHUNK_SIZE: usize> Drop for ChunkStorage<T, CHUNK_SIZE> {
    fn drop(&mut self) {
        let len = self.len();
        let storage  = unsafe{ self.get_storage() };
        for i in 0..len {
             unsafe{ ptr::drop_in_place(storage.get_unchecked_mut(i).as_mut_ptr()); }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::chunk::ChunkStorage;
    use std::thread;
    use std::sync::Arc;
    use itertools::{Itertools, assert_equal};

    #[derive(Clone, Eq, PartialEq, Hash, Debug)]
    struct Data{
        id : usize,
        name: String
    }

    #[test]
    fn test() {
        let mut storage = ChunkStorage::<Data, 4>::new();
        assert!(storage.capacity() == 4);
        assert!(storage.len() == 0);

        assert!(
            storage.try_push(Data{id:0, name:String::from("0")})
        .is_ok());

        assert!(storage.len() == 1);
        assert!(unsafe { storage.get_unchecked_mut(0) }.id == 0);

        // test try_push fail
        assert!(
            storage.try_push(Data{id:1, name:String::from("1")})
        .is_ok());
        assert!(
            storage.try_push(Data{id:2, name:String::from("2")})
        .is_ok());
        assert!(
            storage.try_push(Data{id:3, name:String::from("3")})
        .is_ok());
        assert!(
            storage.try_push(Data{id:4, name:String::from("4")})
        .is_err());

        // iterator test
        assert_equal(storage.iter(), [
            Data{id: 0, name: String::from("0")},
            Data{id: 1, name: String::from("1")},
            Data{id: 2, name: String::from("2")},
            Data{id: 3, name: String::from("3")}
        ].iter());
    }

    fn fill_test_data<const CHUNK_SIZE: usize>(
        original: &mut Vec<Data>, storage: &mut Arc<ChunkStorage<Data, CHUNK_SIZE>>,
        threads_count: usize,
        per_thread_elements: usize
    ){
        // original
        for i in 0..threads_count*per_thread_elements{
            original.push(Data{id : i, name : i.to_string()});
        }

        // push to storage
        let mut threads = Vec::new();
        for t in 0..threads_count{
            let storage = storage.clone();
            let thread = thread::spawn(move || {
                let from = t*per_thread_elements;
                let to = from+per_thread_elements;
                for i in from..to{
                    let _ = storage.try_push(Data{id : i, name : i.to_string()});
                }
            });
            threads.push(thread);
        }

        for thread in threads{
            thread.join().unwrap();
        }
    }

    #[test]
    fn mt_exact_size_test() {
        let mut original = Vec::new();
        let mut storage = Arc::new(ChunkStorage::<Data, 10000>::new());
        let threads_count = 10;
        let per_thread_elements = 1000;

        // fill in
        fill_test_data(&mut original, &mut storage, threads_count, per_thread_elements);

        // verify
        let mut storage_data = Vec::new();
        for i in 0..storage.len(){
            let data = unsafe { storage.get_unchecked(i) };
            storage_data.push(data.clone());
        }
        storage_data.sort_by_key(|data|data.id);

        assert!(storage_data.len() == original.len());
        for i in 0..storage.len(){
            assert!(storage_data[i] == original[i]);
        }
    }

    #[test]
    fn mt_excess_size_test() {
        let mut original = Vec::new();
        let mut storage = Arc::new(ChunkStorage::<Data, 10000>::new());
        let threads_count = 10;
        let per_thread_elements = 3000;

        // fill in
        fill_test_data(&mut original, &mut storage, threads_count, per_thread_elements);

        // verify
        let mut storage_data = Vec::new();
        for i in 0..storage.len(){
            let data = unsafe { storage.get_unchecked(i) };
            storage_data.push(data.clone());
        }
        storage_data.sort_by_key(|data|data.id);
        assert!(storage.len() == storage.capacity());
        assert!(storage_data.iter().all_unique());
        for data in &storage_data{
            assert!(original.contains(data));
        }
    }


}
