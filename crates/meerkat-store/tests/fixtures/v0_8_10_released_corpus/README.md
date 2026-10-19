A byte copy of the released 0.8.10 corpus database. Its owner is
`crates/meerkat-runtime/tests/fixtures/v0_8_10_released_realm/corpus/realm/sessions.sqlite3`,
which that corpus's `fixture-manifest.json` binds by SHA-256 (see its README).
The copy lets meerkat-store's published package carry the bytes that
`sqlite_store::tests::exact_released_0810_corpus_imports_and_fully_loads_current_session`
imports. The workspace-only test `tests/released_0810_corpus_copy.rs` fails
when the copy and its owner differ, or when this file stops naming the owner.
