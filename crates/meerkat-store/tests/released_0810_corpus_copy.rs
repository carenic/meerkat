//! meerkat-store's copy of the released 0.8.10 corpus database stays its
//! owner's bytes.
//!
//! Workspace-only: it reads meerkat-runtime's corpus, which the published
//! package cannot reach, so the package leaves this file out
//! (ci/standalone-workspace-only.toml).

#![allow(clippy::expect_used)]

use std::path::Path;

/// The owner of the copy, named by the copy's README.
const OWNER: &str =
    "crates/meerkat-runtime/tests/fixtures/v0_8_10_released_realm/corpus/realm/sessions.sqlite3";

#[test]
fn released_0810_corpus_copy_matches_its_owner() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let owner = std::fs::read(crate_root.join(
        "../meerkat-runtime/tests/fixtures/v0_8_10_released_realm/corpus/realm/sessions.sqlite3",
    ))
    .expect("read meerkat-runtime's released corpus database");
    let copy =
        std::fs::read(crate_root.join("tests/fixtures/v0_8_10_released_corpus/sessions.sqlite3"))
            .expect("read meerkat-store's copy");
    assert!(
        owner == copy,
        "tests/fixtures/v0_8_10_released_corpus/sessions.sqlite3 ({} bytes) must be a byte copy \
         of {OWNER} ({} bytes): copy the owner's file again",
        copy.len(),
        owner.len()
    );
    let readme = std::fs::read_to_string(
        crate_root.join("tests/fixtures/v0_8_10_released_corpus/README.md"),
    )
    .expect("read the copy's README");
    assert!(
        readme.contains(OWNER),
        "the copy's README must name its owner {OWNER}"
    );
}
