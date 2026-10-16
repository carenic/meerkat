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
    /// The command every exemption's fixed-program constructor builds (with
    /// its cleaned `PATH`); each constructor use is listed as its own
    /// exemption.
    ExemptionConstructor,
}

/// Each exemption's fixed-program constructor
/// (`HostLaunchExemption::<constructor>(`).
fn constructor(exemption: HostLaunchExemption) -> &'static str {
    match exemption {
        HostLaunchExemption::GitSkillSource => "git",
        HostLaunchExemption::CredentialHelper => "credential_command",
        HostLaunchExemption::BrowserOpener => "browser_opener",
        HostLaunchExemption::TerminalControl => "terminal_control",
        HostLaunchExemption::DeveloperBuildTooling => "wasm_pack",
        other => panic!("{other:?} has no constructor listed here"),
    }
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
/// name their exemption's fixed-program constructor in code
/// (`HostLaunchExemption::<constructor>(`).
const SITES: &[(&str, &str, usize, Launch)] = &[
    (
        "crates/meerkat-auth-core/src/auth_store/command.rs",
        "HostLaunchExemption::credential_command(",
        1,
        Launch::Exempt(HostLaunchExemption::CredentialHelper),
    ),
    (
        "crates/meerkat-auth-core/src/mcp_oauth.rs",
        "HostLaunchExemption::browser_opener(",
        1,
        Launch::Exempt(HostLaunchExemption::BrowserOpener),
    ),
    (
        "crates/meerkat-cli/src/main.rs",
        "HostLaunchExemption::terminal_control(",
        2,
        Launch::Exempt(HostLaunchExemption::TerminalControl),
    ),
    (
        "crates/meerkat-cli/src/main.rs",
        "HostLaunchExemption::wasm_pack(",
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
        "HostLaunchExemption::git(",
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
    // Read at run time (cargo, nextest and the Bazel test rule set it), so an
    // archived test binary never carries the builder's path.
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR is set for test processes"),
    );
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
/// it has no body. A match arm, field, variant or argument (anything that
/// does not start with an item or statement keyword) also ends at its `,`,
/// and any item ends where the construct enclosing it closes, so the code
/// after it is scanned.
fn item_end(bytes: &[u8], start: usize) -> usize {
    let separated = !starts_with_item_keyword(bytes, start);
    let mut index = start;
    let mut depth = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'{' | b'(' | b'[' => depth += 1,
            closing @ (b'}' | b')' | b']') => {
                if depth == 0 {
                    return index;
                }
                depth -= 1;
                if depth == 0 && closing == b'}' {
                    return index + 1;
                }
            }
            b';' if depth == 0 => return index + 1,
            b',' if depth == 0 && separated => return index + 1,
            _ => {}
        }
        index += 1;
    }
    bytes.len()
}

/// Whether the code at `start`, past whitespace and attributes, starts with
/// an item or statement keyword (whose `,` at the top level, as in generic
/// parameters, does not end it).
fn starts_with_item_keyword(bytes: &[u8], start: usize) -> bool {
    const KEYWORDS: &[&str] = &[
        "async",
        "const",
        "enum",
        "extern",
        "fn",
        "impl",
        "let",
        "macro_rules",
        "mod",
        "pub",
        "static",
        "struct",
        "trait",
        "type",
        "union",
        "unsafe",
        "use",
    ];
    let mut index = start;
    loop {
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) != Some(&b'#') {
            break;
        }
        let mut depth = 0usize;
        while let Some(&byte) = bytes.get(index) {
            index += 1;
            match byte {
                b'[' => depth += 1,
                b']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    let word_end = bytes[index.min(bytes.len())..]
        .iter()
        .position(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))
        .map_or(bytes.len(), |offset| index + offset);
    let word = std::str::from_utf8(&bytes[index.min(bytes.len())..word_end]).unwrap_or("");
    KEYWORDS.contains(&word)
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
/// and each exemption's `HostLaunchExemption::<constructor>(`.
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
        let expression = format!("HostLaunchExemption::{}(", constructor(exemption));
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

/// The paths `clippy.toml` disallows (`disallowed-methods`), in file order.
fn clippy_disallowed_methods(root: &Path) -> Vec<String> {
    let path = root.join("clippy.toml");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let config: toml::Table = text
        .parse()
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
    config
        .get("disallowed-methods")
        .and_then(toml::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| match entry {
                    toml::Value::String(path) => Some(path.clone()),
                    toml::Value::Table(entry) => entry
                        .get("path")
                        .and_then(toml::Value::as_str)
                        .map(str::to_owned),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Clippy refuses every API that starts a process (`disallowed_methods` in
/// `clippy.toml`, which resolves aliased imports and calls through
/// re-exports), so a launch the text scan above cannot see still fails the
/// lint: each expression the scan looks for is one of them.
#[test]
fn clippy_disallows_every_process_launch_api() {
    let disallowed = clippy_disallowed_methods(&repo_root());
    for required in [
        "std::process::Command::new",
        "tokio::process::Command::new",
        "std::os::unix::process::CommandExt::exec",
        "libc::popen",
        "nix::unistd::execve",
    ] {
        assert!(
            disallowed.iter().any(|path| path == required),
            "clippy.toml disallows {required}: {disallowed:#?}"
        );
    }
    for pattern in LAUNCH_PATTERNS {
        let expression = pattern.trim_end_matches('(');
        assert!(
            disallowed
                .iter()
                .any(|path| path == expression || path.ends_with(&format!("::{expression}"))),
            "clippy.toml disallows the launch the scan finds as {pattern}: {disallowed:#?}"
        );
    }
}

/// Shipped code allows the launch lint only at the sites `SITES` lists
/// as starting a process themselves (the confinement backend, a confined
/// kind's trusted-host branch, the exemptions' one constructor), once per
/// launch expression there; an exempt launch goes through its exemption's
/// fixed-program constructor and needs none. `SITES` stays the one list.
#[test]
fn the_launch_lint_is_allowed_only_at_the_listed_launch_sites() {
    let root = repo_root();
    let mut expected: BTreeMap<String, usize> = BTreeMap::new();
    for (file, _, count, launch) in SITES {
        if !matches!(launch, Launch::Exempt(_)) {
            *expected.entry((*file).to_string()).or_default() += count;
        }
    }
    let mut found: BTreeMap<String, usize> = BTreeMap::new();
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
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let code = shipped_code(&source);
        let count = code.matches("clippy::disallowed_methods").count()
            + code.matches("clippy::disallowed_types").count();
        if count > 0 {
            found.insert(relative, count);
        }
    }
    assert_eq!(
        found, expected,
        "shipped code allows the process launch lint exactly once per launch expression \
         of the sites SITES lists as launching themselves, and nowhere else"
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
                format!("HostLaunchExemption::{}(", constructor(*exemption)),
                "{file}: the site names its own exemption's constructor"
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
fn not_test() { let _ = HostLaunchExemption::browser_opener("https://example.invalid", None); }
#[tokio::test]
async fn async_test() { let _ = std::process::Command::new("async test"); }
fn lifetime<'a>(value: &'a str) -> &'a str { value }
"##;
    let found = launches(&shipped_code(source));
    assert_eq!(found.get("Command::new("), Some(&1), "{found:?}");
    assert_eq!(found.get("webbrowser::open"), None, "{found:?}");
    assert_eq!(
        found.get("HostLaunchExemption::browser_opener("),
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

/// A test-only item ends where the item does: a test-only match arm, field
/// or argument ends at its comma, and the shipped code after it is scanned.
#[test]
fn the_scanner_keeps_code_after_a_test_only_arm_or_field() {
    let source = r#"
fn arms(x: u8) {
    match x {
        #[cfg(test)]
        0 => helper(),
        _ => {
            let _ = std::process::Command::new("after-a-test-only-arm");
        }
    }
}
struct Fields {
    #[cfg(test)]
    probe: u8,
    kept: u8,
}
fn after_fields() { let _ = std::process::Command::new("after-a-test-only-field"); }
"#;
    let found = launches(&shipped_code(source));
    assert_eq!(found.get("Command::new("), Some(&2), "{found:?}");
}
