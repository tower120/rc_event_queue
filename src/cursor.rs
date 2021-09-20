use std::cmp::Ordering;
use crate::dynamic_chunk::DynamicChunk;
use crate::event_queue::Settings;

// TODO: Untested comparison!!
pub(super) struct Cursor<T, S: Settings>
{
    // TODO: try hide
    /// Always valid
    pub chunk: *const DynamicChunk<T, S>,
    /// in-chunk index
    pub index : usize
}

impl<T, S: Settings>Copy for Cursor<T, S> {}
impl<T, S: Settings>Clone for Cursor<T, S> {
    fn clone(&self) -> Self {
        Self{ chunk: self.chunk, index: self.index }
    }
}


impl<T, S: Settings> Cursor<T, S> {
    fn chunk_ref(&self) -> &DynamicChunk<T, S>{
        unsafe { &*self.chunk }
    }
}


impl<T, S: Settings> PartialEq for Cursor<T, S> {
    fn eq(&self, other: &Self) -> bool {
        self.chunk == other.chunk
        && self.index == other.index
    }
}
impl<T, S: Settings> Eq for Cursor<T, S>{}


impl<T, S: Settings> PartialOrd for Cursor<T, S> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }

    // TODO: Is this needed? Benchmark with/without specialized lt comparison
    fn lt(&self, other: &Self) -> bool {
        let self_chunk_id  = self.chunk_ref().id();
        let other_chunk_id = other.chunk_ref().id();

        if self_chunk_id < other_chunk_id{
            return true;
        }
        if self_chunk_id > other_chunk_id{
            return false;
        }
        return self.index < other.index;
    }
}
impl<T, S: Settings> Ord for Cursor<T, S> {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_chunk_id  = self.chunk_ref().id();
        let other_chunk_id = other.chunk_ref().id();

        if self_chunk_id < other_chunk_id {
            return Ordering::Less;
        }
        if self_chunk_id > other_chunk_id {
            return Ordering::Greater;
        }
        self.index.cmp(&other.index)
    }
}