//! K23 docs-CLI-examples gate, repository half.
//!
//! The packaged `rkat` binary test `fenced_rkat_doc_examples_parse_against_clap`
//! (`src/main.rs`) parses every invocation in `tests/fixtures/doc_examples.txt`
//! against the real clap definition, so it also runs in the published package,
//! which carries no `docs/`. This test owns the only extractor: it collects
//! every ```-fenced `rkat` invocation in `docs/**/*.mdx` and fails unless the
//! fixture holds exactly those lines, byte for byte. After adding or changing
//! an `rkat` example in `docs/`, refresh the fixture with the command in
//! [`REFRESH`].

#![allow(clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

// The Bazel generator gives this target the whole checkout and WORKSPACE_ROOT.
const LIVE_WORKSPACE_RUNFILES: &str = "required";

const FIXTURE: &str = "crates/meerkat-cli/tests/fixtures/doc_examples.txt";
const REFRESH: &str =
    "UPDATE_DOC_EXAMPLES_FIXTURE=1 cargo test -p rkat --test doc_examples_fixture";

fn workspace_root() -> PathBuf {
    assert_eq!(LIVE_WORKSPACE_RUNFILES, "required");
    std::env::var_os("WORKSPACE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("workspace root")
                .to_path_buf()
        })
}

/// Doc lines that are illustrative shapes rather than literal invocations:
/// placeholders, shell composition, or output elision.
fn skippable(line: &str) -> bool {
    line.contains('<')
        || line.contains('[')
        || line.contains("...")
        || line.contains('|')
        || line.contains('>')
        || line.contains("$(")
        || line.contains('`')
        || line.contains('$')
        || line.contains('#')
}

/// The lines inside ```-fences, a trailing `\` joining the next line.
fn fenced_command_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_fence = false;
    let mut pending = String::new();
    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            pending.clear();
            continue;
        }
        if !in_fence {
            continue;
        }
        let mut line = trimmed.to_string();
        if !pending.is_empty() {
            line = format!("{pending} {line}");
            pending.clear();
        }
        if let Some(stripped) = line.strip_suffix('\\') {
            pending = stripped.trim_end().to_string();
            continue;
        }
        lines.push(line);
    }
    lines
}

/// Quotes open and close the way the binary test's splitter reads them; an
/// unbalanced line is not a complete invocation.
fn quotes_balance(line: &str) -> bool {
    let (mut in_single, mut in_double) = (false, false);
    for ch in line.chars() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ => {}
        }
    }
    !in_single && !in_double
}

fn mdx_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|err| panic!("read dir {}: {err}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|err| panic!("read dir {}: {err}", dir.display()))
            .path();
        if path.is_dir() {
            mdx_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("mdx") {
            files.push(path);
        }
    }
}

/// The fixture as the docs define it: a header, then one
/// `<docs path>\t<invocation>` line per example, in path order.
fn expected_fixture(docs: &Path) -> String {
    let mut files = Vec::new();
    mdx_files(docs, &mut files);
    files.sort();
    let mut out = format!(
        "# Generated from docs/**/*.mdx; do not edit by hand. Refresh: {REFRESH}\n\
         # Adding or changing an rkat example in docs/ means refreshing this file, as with schemas and BUILD files.\n\
         # Each line is <docs path><TAB><invocation>; src/main.rs parses every invocation against the CLI.\n"
    );
    for path in files {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        let relative = Path::new("docs")
            .join(path.strip_prefix(docs).expect("docs file under docs/"))
            .to_string_lossy()
            .replace('\\', "/");
        for line in fenced_command_lines(&text) {
            let line = line.strip_prefix("$ ").unwrap_or(&line);
            if !(line.starts_with("rkat ") || line == "rkat") {
                continue;
            }
            if skippable(line) || !quotes_balance(line) {
                continue;
            }
            out.push_str(&relative);
            out.push('\t');
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[test]
fn doc_examples_fixture_matches_the_docs() {
    let root = workspace_root();
    let expected = expected_fixture(&root.join("docs"));
    let fixture = root.join(FIXTURE);
    if std::env::var("UPDATE_DOC_EXAMPLES_FIXTURE").as_deref() == Ok("1") {
        std::fs::write(&fixture, &expected)
            .unwrap_or_else(|err| panic!("write {}: {err}", fixture.display()));
        return;
    }
    let actual = std::fs::read_to_string(&fixture)
        .unwrap_or_else(|err| panic!("read {}: {err}", fixture.display()));
    if actual == expected {
        return;
    }
    let actual_lines: std::collections::BTreeSet<&str> = actual.lines().collect();
    let expected_lines: std::collections::BTreeSet<&str> = expected.lines().collect();
    let missing: Vec<&str> = expected_lines.difference(&actual_lines).copied().collect();
    let stale: Vec<&str> = actual_lines.difference(&expected_lines).copied().collect();
    panic!(
        "{FIXTURE} does not match the rkat examples in docs/; refresh it with:\n  {REFRESH}\n\
         in the docs, not the fixture ({}):\n  {}\n\
         in the fixture, not the docs ({}):\n  {}",
        missing.len(),
        missing.join("\n  "),
        stale.len(),
        stale.join("\n  "),
    );
}
