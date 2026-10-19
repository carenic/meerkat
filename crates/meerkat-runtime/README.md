# meerkat-runtime

v9 runtime control-plane for Meerkat agent lifecycle.

Part of [Meerkat](https://github.com/lukacf/meerkat); documentation at
<https://docs.rkat.ai>.

## Testing

The published package carries its own tests. In the unpacked crate, run them
with the features they exercise:

```bash
cargo test --features test-support,sqlite-store,live,local-authorization
```

Plain `cargo test` builds too; it runs only the tests the default features
compile in.
