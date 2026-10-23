//! Recurrence pin: meerkat-core must contain ZERO provider-specific model
//! data.
//!
//! The B2 split (2026-04-18) wrongly folded the model catalog back into
//! `meerkat_core::model_profile`, reverting the 0.5x core/data separation.
//! The 0.7.1 extraction re-established the invariant: core owns the
//! vocabulary types and `ModelCatalog` mechanics; the `meerkat-models` crate
//! owns the canonical data and injects it as an explicit parameter
//! (dependency direction is strictly `meerkat-models -> meerkat-core`).
//!
//! These tests pin both halves of that invariant so the separation cannot be
//! silently reverted a second time.

#![allow(clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read directory {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| panic!("cannot read entry in {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// (a) meerkat-core must never depend on meerkat-models. The dependency
/// direction is models -> core; the reverse would let provider data leak
/// back into the core contract surface.
#[test]
fn core_does_not_depend_on_meerkat_models() {
    let manifest_path = repo_root().join("crates/meerkat-core/Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest_path.display()));
    assert!(
        !manifest.contains("meerkat-models"),
        "crates/meerkat-core/Cargo.toml references meerkat-models.\n\
         \n\
         PRINCIPLE: meerkat-core owns the model-catalog vocabulary and the\n\
         ModelCatalog mechanics, never the provider data. The data lives in\n\
         meerkat-models, which depends on meerkat-core — the reverse edge is\n\
         forbidden. Inject catalog data as an explicit `ModelCatalog`\n\
         parameter (canonically `meerkat_models::canonical()`) instead of\n\
         depending on the data crate from core."
    );
}

/// The shape that makes a quoted literal a provider model name, if any:
/// - a provider family and a version: `"gpt-5`, `"claude-4`, `"gemini-3`,
///   `"gemma-4`;
/// - an Anthropic model family: `"claude-opus`, `"claude-sonnet`,
///   `"claude-haiku`, `"claude-fable`;
/// - the open-weight `gpt-oss` family anywhere in the id;
/// - an open-weight parameter count closing the id: `"...-30b"`, `"...-1.5b"`.
///
/// `rest` runs from just after the opening quote; `literal` stops at the
/// closing quote or line end.
fn model_name_shape(rest: &str, literal: &str) -> Option<&'static str> {
    for prefix in ["gpt-", "claude-", "gemini-", "gemma-"] {
        if let Some(tail) = rest.strip_prefix(prefix)
            && tail.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            return Some("a provider family and a version");
        }
    }
    if let Some(tail) = rest.strip_prefix("claude-")
        && ["opus", "sonnet", "haiku", "fable"]
            .iter()
            .any(|family| tail.starts_with(family))
    {
        return Some("an Anthropic model family");
    }
    if literal.contains("gpt-oss") {
        return Some("the gpt-oss model family");
    }
    if is_parameter_count_id(literal) {
        return Some("an open-weight parameter count");
    }
    None
}

/// An id without whitespace that ends in `-<digits>b` or
/// `-<digits>.<digits>b`, the way open-weight model ids name their size.
fn is_parameter_count_id(literal: &str) -> bool {
    if literal.is_empty() || literal.len() > 64 || literal.contains(char::is_whitespace) {
        return false;
    }
    let Some(body) = literal.strip_suffix('b') else {
        return false;
    };
    let Some((_, count)) = body.rsplit_once('-') else {
        return false;
    };
    let (whole, fraction) = match count.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (count, None),
    };
    let digits = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
    digits(whole) && fraction.is_none_or(digits)
}

/// The first provider model-name literal in `text`, with its line and shape.
/// Without regex dependencies: each double quote opens a candidate literal
/// that runs to the next double quote or line end.
fn first_violation(text: &str) -> Option<String> {
    for (idx, _) in text.match_indices('"') {
        let rest = &text[idx + 1..];
        let literal = rest.split(['"', '\n']).next().unwrap_or_default();
        if let Some(shape) = model_name_shape(rest, literal) {
            let line = text[..idx].lines().count();
            let snippet: String = literal.chars().take(64).collect();
            return Some(format!("line {line}: \"{snippet}\" ({shape})"));
        }
    }
    None
}

/// Each shape the ban recognises, and the synthetic ids it leaves alone.
#[test]
fn the_ban_recognises_each_model_name_shape() {
    for (text, shape) in [
        ("let model = \"gpt-5\";", "a provider family and a version"),
        (
            "let model = \"claude-4-x\";",
            "a provider family and a version",
        ),
        (
            "let model = \"gemini-3-flash\";",
            "a provider family and a version",
        ),
        (
            "let model = \"gemma-4-e2b\";",
            "a provider family and a version",
        ),
        (
            "let model = \"claude-opus-5\";",
            "an Anthropic model family",
        ),
        (
            "let model = \"claude-sonnet-4-5\";",
            "an Anthropic model family",
        ),
        (
            "let model = \"claude-haiku-5-5\";",
            "an Anthropic model family",
        ),
        (
            "let model = \"claude-fable-5\";",
            "an Anthropic model family",
        ),
        (
            "let model = \"cerebras-gpt-oss-120b\";",
            "the gpt-oss model family",
        ),
        (
            "let model = \"muse-glimmer-30b\";",
            "an open-weight parameter count",
        ),
        (
            "let model = \"local-mixer-1.5b\";",
            "an open-weight parameter count",
        ),
    ] {
        let found = first_violation(text);
        assert!(
            found
                .as_deref()
                .is_some_and(|found| found.ends_with(&format!("({shape})"))),
            "{text} must be flagged as {shape}: {found:?}"
        );
    }
    for text in [
        "let model = \"test-anthropic-default\";",
        "let model = \"test-self-hosted-large\";",
        "# model = \"<model-id>\"",
        "let note = \"a context of 30b tokens\";",
        "let name = \"claude-code\";",
        "let name = \"gemma\";",
        "let name = \"gpt-like\";",
        "let name = \"abc-b\";",
    ] {
        assert_eq!(first_violation(text), None, "{text} is not a model name");
    }
}

/// (b) No file under crates/meerkat-core/src may contain a provider model-name
/// literal (the shapes of [`model_name_shape`]). This is a tombstone-class
/// textual ban: the presence of such a literal anywhere in core IS the
/// violation, regardless of context - real model names are provider data and
/// belong in meerkat-models (tests included; core tests use the synthetic
/// `model_profile::test_catalog` fixture).
#[test]
fn core_sources_contain_no_provider_model_literals() {
    let src_root = repo_root().join("crates/meerkat-core/src");
    let mut files = Vec::new();
    collect_files(&src_root, &mut files);
    assert!(
        !files.is_empty(),
        "crates/meerkat-core/src must not be empty"
    );

    let mut violations = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue; // non-UTF-8 assets cannot carry the literal pattern we ban
        };
        if let Some(found) = first_violation(&text) {
            violations.push(format!("{} ({found})", path.display()));
        }
    }

    assert!(
        violations.is_empty(),
        "provider model-name literals found under crates/meerkat-core/src:\n  {}\n\
         \n\
         PRINCIPLE: meerkat-core must contain ZERO provider-specific model\n\
         data — including model-name string literals in tests, fixtures, and\n\
         templates. Real model rows, defaults, and names live in the\n\
         meerkat-models crate and are injected through the explicit\n\
         `ModelCatalog` parameter. Use obviously-fake ids (e.g.\n\
         `test-anthropic-default` from model_profile::test_catalog) in core\n\
         unit tests, or move the test to meerkat-models.",
        violations.join("\n  ")
    );
}
