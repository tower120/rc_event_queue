use crate::event_queue::chunk::Chunk;

pub(crate) struct List<T, const CHUNK_SIZE : usize, const AUTO_CLEANUP: bool>{
    pub(crate) first: *mut Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    pub(crate) last : *mut Chunk<T, CHUNK_SIZE, AUTO_CLEANUP>,
    pub(crate) chunk_id_counter: usize,
}