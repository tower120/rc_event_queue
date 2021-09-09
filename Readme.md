## Reader counted event queue.

Fast multi-producer multi-consumer message queue. Optimized for cases, where you need to write and read fast, and
there is no serious contention between threads. In cases like this performance closes to single-threaded `VecDeque` reading.

Have very low CPU+memory overhead.

See [principle of operation](doc/principal-of-operation.md).

```rust
let event = EventQueue<usize, 512, true>::new();
let mut reader = event.subscribe();

event.push(1);
event.push(10);
event.push(100);
event.push(1000);

assert!(reader.iter().sum() == 1111);
```

WIP.
