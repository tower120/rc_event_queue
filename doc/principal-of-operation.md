# Principal of operation

EventQueue is multi-producer, multi-consumer message queue.
Performance-wise it is biased to the readers side. All reads are lockless and very fast.
Writes happens under lock.

Single-threaded read performance is between `Vec` and `VecDeque` for best-case scenario; 1.5-2x slower then `VecDeque` - for worse.

Single-threaded write performance 4-5x slower then `VecDeque`. But writing with `EventQueue::extend` can give you `VecDeque`-like performance.

Memory-wise there is only fixed overhead. Each reader is just kind a pointer. 


## The main idea

![](images/event_queue.png)

EventQueue's storage is a single-linked list of chunks.
In order to read from it, you need to register a Reader (Reader is kinda forward iterator).
As soon as all readers processed the chunk - it is safe to free it.
Thus - we only need to track the number of readers that completely read the chunk.

Chunk's `read_count` atomic +1 operation happens only when `Reader` cross inter-chunk boundary. And that's basically 
all atomic stores for reader.

## EventQueue

```rust
struct EventQueue{
    list: Mutex<List<Chunk>>,   // this is writer lock
    readers_count: usize,
}
```

```rust
struct Chunk<T, const CAPACITY: usize>{
    storage: [T; CAPACITY],
    len: AtomicUsize,
    read_count: AtomicUsize,
    next: AtomicPtr,
    event: &Event,
}
```
Synchronization between EventQueue and Readers happens on `Chunk::len` atomic Release/Acquire.
When writer `push`, it locks `list`, write to chunk, then atomically increase `Chunk::len` (Release).

Reader on start of the read, atomically load `Chunk::len` (Acquire). This guarantees that all memory writes, that happened
prior to `len` Release will become visible on current thread (in analogue with spin lock).

## Reader

```rust
struct Reader{
    chunk: *const Chunk,
    index: usize
}
```
In order to read, `Reader` need:
1) Atomic load `chunk` len.
2) Read and increment `index` until the end of chunk reached.
3) If chunk len == chunk capacity, do atomic load `Chunk::next` and switch chunk. Else - stop. 
4) If we jumped to the next chunk - `fetch_add` `Chunk::read_count`. If `Chunk::read_count` == `EventQueue::readers_count` do `cleanup`.

As you can see, most of the time, it works like `slice`. And touch only chunk's memory.

`EventQueue::clenup` traverse event chunks, and dispose those, with read_count == readers_count. So, as soon as we increase
`Chunk::read_count`, there is a chance that chunk will be disposed. This means, that we have either return item copies, 
or postpone increasing `Chunk::read_count` until we finish read session. Currently, `event_chunk_rs` do second.

`Reader::irer()` serves in role of read session. On construction, it reads `Chunk::len`, on destruction updates `Chunk::read_count`.  

## Clear

EventQueue does not track readers, and readers does not signal/lock on read start/end.

First of all - clear - means pointing readers to the right position. To solve this, we make `Reader` to read minimal 
position from `EventQueue` on each read start, and update if necessary.

We add following fields to `EventQueue`: 
```rust
struct EventQueue{
    ...
    start_position_chunk: *const Chunk,
    start_position_index: usize,
}
```

We add notion of `start_position_epoch`. Each time `start_position` updated - `start_position_epoch` increased.
`start_position_epoch` fuses with `Chunk::len` - this way we have only one atomic load for reader. So in the end:

```rust
struct Chunk{
    ...
    len_and_epoch: AtomicUsize
}
```
Also, Reader need its own start_position_epoch:
```rust
struct Reader{
    ...
    start_position_epoch: u32
}
```

Reader, with one atomic load read both chunk len, and current start_point_epoch. 
If start_point_epoch does not match current, we update Reader's position and mark chunks in-between as read.

On `clear()` all chunks updated with new strart_epoch.

This technique has one side effect - **it does not actually free memory immediately**. Readers need to advance first.
So if you want to truncate queue due to memory limits, you need to touch all associated readers, after `clear()`.


## Optimisation techniques

_AUTO_CLEANUP=false_

_reuse chunks_