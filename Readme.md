## Reader counted event queue

Fast, multi-producer multi-consumer / single-producer multi-consumer FIFO queue. Lockless read, locked write. Writes, does not block reads. 
Consumer oriented. Contiguous memory layout.

Have very low CPU+memory overhead. Single-thread read performance close to `VecDeque`. 
Write performance, using `EventQueue::extend` with at least 4 items, close to `VecDeque` as well. [See benchmarks](doc/benchmarks.md).

[Principle of operation](doc/principal-of-operation.md). Short version - EventQueue does not know where it's readers exactly are. 
It operates on the chunk basis. Hence - exact lower bound is unkown. 

```rust
let event = EventQueue::<usize>::new();
let mut reader = event.subscribe();

event.push(1);
event.push(10);
event.push(100);
event.push(1000);

assert!(reader.iter().sum() == 1111);
assert!(reader.iter().sum() == 0);

event.extend(0..10);
assert!(reader.iter().sum() == 55);
```

"soft clear":
```rust
event.push(1);
event.push(10);
event.clear();
event.push(100);
event.push(1000);

assert!(reader.iter().sum() == 1100);
```
This means that any queue-cut operations does not free memory immediately. Readers should be touched for this first.

### Emergency cut

If any of the readers did not read for a long time - it can retain queue from cleanup.
This means that queue capacity will grow. On long run systems, you may want to periodically check `total_capacity`, 
and if its grow too much - you may want to force-cut/clear it.

```rust
if event.total_capacity() > 100000{
    event.truncate_front(1000);     // leave some of the latest messages to read
    event.change_chunk_size(2048);  // reduce size of the active chunk

    // Now the tricky part - you need to have access to all readers
    for reader in readers{
        reader.update_position();
        // reader.iter();   // this have same effect as above
    }
}

```

### Optimisation

#### AUTO_CLEANUP

Disable `AUTO_CLEANUP` in `Settings`, in order to postpone chunks deallocations.

```rust
use rc_event_reader::mpmc::{EventQueue, EventReader, Settings};

struct S{} impl Settings for S{
    const MIN_CHUNK_SIZE: u32 = 4;
    const MAX_CHUNK_SIZE: u32 = 4096;
    const AUTO_CLEANUP: bool = false;
}

let event = EventQueue::<usize, S>::new();
let mut reader = event.subscribe();

event.extend(0..10);
reader.iter().last(); // With AUTO_CLEANUP = true, this would cause chunk deallocation

...

event.cleanup();   // Free used chunks
```
#### double_buffering

Use `double_buffering` feature. This will reuse chunk. When `EventQueue` reach its optimal size - chunks will be just swapped,
without alloc/dealloc.

### Soundness

`EventQueue` covered with tests. Also [loom](https://github.com/tokio-rs/loom) tests. See [doc/tests.md](doc/tests.md)
