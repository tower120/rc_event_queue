use crate::event_queue::chunk::Chunk;

pub(crate) struct List<T, const CHUNK_SIZE : usize>{
    pub(crate) first: *mut Chunk<T, CHUNK_SIZE>,
    pub(crate) last : *mut Chunk<T, CHUNK_SIZE>,
    pub(crate) chunk_id_counter: usize,
}