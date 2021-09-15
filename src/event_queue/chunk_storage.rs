//! thread_safe_grow_only_arrayvec
//! Lock-free read
//!
//! Elements thread-safe mutable access is not guaranteed. Read only is always thread-safe.

use crate::sync::{Ordering, AtomicU64};
use std::mem::MaybeUninit;
use std::ptr;
use crate::len_and_epoch::LenAndEpoch;

/// Error, indicating insufficient capacity
pub struct CapacityError<V>{
    pub value: V,
}

pub struct ChunkStorage<T, const CHUNK_SIZE: usize>{
    storage : [MaybeUninit<T>; CHUNK_SIZE],

    /// LenAndEpoch. Epoch same across all chunks. Epoch updated in all chunks at [EventQueue::clear]
    /// len fused with epoch for optimization purposes. This allow to get start_position_epoch without
    /// touching EventQueue and without additional atomic load(acquire)
    len_and_start_position_epoch: AtomicU64,
}

unsafe impl<T, const CHUNK_SIZE: usize> Send for ChunkStorage<T, CHUNK_SIZE> {}
unsafe impl<T, const CHUNK_SIZE: usize> Sync for ChunkStorage<T, CHUNK_SIZE> {}

impl<T, const CHUNK_SIZE: usize> ChunkStorage<T, CHUNK_SIZE> {
    pub fn new(epoch: u32) -> Self {
        Self{
            storage : unsafe { MaybeUninit::uninit().assume_init() },
            len_and_start_position_epoch: AtomicU64::new(LenAndEpoch::new(0, epoch).into())
        }
    }

    pub fn set_epoch(&mut self, epoch: u32, load_ordering: Ordering, store_ordering: Ordering){
        let len = self.len_and_epoch(load_ordering).len();
        self.len_and_start_position_epoch.store(
            LenAndEpoch::new(len, epoch).into(),
            store_ordering
        );
    }

    /// Needs additional synchronization, because several threads writing simultaneously may finish writes
    /// not in order, but len increases sequentially. This may cause items before len index being not fully written.
    #[inline(always)]
    pub fn try_push(&mut self, value: T, store_ordering: Ordering) -> Result<(), CapacityError<T>>{
        // Relaxed because updated only with &mut self
        let len_and_epoch: LenAndEpoch = self.len_and_start_position_epoch.load(Ordering::Relaxed).into();
        let index = len_and_epoch.len();
        let epoch = len_and_epoch.epoch();
        if (index as usize) >= CHUNK_SIZE{
            return Result::Err(CapacityError{value});
        }

        unsafe{ self.push_at(value, index, epoch, store_ordering); }

        return Result::Ok(());
    }

    #[inline(always)]
    pub unsafe fn push_unchecked(&mut self, value: T, store_ordering: Ordering){
        // Relaxed because updated only with &mut self
        let len_and_epoch: LenAndEpoch = self.len_and_start_position_epoch.load(Ordering::Relaxed).into();
        let index = len_and_epoch.len();
        let epoch = len_and_epoch.epoch();

        self.push_at(value, index, epoch, store_ordering);
    }

    #[inline(always)]
    pub(crate) unsafe fn push_at(&mut self, value: T, index: u32, epoch: u32, store_ordering: Ordering) {
        debug_assert!((index as usize) < CHUNK_SIZE);

        *self.storage.get_unchecked_mut(index as usize) = MaybeUninit::new(value);

        self.len_and_start_position_epoch.store(
            LenAndEpoch::new(index+1, epoch).into(),
            store_ordering
        );
    }

    /// Append items from iterator, until have free space
    /// Returns Ok if everything fit, CapacityError() - if not
    pub fn extend<I>(&mut self, iter: &mut I, store_ordering: Ordering) -> Result<(), CapacityError<()>>
        where I:Iterator<Item = T>
    {
        let len_and_epoch: LenAndEpoch = self.len_and_start_position_epoch.load(Ordering::Relaxed).into();
        let epoch = len_and_epoch.epoch();
        let mut index = len_and_epoch.len() as usize;

        loop {
            if index == CHUNK_SIZE{
                self.len_and_start_position_epoch.store(
                    LenAndEpoch::new(CHUNK_SIZE as u32, epoch).into(),
                    store_ordering
                );
                return Result::Err(CapacityError{value:()});
            }

            match iter.next(){
                None => {
                    self.len_and_start_position_epoch.store(
                        LenAndEpoch::new(index as u32, epoch).into(),
                        store_ordering
                    );
                    return Result::Ok(());
                }
                Some(value) => {
                    unsafe{
                        *self.storage.get_unchecked_mut(index) = MaybeUninit::new(value);
                    }
                }
            }

            index+=1;
        }
    }


    // #[inline(always)]
    // pub unsafe fn push_unchecked(&self, value: T, load_ordering: Ordering, store_ordering: Ordering){
    //     let len_and_epoch: LenAndEpoch = self.len_and_start_position_epoch.load(load_ordering).into();
    //     let index = len_and_epoch.len();
    //     let epoch = len_and_epoch.epoch();
    //     debug_assert!((index as usize) < CHUNK_SIZE);
    //
    //     *self.get_storage().get_unchecked_mut(index) = MaybeUninit::new(value);
    //
    //     self.len_and_start_position_epoch.store(
    //         LenAndEpoch::new(index+1, epoch).into(),
    //         store_ordering
    //     );
    // }

    #[inline(always)]
    pub unsafe fn get_unchecked(&self, index: usize) -> &T{
        self.storage.get_unchecked(index).assume_init_ref()
    }

    #[inline(always)]
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T{
        self.storage.get_unchecked_mut(index).assume_init_mut()
    }

    #[inline(always)]
    pub fn len_and_epoch(&self, ordering: Ordering) -> LenAndEpoch {
        self.len_and_start_position_epoch.load(ordering).into()
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        CHUNK_SIZE
    }
}

impl<T, const CHUNK_SIZE: usize> Drop for ChunkStorage<T, CHUNK_SIZE> {
    fn drop(&mut self) {
        let len = self.len_and_epoch(Ordering::Acquire).len() as usize;
        for i in 0..len {
             unsafe{ ptr::drop_in_place(self.storage.get_unchecked_mut(i).as_mut_ptr()); }
        }
    }
}

// #[cfg(test)]
// mod tests {
//     use crate::chunk::ChunkStorage;
//     use std::thread;
//     use std::sync::Arc;
//     use itertools::{Itertools, assert_equal};
//
//     #[derive(Clone, Eq, PartialEq, Hash, Debug)]
//     struct Data{
//         id : usize,
//         name: String
//     }
//
//     #[test]
//     fn test() {
//         let mut storage = ChunkStorage::<Data, 4>::new();
//         assert!(storage.capacity() == 4);
//         assert!(storage.len() == 0);
//
//         assert!(
//             storage.push(Data{id:0, name:String::from("0")})
//         .is_ok());
//
//         assert!(storage.len() == 1);
//         assert!(unsafe { storage.get_unchecked(0) }.id == 0);
//
//         // test try_push fail
//         assert!(
//             storage.try_push(Data{id:1, name:String::from("1")})
//         .is_ok());
//         assert!(
//             storage.try_push(Data{id:2, name:String::from("2")})
//         .is_ok());
//         assert!(
//             storage.try_push(Data{id:3, name:String::from("3")})
//         .is_ok());
//         assert!(
//             storage.try_push(Data{id:4, name:String::from("4")})
//         .is_err());
//
//         // iterator test
//         assert_equal(storage.iter(), [
//             Data{id: 0, name: String::from("0")},
//             Data{id: 1, name: String::from("1")},
//             Data{id: 2, name: String::from("2")},
//             Data{id: 3, name: String::from("3")}
//         ].iter());
//     }
//
//     fn fill_test_data<const CHUNK_SIZE: usize>(
//         original: &mut Vec<Data>, storage: &mut Arc<ChunkStorage<Data, CHUNK_SIZE>>,
//         threads_count: usize,
//         per_thread_elements: usize
//     ){
//         // original
//         for i in 0..threads_count*per_thread_elements{
//             original.push(Data{id : i, name : i.to_string()});
//         }
//
//         // push to storage
//         let mut threads = Vec::new();
//         for t in 0..threads_count{
//             let storage = storage.clone();
//             let thread = thread::spawn(move || {
//                 let from = t*per_thread_elements;
//                 let to = from+per_thread_elements;
//                 for i in from..to{
//                     let _ = storage.try_push(Data{id : i, name : i.to_string()});
//                 }
//             });
//             threads.push(thread);
//         }
//
//         for thread in threads{
//             thread.join().unwrap();
//         }
//     }
//
//     #[test]
//     fn mt_exact_size_test() {
//         let mut original = Vec::new();
//         let mut storage = Arc::new(ChunkStorage::<Data, 10000>::new());
//         let threads_count = 10;
//         let per_thread_elements = 1000;
//
//         // fill in
//         fill_test_data(&mut original, &mut storage, threads_count, per_thread_elements);
//
//         // verify
//         let mut storage_data = Vec::new();
//         for i in 0..storage.len(){
//             let data = unsafe { storage.get_unchecked(i) };
//             storage_data.push(data.clone());
//         }
//         storage_data.sort_by_key(|data|data.id);
//
//         assert!(storage_data.len() == original.len());
//         for i in 0..storage.len(){
//             assert!(storage_data[i] == original[i]);
//         }
//     }
//
//     #[test]
//     fn mt_excess_size_test() {
//         let mut original = Vec::new();
//         let mut storage = Arc::new(ChunkStorage::<Data, 10000>::new());
//         let threads_count = 10;
//         let per_thread_elements = 3000;
//
//         // fill in
//         fill_test_data(&mut original, &mut storage, threads_count, per_thread_elements);
//
//         // verify
//         let mut storage_data = Vec::new();
//         for i in 0..storage.len(){
//             let data = unsafe { storage.get_unchecked(i) };
//             storage_data.push(data.clone());
//         }
//         storage_data.sort_by_key(|data|data.id);
//         assert!(storage.len() == storage.capacity());
//         assert!(storage_data.iter().all_unique());
//         for data in &storage_data{
//             assert!(original.contains(data));
//         }
//     }
//
//
// }
