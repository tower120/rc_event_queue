//! Lock-free linked list of chunks.
//! Grow only.
//!

use std::sync::atomic::{AtomicPtr, Ordering, AtomicIsize};
use crate::chunk::ChunkStorage;

struct Node<T, NodePayload, const CHUNK_SIZE : usize>{
    chunk   : ChunkStorage<T, CHUNK_SIZE>,
    payload : NodePayload
}

impl<T, NodePayload, const CHUNK_SIZE : usize> Node<T, NodePayload, CHUNK_SIZE >
{
    fn new(payload : NodePayload) -> Box<Self>{
        Box::new(Self{
            chunk   : ChunkStorage::new(),
            payload : payload
        })
    }
}

// TODO: Node impl Deref ?


// TODO: rename to chunk list
// TODO: pass ChunkStorage
struct List<T, NodePayload, const CHUNK_SIZE : usize>{
    first_node: AtomicPtr<Node<T, NodePayload, CHUNK_SIZE>>,
    last_node : AtomicPtr<Node<T, NodePayload, CHUNK_SIZE>>,

    /// -max_int == cleanup in progress
    active_writers : AtomicIsize,
}

impl<T, NodePayload, const CHUNK_SIZE : usize> List<T, NodePayload, CHUNK_SIZE >
{
    fn new(payload : NodePayload) -> Self{
        let node = Node::<T, NodePayload, CHUNK_SIZE >::new(payload);
        let node_ptr = Box::into_raw(node);
        Self{
            first_node: AtomicPtr::new(node_ptr),
            last_node : AtomicPtr::new(node_ptr),
            active_writers : AtomicIsize::new(0)
        }
    }

    fn push(&self, value: T, payload_constructor: impl FnOnce()->NodePayload){
        // let node = self.last_node.load(Ordering::Acquire);
        // if node.chunk.try_push(value).is_ok(){
        //     return;
        // }

        // make new Node

    }

    /// Only safe to call, when no-one pushing
    unsafe fn free_first_node(&mut self){
        todo!()
    }
}



#[cfg(test)]
mod tests {
    use crate::lockfree_list::List;

    #[derive(Clone, Eq, PartialEq, Hash)]
    struct Data{
        id : usize,
        name: String
    }

    struct Payload{}

    #[test]
    fn test() {
        let chunk_list = List::<Data, Payload, 4>::new(Payload{});

    }
}
