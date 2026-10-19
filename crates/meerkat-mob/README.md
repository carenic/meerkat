# meerkat-mob

Multi-agent orchestration runtime for Meerkat.

Part of [Meerkat](https://github.com/lukacf/meerkat); documentation at
<https://docs.rkat.ai>.

## Testing

The published package carries its own tests. In the unpacked crate, run them
with the features they exercise and the 8 MiB thread stack the Meerkat
workspace runs them with (they are not checked at Rust's 2 MiB default):

```bash
RUST_MIN_STACK=8388608 cargo test --features test-support
```

Plain `cargo test` builds too; it runs only the tests the default features
compile in.
