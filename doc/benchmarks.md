### Bench machine
```
i7-4771 (4C/8T; 32Kb I/D L1)
DDR3-1666
Windows 10
rustc 1.5.5 stable
```

Data from benchmarks in `/benches`

## Single thread read performance

![](images/st_read_bench.svg)

As you can see, on the long session it lies between `Vec` and `VecDeque`. On very short 
sessions - it is x2 slower then `VecDeque`.

"Read session size" - is the number of items that reader consume per each `.iter()` call.

## Single thread write performance

![](images/st_write_bench.svg)

Write to `EventQueue` is **not** lockless. Hence, `EventQueue::push` is x4 times slower,
then `Vec::push` (which is not bad already). To overcome that - `EventQueue` has bulk
insert - `EventQueue::extend`. On long write sessions - it closes to `Vec::extend`.

"Write session size" - is the number of items that reader push per each `.extend()` call.

## Thread count read-performance dependency

Read performance drop non-linear, with linear threads count grow. At some point it becomes 
memory-bandwidth bound.

```
readers_start_offset_step=0; read_session_size=8096; threads_count=2/chunk:512
                        time:   [177.93 us 178.44 us 179.08 us]
readers_start_offset_step=0; read_session_size=8096; threads_count=4/chunk:512
                        time:   [237.34 us 238.26 us 239.25 us]
readers_start_offset_step=0; read_session_size=8096; threads_count=8/chunk:512
                        time:   [338.97 us 341.73 us 344.28 us]                        
```

## Chunk size dependency

The bigger the chunk - the better. After some point (4096 - on benched machine) 
performance benefits become marginal.

_Reducing chunk size and spreading reader positions over queue, in order to make each reader
read its own chunk - did not show performance benefits._

## Read session size dependency

Getting iterator from `EventReader` and crossing inter-chunk boundary (rare case) is the only potential
source of overhead. So, the more you read from the same iterator - the better.