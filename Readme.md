[![crates.io](https://img.shields.io/crates/v/rc_event_queue.svg)](https://crates.io/crates/rc_event_queue)
[![Docs](https://docs.rs/rc_event_queue/badge.svg)](https://docs.rs/rc_event_queue)
[![CI](https://github.com/tower120/rc_event_queue/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/tower120/rc_event_queue/actions/workflows/ci.yml)

## Reader counted event queue

Fast, concurrent FIFO event queue _(or message queue)_. Multiple consumers receive every message.

- mpmc _(multi-producer multi-consumer)_ - lock-free read, locked write.
- spmc _(single-producer multi-consumer)_ - lock-free read, lock-free write.

Write operations never block read operations. Performance consumer oriented. Mostly contiguous memory layout.

**!! Important note !!** In order to use this in practice - your readers need at least sometimes to be alive, or you need a way to touch them.
They don't need to actually consume - just calling `iter()`/`update_position()` is enough. See [Emergency cut](#emergency-cut).

### Performance

Have VERY low CPU + memory overhead. Most of the time reader just do 1 atomic load per `iter()` call. That's all! 

#### Single-threaded.

Read - close to `VecDeque`! Write:
- `mpmc` - `push` 4x slower then `VecDeque`. `extend` with at least 4 items, close to `VecDeque`. 
- `spmc` - equal to `VecDeque`!

#### Multi-threaded. 

Read - per thread performance degrades slowly, with each additional simultaneously reading thread.
_(Also remember, since `rc_event_queue` is message queue, and each reader read ALL queue -
adding more readers does not consume queue faster)_

Write - per thread performance degrades close to linearly, with each additional simultaneously writing thread. 
(Due to being locked). Not applicable to `spmc`.

[See mpmc benchmarks](doc/mpmc_benchmarks.md).

### Principle of operation

See [doc/principle-of-operation.md](doc/principle-of-operation.md). 

Short version - `EventQueue` operates on the chunk basis. `EventQueue` does not touch `EventReader`s . `EventReader`s always
"pull" from `EventQueue`. The only way `EventReader` interact with `EventQueue` - by increasing read counter
when switching to next chunk during traverse.    

### Usage

[API doc](https://docs.rs/rc_event_queue/)

```rust
use rc_event_queue::prelude::*;
use rc_event_queue::mpmc::{EventQueue, EventReader};

let event = EventQueue::<usize>::new();
let mut reader1 = EventReader::new(event);
let mut reader2 = EventReader::new(event);

event.push(1);
event.push(10);
event.push(100);
event.push(1000);

fn sum (mut iter: impl LendingIterator<ItemValue = usize>) -> usize {
    let mut sum = 0;
    while let Some(item) = iter.next() {
        sum += item;
    }
    sum
}

assert!(sum(reader1.iter()) == 1111);
assert!(sum(reader1.iter()) == 0);
assert!(sum(reader2.iter()) == 1111);
assert!(sum(reader2.iter()) == 0);

event.extend(0..10);
assert!(sum(reader1.iter()) == 55);
assert!(sum(reader2.iter()) == 55);
```

"soft clear":
```rust
event.push(1);
event.push(10);
event.clear();
event.push(100);
event.push(1000);

assert!(sum(reader1.iter()) == 1100);
assert!(sum(reader2.iter()) == 1100);
```
Where "soft" - means that any queue-cut operations does not free memory immediately. Readers should be touched for this first.

### Emergency cut

If any of the readers did not read for a long time - it can retain queue from cleanup.
This means that queue capacity will grow. On long run systems, you may want to periodically check `total_capacity`, 
and if it grows too much - you may want to force-cut/clear it.

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

#### CLEANUP

Set `CLEANUP` to `Never` in `Settings`, in order to postpone chunks deallocations.

```rust
use rc_event_reader::mpmc::{EventQueue, EventReader, Settings};

struct S{} impl Settings for S{
    const MIN_CHUNK_SIZE: u32 = 4;
    const MAX_CHUNK_SIZE: u32 = 4096;
    const CLEANUP: CleanupMode = CleanupMode::Never;
}

let event = EventQueue::<usize, S>::new();
let mut reader = event.subscribe();

event.extend(0..10);
sum(reader.iter()); // With CLEANUP != Never, this would cause chunk deallocation

...

event.cleanup();   // Free used chunks
```
#### double_buffering

Use `double_buffering` feature. This will reuse biggest freed chunk. When `EventQueue` reach its optimal size - chunks will be just swapped,
without alloc/dealloc.

### Soundness

`EventQueue` covered with tests. Also [loom](https://github.com/tokio-rs/loom) tests. See [doc/tests.md](doc/tests.md)
