//! Every process launch site in shipped code either routes through the
//! host's OS process confinement or is one of the typed host launch
//! exemptions ([`meerkat_core::HostLaunchExemption`]). A launch site this
//! inventory does not classify fails the test until it is classified here,
//! and a classified site that is gone fails it too.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use meerkat_core::{ConfinementLaunchKind, HostLaunchExemption};

/// How a launch site starts its process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Launch {
    /// The OS confinement backend's own start of a confined process.
    ConfinementBackend,
    /// A confined launch kind's start as the trusted host: it runs only when
    /// the host's selection leaves the kind unconfined; a kind the host
    /// confines starts through the backend or is refused.
    TrustedHostBranch(ConfinementLaunchKind),
    /// A host launch exemption: the host runs the program as itself.
    Exempt(HostLaunchExemption),
    /// The command constructor of every exemption
    /// ([`HostLaunchExemption::command`]); each use is listed as its own
    /// exemption.
    ExemptionConstructor,
}

/// Expressions that start a process.
const LAUNCH_PATTERNS: &[&str] = &[
    "Command::new(",
    "webbrowser::open",
    "libc::fork(",
    "libc::vfork(",
    "libc::posix_spawn(",
    "libc::posix_spawnp(",
    "libc::execv",
    "libc::system(",
    "nix::unistd::fork(",
    "nix::unistd::execv",
];

/// Every launch site in shipped code: the file, the expression, how many
/// times the file has it, and how it starts its process. Exempt launches
/// that build a `Command` name their exemption in code
/// (`HostLaunchExemption::<exemption>.command(`).
const SITES: &[(&str, &str, usize, Launch)] = &[
    (
        "crates/meerkat-auth-core/src/auth_store/command.rs",
        "HostLaunchExemption::CredentialHelper.command(",
        1,
        Launch::Exempt(HostLaunchExemption::CredentialHelper),
    ),
    (
        "crates/meerkat-auth-core/src/mcp_oauth.rs",
        "HostLaunchExemption::BrowserOpener.command(",
        3,
        Launch::Exempt(HostLaunchExemption::BrowserOpener),
    ),
    (
        "crates/meerkat-cli/src/main.rs",
        "webbrowser::open",
        1,
        Launch::Exempt(HostLaunchExemption::BrowserOpener),
    ),
    (
        "crates/meerkat-cli/src/main.rs",
        "HostLaunchExemption::TerminalControl.command(",
        2,
        Launch::Exempt(HostLaunchExemption::TerminalControl),
    ),
    (
        "crates/meerkat-cli/src/main.rs",
        "HostLaunchExemption::DeveloperBuildTooling.command(",
        1,
        Launch::Exempt(HostLaunchExemption::DeveloperBuildTooling),
    ),
    (
        "crates/meerkat-core/src/confinement.rs",
        "Command::new(",
        1,
        Launch::ExemptionConstructor,
    ),
    (
        "crates/meerkat-skills/src/source/git.rs",
        "HostLaunchExemption::GitSkillSource.command(",
        1,
        Launch::Exempt(HostLaunchExemption::GitSkillSource),
    ),
    (
        "crates/meerkat-skills/src/source/protocol.rs",
        "Command::new(",
        1,
        Launch::TrustedHostBranch(ConfinementLaunchKind::McpStdio),
    ),
    (
        "crates/meerkat-mcp/src/connection.rs",
        "Command::new(",
        1,
        Launch::TrustedHostBranch(ConfinementLaunchKind::McpStdio),
    ),
    (
        "crates/meerkat-hooks/src/lib.rs",
        "Command::new(",
        1,
        Launch::TrustedHostBranch(ConfinementLaunchKind::CommandHooks),
    ),
    (
        "crates/meerkat-tools/src/builtin/shell/custody_spawn.rs",
        "Command::new(",
        1,
        Launch::TrustedHostBranch(ConfinementLaunchKind::Shell),
    ),
    (
        "crates/meerkat-tools/src/builtin/shell/custody/gate.rs",
        "Command::new(",
        1,
        Launch::TrustedHostBranch(ConfinementLaunchKind::Shell),
    ),
    (
        "crates/meerkat-sandbox/src/linux.rs",
        "Command::new(",
        2,
        Launch::ConfinementBackend,
    ),
    (
        "crates/meerkat-sandbox/src/linux.rs",
        "libc::fork(",
        1,
        Launch::ConfinementBackend,
    ),
    (
        "crates/meerkat-sandbox/src/native.rs",
        "libc::posix_spawn(",
        1,
        Launch::ConfinementBackend,
    ),
];

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR -> crates/meerkat; parent x2 = workspace root.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .expect("crates/meerkat has a parent")
        .parent()
        .expect("crates has a parent (workspace root)")
        .to_path_buf();
    if root.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        root
    }
}

/// Every `.rs` file under `root`, skipping build output and VCS metadata.
fn walk_rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if !matches!(
                    name.as_ref(),
                    "target" | ".git" | "node_modules" | ".cargo" | "dist"
                ) && !name.starts_with("bazel-")
                {
                    pending.push(path);
                }
            } else if name.ends_with(".rs") {
                out.push(path);
            }
        }
    }
    out
}

/// Whether a repository-relative source path is shipped library or binary
/// code: a crate's `src` tree, without the developer tooling crate, build
/// scripts, and test-only files.
fn is_shipped_source(relative: &str) -> bool {
    let Some(rest) = relative.strip_prefix("crates/") else {
        return false;
    };
    let Some((krate, in_crate)) = rest.split_once('/') else {
        return false;
    };
    if krate == "xtask" || !in_crate.starts_with("src/") {
        return false;
    }
    let file = in_crate.rsplit('/').next().unwrap_or(in_crate);
    let stem = file.strip_suffix(".rs").unwrap_or(file);
    !(stem == "tests"
        || stem.ends_with("_tests")
        || in_crate.split('/').any(|part| part == "tests"))
}

/// `source` with comments and literals blanked and every test-only item
/// (`#[cfg(test)]`, `#[cfg(all(test, ..))]`, `#[test]` and `#[*::test]`)
/// removed, so only code a shipped build compiles remains.
fn shipped_code(source: &str) -> String {
    let code = blank_comments_and_literals(source);
    let bytes = code.as_bytes();
    let mut out = String::with_capacity(code.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#'
            && let Some((attribute, end)) = attribute_at(&code, index)
        {
            if gates_test_only(attribute) {
                index = item_end(bytes, end);
                continue;
            }
            out.push_str(&code[index..end]);
            index = end;
            continue;
        }
        let character = code[index..].chars().next().unwrap();
        out.push(character);
        index += character.len_utf8();
    }
    out
}

/// The attribute starting at `start` (`#[...]` or `#![...]`) and the index
/// after it.
fn attribute_at(code: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = code.as_bytes();
    let mut index = start + 1;
    if bytes.get(index) == Some(&b'!') {
        index += 1;
    }
    if bytes.get(index) != Some(&b'[') {
        return None;
    }
    let mut depth = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&code[start..=index], index + 1));
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// Whether an attribute makes its item test-only.
fn gates_test_only(attribute: &str) -> bool {
    let compact: String = attribute.chars().filter(|c| !c.is_whitespace()).collect();
    compact == "#[test]"
        || (compact.starts_with("#[") && compact.ends_with("::test]"))
        || (compact.contains("::test(") && !compact.starts_with("#[cfg"))
        || compact == "#[cfg(test)]"
        || compact.starts_with("#[cfg(all(test,")
        || (compact.starts_with("#[cfg(all(") && compact.contains(",test,"))
        || (compact.starts_with("#[cfg(all(") && compact.contains(",test))"))
}

/// The index after the item that starts at or after `start`: past any
/// further attributes, through its matching closing brace, or its `;` when
/// it has no body.
fn item_end(bytes: &[u8], start: usize) -> usize {
    let mut index = start;
    let mut depth = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index + 1;
                }
            }
            b';' if depth == 0 => return index + 1,
            _ => {}
        }
        index += 1;
    }
    bytes.len()
}

/// `source` with every comment and string or character literal replaced by
/// spaces (newlines kept).
fn blank_comments_and_literals(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    let blank = |out: &mut String, character: char| {
        out.push(if character == '\n' { '\n' } else { ' ' });
    };
    while index < chars.len() {
        let character = chars[index];
        let next = chars.get(index + 1).copied();
        if character == '/' && next == Some('/') {
            while index < chars.len() && chars[index] != '\n' {
                blank(&mut out, chars[index]);
                index += 1;
            }
            continue;
        }
        if character == '/' && next == Some('*') {
            let mut depth = 0usize;
            while index < chars.len() {
                if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
                    depth += 1;
                    blank(&mut out, chars[index]);
                    blank(&mut out, '*');
                    index += 2;
                    continue;
                }
                if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                    depth -= 1;
                    blank(&mut out, '*');
                    blank(&mut out, '/');
                    index += 2;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                blank(&mut out, chars[index]);
                index += 1;
            }
            continue;
        }
        // Raw strings: r"..", r#".."#, br#".."#.
        let raw_start = match (character, next) {
            ('r', Some('"' | '#')) => Some(index + 1),
            ('b', Some('r')) if matches!(chars.get(index + 2), Some('"' | '#')) => Some(index + 2),
            _ => None,
        };
        if let Some(mut cursor) = raw_start
            && !identifier_continues(&chars, index)
        {
            let mut hashes = 0usize;
            while chars.get(cursor) == Some(&'#') {
                hashes += 1;
                cursor += 1;
            }
            if chars.get(cursor) == Some(&'"') {
                cursor += 1;
                while cursor < chars.len() {
                    if chars[cursor] == '"'
                        && (0..hashes).all(|offset| chars.get(cursor + 1 + offset) == Some(&'#'))
                    {
                        cursor += 1 + hashes;
                        break;
                    }
                    cursor += 1;
                }
                for &skipped in &chars[index..cursor.min(chars.len())] {
                    blank(&mut out, skipped);
                }
                index = cursor;
                continue;
            }
        }
        if character == '"' {
            blank(&mut out, character);
            index += 1;
            while index < chars.len() {
                let current = chars[index];
                blank(&mut out, current);
                index += 1;
                if current == '\\' {
                    if index < chars.len() {
                        blank(&mut out, chars[index]);
                        index += 1;
                    }
                } else if current == '"' {
                    break;
                }
            }
            continue;
        }
        if character == '\'' {
            // A character literal ('x', '\n', '\u{..}'); a lifetime has no
            // closing quote right after one character or escape.
            let end = if next == Some('\\') {
                chars
                    .get(index + 3..)
                    .and_then(|rest| rest.iter().position(|&c| c == '\''))
                    .map(|offset| index + 3 + offset)
            } else if chars.get(index + 2) == Some(&'\'') {
                Some(index + 2)
            } else {
                None
            };
            if let Some(end) = end {
                for &skipped in &chars[index..=end] {
                    blank(&mut out, skipped);
                }
                index = end + 1;
                continue;
            }
        }
        out.push(character);
        index += 1;
    }
    out
}

/// Whether the character before `index` continues an identifier, so an `r`
/// or `b` there is not a literal prefix.
fn identifier_continues(chars: &[char], index: usize) -> bool {
    index > 0 && (chars[index - 1].is_alphanumeric() || chars[index - 1] == '_')
}

/// Every launch expression in `code` with its count: each launch pattern,
/// and each `HostLaunchExemption::<exemption>.command(`.
fn launches(code: &str) -> BTreeMap<String, usize> {
    let mut found = BTreeMap::new();
    for pattern in LAUNCH_PATTERNS {
        let count = code.matches(pattern).count();
        if count > 0 {
            found.insert((*pattern).to_string(), count);
        }
    }
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    for exemption in HostLaunchExemption::ALL {
        let expression = format!("HostLaunchExemption::{exemption:?}.command(");
        let count = compact.matches(&expression).count();
        if count > 0 {
            found.insert(expression, count);
        }
    }
    found
}

#[test]
fn every_process_launch_in_shipped_code_is_confined_or_a_typed_exemption() {
    let root = repo_root();
    let mut found: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut scanned = 0usize;
    for path in walk_rust_sources(&root) {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let relative = relative.trim_start_matches("./").to_string();
        if !is_shipped_source(&relative) {
            continue;
        }
        scanned += 1;
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        for (expression, count) in launches(&shipped_code(&source)) {
            found.insert((relative.clone(), expression), count);
        }
    }
    assert!(
        scanned > 100,
        "the workspace sources were not found under {} ({scanned} files)",
        root.display()
    );

    let expected: BTreeMap<(String, String), usize> = SITES
        .iter()
        .map(|(file, expression, count, _)| ((file.to_string(), expression.to_string()), *count))
        .collect();
    let unlisted: Vec<_> = found
        .iter()
        .filter(|(site, count)| expected.get(*site) != Some(count))
        .collect();
    let gone: Vec<_> = expected
        .iter()
        .filter(|(site, count)| found.get(*site) != Some(count))
        .collect();
    assert!(
        unlisted.is_empty() && gone.is_empty(),
        "every process launch in shipped code routes through the host's confinement or is a \
         typed host launch exemption, and SITES lists each one.\n\
         found but not listed (or a different count): {unlisted:#?}\n\
         listed but not found: {gone:#?}"
    );
}

#[test]
fn every_host_launch_exemption_is_in_use_and_named_where_it_launches() {
    for exemption in HostLaunchExemption::ALL {
        assert!(
            SITES
                .iter()
                .any(|(_, _, _, launch)| *launch == Launch::Exempt(exemption)),
            "{exemption:?} has no launch site"
        );
        assert!(
            !exemption.reason().is_empty(),
            "{exemption:?} states its reason"
        );
    }
    for (file, expression, _, launch) in SITES {
        if let Launch::Exempt(exemption) = launch
            && expression.starts_with("HostLaunchExemption::")
        {
            assert_eq!(
                *expression,
                format!("HostLaunchExemption::{exemption:?}.command("),
                "{file}: the site names its own exemption"
            );
        }
    }
}

#[test]
fn the_scanner_sees_code_and_skips_comments_literals_and_tests() {
    let source = r##"
// Command::new("in a comment")
/* Command::new("in a block") */
fn shipped() { let _ = std::process::Command::new("shipped"); }
fn literal() { let _ = "Command::new(" ; let _ = r#"webbrowser::open"#; let _ = 'x'; }
#[cfg(test)]
mod tests { fn t() { let _ = std::process::Command::new("test"); } }
#[cfg(all(unix, test))]
fn gated() { let _ = std::process::Command::new("gated"); }
#[cfg(not(test))]
fn not_test() { let _ = HostLaunchExemption::BrowserOpener.command("open"); }
#[tokio::test]
async fn async_test() { let _ = std::process::Command::new("async test"); }
fn lifetime<'a>(value: &'a str) -> &'a str { value }
"##;
    let found = launches(&shipped_code(source));
    assert_eq!(found.get("Command::new("), Some(&1), "{found:?}");
    assert_eq!(found.get("webbrowser::open"), None, "{found:?}");
    assert_eq!(
        found.get("HostLaunchExemption::BrowserOpener.command("),
        Some(&1),
        "{found:?}"
    );
    assert!(is_shipped_source("crates/meerkat-cli/src/main.rs"));
    assert!(!is_shipped_source("crates/meerkat-mcp/src/egress_tests.rs"));
    assert!(!is_shipped_source(
        "crates/meerkat/tests/host_confinement.rs"
    ));
    assert!(!is_shipped_source("crates/xtask/src/main.rs"));
    assert!(!is_shipped_source("crates/meerkat/build.rs"));
    assert!(!is_shipped_source("tests/integration/src/harness_only.rs"));
}
