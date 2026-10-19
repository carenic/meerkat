# Meta 0.9.3 gateway runtime.sqlite fixture

`runtime.sqlite` is the runtime store a published Meta 0.9.3 persistent
gateway (`rpc_gateway --persistent`) left in its state directory. That gateway
composes a standalone WholeBlob `SqliteRuntimeStore` (`SqliteRuntimeStore::new`)
on `runtime.sqlite` and keeps its session store in another file, so this
database holds WholeBlob runtime authority and bodies, every runtime queued for
HeadCanonical activation, and no session-store tables at all.

The SQLite store tests in `src/store/sqlite.rs` use it as the release-boundary
evidence that `SqliteRuntimeStore::convert_standalone_whole_blob_in_place`
converts a standalone WholeBlob runtime database in place to the HeadCanonical
profile, co-located with `SqliteSessionStore` in the same file, and that
`SqliteRuntimeStore::new_head_canonical` keeps refusing it. They bind every
byte of the file to `fixture-manifest.json` before opening a temporary copy,
and never write the committed file.

## Provenance

- Producer: the published `meta-rpc-gateway-0.9.3-x86_64-unknown-linux-gnu`
  release asset of the public Meerkat Meta `v0.9.3` release; the manifest
  records the asset and binary SHA-256 and the binary's version output.
- Inputs were synthetic from the start: a loopback chat-completions model that
  answers every request with a numbered reply, and a persistent gateway with
  no roster provider and no provider credentials.
- Three boots over one state directory, each ending in a clean
  `meta/shutdown`:
  1. two members, three turns each; the first member's session compacts once
     (its body carries one transcript rewrite);
  2. three more turns for the first member;
  3. a third member with three turns.

The file was copied after the last clean shutdown, with no WAL sidecar. It
holds three sessions: one with a transcript rewrite and two plain ones.

## Regenerating

Do not regenerate this file with current code: its value is that the
published 0.9.3 binary wrote it. A replacement must come from a published
release binary in the same way, with a new manifest entry for every changed
byte.
