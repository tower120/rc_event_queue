## Reader counted event queue

Fast, multi-producer multi-consumer FIFO queue. Lockless read, fast-lock write. Writes, does not block reads. 

Have very low CPU+memory overhead. Single-thread read performance close to `VecDeque`. Write performance, using `EventQueue::extend` with >32 items, close to `VecDeque` as well. See benchmarks.

[Principle of operation](doc/principal-of-operation.md).

```rust
let event = EventQueue<usize, 512, true>::new();
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

Support "soft clear":
```rust
event.push(1);
event.push(10);
event.clear();
event.push(100);
event.push(1000);

assert!(reader.iter().sum() == 1100);
```
