## Loom tests

Take approx. 15 min

Linux:

```
RUSTFLAGS="--cfg loom" cargo test --lib tests::loom_test --release
```

Windows (powershell):

```
$env:RUSTFLAGS="--cfg loom";
cargo test --lib tests::loom_test --release
```


