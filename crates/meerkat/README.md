# meerkat

Modular, high-performance agent harness for LLM-powered applications.

Part of [Meerkat](https://github.com/lukacf/meerkat); documentation at
<https://docs.rkat.ai>.

## Testing

The published package carries its own tests. In the unpacked crate, run them
with the features they exercise:

```bash
cargo test --features local-authorization,session-store,session-compaction,jsonl-store,memory-store,memory-store-session,comms,mcp,skills,schedule,workgraph,atif
```

Plain `cargo test` builds too; it runs only the tests the default features
compile in.
