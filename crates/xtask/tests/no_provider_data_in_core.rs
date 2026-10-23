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
/// - either of those as the last segment of a provider-qualified id:
///   `"anthropic/claude-opus-5"`, `"models/gemini-3-pro"`;
/// - the open-weight `gpt-oss` family anywhere in the id;
/// - an open-weight parameter count as a segment of the id:
///   `"...-30b"`, `"...-1.5b"`, `"...-8b-instruct"`, `".../...-8B-Instruct"`.
///
/// Families outside these shapes and provider-qualified ids of other
/// families are out of scope unless they carry a parameter count.
///
/// `rest` runs from just after the opening quote; `literal` stops at the
/// closing quote or line end.
fn model_name_shape(rest: &str, literal: &str) -> Option<&'static str> {
    if let Some(shape) = family_shape(rest) {
        return Some(shape);
    }
    if let Some((_, last)) = literal.rsplit_once('/')
        && family_shape(last).is_some()
    {
        return Some("a provider-qualified model id");
    }
    if literal.contains("gpt-oss") {
        return Some("the gpt-oss model family");
    }
    if has_parameter_count_segment(literal) {
        return Some("an open-weight parameter count");
    }
    None
}

/// A provider family and a version, or an Anthropic model family, at the
/// start of `text`.
fn family_shape(text: &str) -> Option<&'static str> {
    for prefix in ["gpt-", "claude-", "gemini-", "gemma-"] {
        if let Some(tail) = text.strip_prefix(prefix)
            && tail.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            return Some("a provider family and a version");
        }
    }
    if let Some(tail) = text.strip_prefix("claude-")
        && ["opus", "sonnet", "haiku", "fable"]
            .iter()
            .any(|family| tail.starts_with(family))
    {
        return Some("an Anthropic model family");
    }
    None
}

/// An id without whitespace, at most 64 chars and not a UUID, with a segment
/// after its first (split on `-` and `/`) that is `<digits>b` or
/// `<digits>.<digits>b` (either case), the way open-weight model ids name
/// their size.
fn has_parameter_count_segment(literal: &str) -> bool {
    if literal.is_empty()
        || literal.len() > 64
        || literal.contains(char::is_whitespace)
        || is_uuid(literal)
    {
        return false;
    }
    let digits = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
    literal.split(['-', '/']).skip(1).any(|segment| {
        let Some(count) = segment
            .strip_suffix('b')
            .or_else(|| segment.strip_suffix('B'))
        else {
            return false;
        };
        match count.split_once('.') {
            Some((whole, fraction)) => digits(whole) && digits(fraction),
            None => digits(count),
        }
    })
}

/// The 8-4-4-4-12 hexadecimal shape of a UUID.
fn is_uuid(literal: &str) -> bool {
    let groups: Vec<&str> = literal.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Every provider model-name literal in `text`: its line, the literal and its
/// shape. Without regex dependencies: each double quote opens a candidate
/// literal that runs to the next double quote or line end.
fn violations(text: &str) -> Vec<(usize, String, &'static str)> {
    let mut found = Vec::new();
    for (idx, _) in text.match_indices('"') {
        let rest = &text[idx + 1..];
        let literal = rest.split(['"', '\n']).next().unwrap_or_default();
        if let Some(shape) = model_name_shape(rest, literal) {
            let line = text[..idx].lines().count();
            found.push((line, literal.chars().take(64).collect(), shape));
        }
    }
    found
}

/// The first provider model-name literal in `text`, as the ban reports it.
fn first_violation(text: &str) -> Option<String> {
    violations(text)
        .into_iter()
        .next()
        .map(|(line, literal, shape)| format!("line {line}: \"{literal}\" ({shape})"))
}

/// Literals the ban leaves in place, each with its reason. An entry that no
/// longer matches a literal fails the ban, so the list cannot go stale.
const ALLOWED: &[(&str, &str, &str)] = &[(
    "crates/meerkat-core/tests/fixtures/v0_8_10_recovery_migration_session.json",
    "gpt-5.5",
    "a session document a released 0.8.10 build wrote; its provenance file pins its bytes by sha256",
)];

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
        (
            "let model = \"anthropic/claude-opus-5\";",
            "a provider-qualified model id",
        ),
        (
            "let model = \"models/gemini-3-pro\";",
            "a provider-qualified model id",
        ),
        (
            "let model = \"open-mixer-8b-instruct\";",
            "an open-weight parameter count",
        ),
        (
            "let model = \"open-org/Mixer-3.1-8B-Instruct\";",
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
        "let id = \"0190e1b2-123b-7b5e-9f2a-3c4d5e6f7a8b\";",
        "let path = \"crates/meerkat-core/src\";",
        "let model = \"models/test-model\";",
    ] {
        assert_eq!(first_violation(text), None, "{text} is not a model name");
    }
}

/// (b) No file under crates/meerkat-core/src or crates/meerkat-core/tests may
/// contain a provider model-name literal (the shapes of [`model_name_shape`])
/// outside [`ALLOWED`]. This is a tombstone-class textual ban: the presence
/// of such a literal anywhere in core IS the violation, regardless of
/// context - real model names are provider data and belong in meerkat-models
/// (tests included; core tests use the synthetic `model_profile::test_catalog`
/// fixture).
#[test]
fn core_sources_contain_no_provider_model_literals() {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in ["crates/meerkat-core/src", "crates/meerkat-core/tests"] {
        let before = files.len();
        collect_files(&root.join(dir), &mut files);
        assert!(files.len() > before, "{dir} must not be empty");
    }

    let mut violations_found = Vec::new();
    let mut allowed_used = vec![false; ALLOWED.len()];
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue; // non-UTF-8 assets cannot carry the literal pattern we ban
        };
        let relative = path
            .strip_prefix(&root)
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let mut remaining = Vec::new();
        for (line, literal, shape) in violations(&text) {
            match ALLOWED
                .iter()
                .position(|(file, allowed, _)| *file == relative && *allowed == literal)
            {
                Some(index) => allowed_used[index] = true,
                None => remaining.push(format!("line {line}: \"{literal}\" ({shape})")),
            }
        }
        if let Some(found) = remaining.first() {
            violations_found.push(format!(
                "{} ({found}{})",
                path.display(),
                if remaining.len() > 1 {
                    format!(", and {} more", remaining.len() - 1)
                } else {
                    String::new()
                }
            ));
        }
    }
    let stale: Vec<String> = ALLOWED
        .iter()
        .zip(&allowed_used)
        .filter(|(_, used)| !**used)
        .map(|((file, literal, reason), _)| format!("{file}: \"{literal}\" ({reason})"))
        .collect();
    assert!(
        stale.is_empty(),
        "ALLOWED entries that match no literal any more; remove them:\n  {}",
        stale.join("\n  ")
    );
    let violations = violations_found;

    assert!(
        violations.is_empty(),
        "provider model-name literals found under crates/meerkat-core/src and tests:\n  {}\n\
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
