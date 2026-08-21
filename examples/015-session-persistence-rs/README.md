# 015 — Session Persistence (Rust)

Write a session to an inspectable JSONL store and load it back by ID. The
example uses a temporary directory and directly exercises the low-level store,
so it demonstrates the persistence roundtrip rather than process recovery.
Runtime-backed persistent realms use `RealmStorageProvider` and default to
SQLite.

## Concepts
- `JsonlStore` - file-based JSONL storage (simple, human-readable)
- `MemoryStore` - in-memory session storage (tests and ephemeral use)
- `SqliteSessionStore` - embedded SQLite database (production)
- `SessionFilter` - query sessions by date, limit, offset
- Session save/load roundtrip

## Storage Architecture
```
Agent ←→ StoreAdapter ←→ SessionStore trait
                              ↓
                    ┌─────────┼─────────┐
                    │         │         │
                 JsonlStore  Memory  SQLite
                 (files)    (RAM)   (sqlite DB)
```

## Run
```bash
# From the repository root
ANTHROPIC_API_KEY=sk-... ./scripts/repo-cargo run -p meerkat \
  --example 015-session-persistence --features jsonl-store
```
