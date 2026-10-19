# Meta 0.9.3 gateway runtime.sqlite fixture with re-anchored bodies

`runtime.sqlite` is the runtime store a published Meta 0.9.3 persistent
gateway (`rpc_gateway --persistent`) left in its state directory after two of
its sessions compacted six times each. With the 0.9.3 default of four retained
rewrites, the gateway retired the two oldest rewrite occurrences of each of
those sessions, so their WholeBlob bodies hold re-anchored transcript graphs:
a retired base and the four retained occurrences. The other 21 runtimes hold
whole transcript graphs: four with one or two rewrite occurrences, 17 without
any.

Like `v0_9_3_meta_gateway_runtime`, the database holds WholeBlob runtime
authority and bodies, every runtime queued for HeadCanonical activation, and
no session-store tables. The SQLite store tests in `src/store/sqlite.rs` use
it as the release-boundary evidence that
`SqliteRuntimeStore::convert_standalone_whole_blob_in_place` converts
re-anchored bodies in place. They bind every byte of the file to
`fixture-manifest.json` before opening a temporary copy, and never write the
committed files.

## Storage

The repository refuses added files above 1000 KB, so the file is stored as two
byte-exact parts, `runtime.sqlite.part1` and `runtime.sqlite.part2`. The tests
concatenate the parts in order and check the size and SHA-256 of the result,
and of each part, against the manifest.

## Provenance

- Producer: the published `meta-rpc-gateway-0.9.3-x86_64-unknown-linux-gnu`
  release asset of the public Meerkat Meta `v0.9.3` release; the manifest
  records the asset and binary SHA-256.
- Inputs were synthetic from the start: a loopback chat-completions model that
  answers every request with a numbered reply, and a persistent gateway with a
  roster of member agents. A user message can make the model report an input
  cost above the auto-compaction threshold once, which compacts that member's
  session.
- Three boots over one state directory, each ending in a clean shutdown.
- One of the two re-anchored sessions was deleted under 0.9.3 and its member
  continued on a new session, so its runtime remains, bound to nothing.

The file was copied after the last clean shutdown, with no WAL sidecar.

## Regenerating

Do not regenerate this file with current code: its value is that the
published 0.9.3 binary wrote it. A replacement must come from a published
release binary in the same way, with a new manifest entry for every changed
byte.
