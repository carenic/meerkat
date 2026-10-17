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
    walk_files(root, |name| name.ends_with(".rs"))
}

/// Every directory under `root`, `root` included, skipping build output,
/// VCS metadata and Cargo's own directories.
fn walk_directories(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if entry.file_type().is_ok_and(|file_type| file_type.is_dir())
                    && !matches!(
                        name.as_ref(),
                        "target" | ".git" | "node_modules" | ".cargo" | "dist"
                    )
                    && !name.starts_with("bazel-")
                {
                    pending.push(entry.path());
                }
            }
        }
        out.push(dir);
    }
    out
}

/// Every file under `root` whose name `keep` accepts, skipping build output
/// and VCS metadata.
fn walk_files(root: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    walk_directories(root)
        .into_iter()
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten())
        .filter(|entry| {
            entry.file_type().is_ok_and(|file_type| !file_type.is_dir())
                && keep(&entry.file_name().to_string_lossy())
        })
        .map(|entry| entry.path())
        .collect()
}

/// Every Cargo configuration file in the tree: a `.cargo/config.toml`, or
/// the older `.cargo/config`, in any directory the walk reaches. Cargo reads
/// the one of each directory from where it runs up to the filesystem root,
/// so a crate's own applies to a build run in that crate.
fn cargo_config_files(root: &Path) -> Vec<PathBuf> {
    walk_directories(root)
        .into_iter()
        .flat_map(|dir| {
            let cargo = dir.join(".cargo");
            [cargo.join("config.toml"), cargo.join("config")]
        })
        .filter(|path| path.is_file())
        .collect()
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

/// The lint names an `allow` or `expect` attribute in `code` lists that
/// silence the launch lint with everything else: `warnings`, and the
/// `clippy::all` and `clippy::style` groups `disallowed_methods` and
/// `disallowed_types` belong to.
fn wholesale_lint_allows(code: &str) -> Vec<String> {
    let mut found = Vec::new();
    for marker in ["allow(", "expect("] {
        let mut rest = code;
        while let Some(at) = rest.find(marker) {
            let after = &rest[at + marker.len()..];
            let list = after.split(')').next().unwrap_or_default();
            found.extend(
                list.split(',')
                    .map(str::trim)
                    .filter(|lint| matches!(*lint, "warnings" | "clippy::all" | "clippy::style"))
                    .map(str::to_owned),
            );
            rest = after;
        }
    }
    found
}

/// The lints whose allow silences the launch lint: the lints themselves,
/// the `clippy::style` and `clippy::all` groups they belong to, and
/// `warnings`.
const LAUNCH_LINT_SILENCERS: [&str; 5] = [
    "clippy::disallowed_methods",
    "clippy::disallowed_types",
    "clippy::style",
    "clippy::all",
    "warnings",
];

/// The flags of a rustc flag list that turn the launch lint off: an allow
/// (`-A`, `--allow`, with the lint attached or as the next flag) of one of
/// [`LAUNCH_LINT_SILENCERS`], and any `--cap-lints`.
fn lint_off_flags(flags: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    let mut flags = flags.iter().map(String::as_str);
    while let Some(flag) = flags.next() {
        let allowed = match flag {
            "-A" | "--allow" => flags.next(),
            "--cap-lints" => {
                found.push(format!("--cap-lints {}", flags.next().unwrap_or_default()));
                continue;
            }
            _ if flag.starts_with("--cap-lints=") => {
                found.push(flag.to_owned());
                continue;
            }
            _ => flag
                .strip_prefix("--allow=")
                .or_else(|| flag.strip_prefix("-A")),
        };
        for lint in allowed.into_iter().flat_map(|allowed| allowed.split(',')) {
            if LAUNCH_LINT_SILENCERS.contains(&lint.trim().replace('-', "_").as_str()) {
                found.push(format!("allows {lint}"));
            }
        }
    }
    found
}

/// What a Cargo configuration file states that turns the launch lint off
/// for the builds it applies to: such a flag in a `rustflags` list (of
/// `[build]`, `[host]`, a `[target.<triple>]` or `[target.'cfg(..)']`
/// table, or a profile), and an `[env]` entry that points Clippy at another
/// configuration (`CLIPPY_CONF_DIR`) or passes it other arguments
/// (`CLIPPY_ARGS`).
fn cargo_config_lint_overrides(config: &toml::Table) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending: Vec<(String, &toml::Value)> = config
        .iter()
        .map(|(key, value)| (key.clone(), value))
        .collect();
    while let Some((key, value)) = pending.pop() {
        match value {
            toml::Value::Table(table) => pending.extend(
                table
                    .iter()
                    .map(|(name, value)| (format!("{key}.{name}"), value)),
            ),
            _ if key == "rustflags" || key.ends_with(".rustflags") => {
                let flags: Vec<String> = match value {
                    toml::Value::String(text) => {
                        text.split_whitespace().map(str::to_owned).collect()
                    }
                    toml::Value::Array(items) => items
                        .iter()
                        .filter_map(toml::Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                    _ => Vec::new(),
                };
                found.extend(
                    lint_off_flags(&flags)
                        .into_iter()
                        .map(|flag| format!("{key}: {flag}")),
                );
            }
            _ => {}
        }
    }
    if let Some(env) = config.get("env").and_then(toml::Value::as_table) {
        found.extend(
            ["CLIPPY_CONF_DIR", "CLIPPY_ARGS"]
                .into_iter()
                .filter(|name| env.contains_key(*name))
                .map(|name| format!("env.{name}")),
        );
    }
    found
}

/// The entries of a Cargo manifest's `[lints.clippy]` or
/// `[workspace.lints.clippy]` that would turn the launch lint off: any entry
/// for `disallowed_methods` or `disallowed_types`, and an `allow` of the
/// `all` or `style` group they belong to.
fn manifest_launch_lint_entries(manifest: &toml::Table) -> Vec<String> {
    let tables = [
        manifest.get("lints").and_then(|lints| lints.get("clippy")),
        manifest
            .get("workspace")
            .and_then(|workspace| workspace.get("lints"))
            .and_then(|lints| lints.get("clippy")),
    ];
    let mut found = Vec::new();
    for table in tables
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
    {
        for (lint, level) in table {
            let name = lint.replace('-', "_");
            let level = level
                .as_str()
                .or_else(|| level.get("level").and_then(toml::Value::as_str));
            if matches!(name.as_str(), "disallowed_methods" | "disallowed_types")
                || (matches!(name.as_str(), "all" | "style") && level == Some("allow"))
            {
                found.push(format!("{lint} = {level:?}"));
            }
        }
    }
    found
}

/// Shipped code allows the launch lint only at the sites `SITES` lists
/// as starting a process themselves (the confinement backend, a confined
/// kind's trusted-host branch, the exemptions' one constructor), once per
/// launch expression there; an exempt launch goes through its exemption's
/// fixed-program constructor and needs none. `SITES` stays the one list.
///
/// Nothing turns the lint off in bulk either: the workspace has one Clippy
/// configuration file (a crate's own `clippy.toml` or `.clippy.toml` would
/// replace the root one, and its disallowed list, for that crate), shipped
/// code allows neither `warnings` nor the `clippy::all` or `clippy::style`
/// group, no Cargo manifest configures the lint or allows its group, and no
/// Cargo configuration file in the tree passes rustc such an allow or a
/// `--cap-lints`, or sets `CLIPPY_CONF_DIR` (another configuration) or
/// `CLIPPY_ARGS` for the builds it applies to. What no file in the tree can
/// show is outside the check: those variables or `RUSTFLAGS` in a run's
/// environment, and Cargo configuration files above the checkout or in
/// Cargo's home. The repository's build lanes and CI set none that turns
/// the lint off.
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
    let mut bulk: Vec<String> = Vec::new();
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
            found.insert(relative.clone(), count);
        }
        let wholesale = wholesale_lint_allows(&code);
        if !wholesale.is_empty() {
            bulk.push(format!("{relative}: allows {wholesale:?}"));
        }
    }
    assert_eq!(
        found, expected,
        "shipped code allows the process launch lint exactly once per launch expression \
         of the sites SITES lists as launching themselves, and nowhere else"
    );

    let relative = |path: &Path| {
        path.strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let clippy_configs: Vec<String> =
        walk_files(&root, |name| matches!(name, "clippy.toml" | ".clippy.toml"))
            .iter()
            .map(|path| relative(path))
            .collect();
    assert_eq!(
        clippy_configs,
        ["clippy.toml"],
        "the workspace has one Clippy configuration, at its root"
    );
    for config in cargo_config_files(&root) {
        let text = fs::read_to_string(&config)
            .unwrap_or_else(|error| panic!("read {}: {error}", config.display()));
        let table: toml::Table = text
            .parse()
            .unwrap_or_else(|error| panic!("parse {}: {error}", config.display()));
        let overrides = cargo_config_lint_overrides(&table);
        if !overrides.is_empty() {
            bulk.push(format!("{}: {overrides:?}", relative(&config)));
        }
    }
    for manifest in walk_files(&root, |name| name == "Cargo.toml") {
        let text = fs::read_to_string(&manifest)
            .unwrap_or_else(|error| panic!("read {}: {error}", manifest.display()));
        let table: toml::Table = text
            .parse()
            .unwrap_or_else(|error| panic!("parse {}: {error}", manifest.display()));
        let entries = manifest_launch_lint_entries(&table);
        if !entries.is_empty() {
            bulk.push(format!("{}: {entries:?}", relative(&manifest)));
        }
    }
    assert!(
        bulk.is_empty(),
        "nothing turns the process launch lint off in bulk:\n{}",
        bulk.join("\n")
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

/// The bulk checks see what they look for: a `warnings` or group allow in
/// an `allow` or `expect` list, a manifest entry for the lint or an allow of
/// its group, and a Cargo configuration's rustc allow, `--cap-lints` or
/// Clippy variable, but not a lint, level or flag that leaves it on.
#[test]
fn the_bulk_lint_checks_see_what_turns_the_lint_off() {
    assert_eq!(
        wholesale_lint_allows(
            "#![allow(clippy::all)] #[allow(dead_code, warnings)] #[expect(clippy::style)] \
             #[allow(clippy::pedantic, clippy::allow_attributes)] fn allow(x: u8) {}"
        ),
        ["clippy::all", "warnings", "clippy::style"]
    );
    let manifest = |text: &str| -> Vec<String> {
        manifest_launch_lint_entries(&text.parse::<toml::Table>().unwrap())
    };
    assert_eq!(
        manifest(
            "[lints.clippy]\ndisallowed-methods = \"warn\"\nall = { level = \"allow\", priority = -1 }\n\
             pedantic = \"allow\"\n"
        )
        .len(),
        2
    );
    assert_eq!(
        manifest("[workspace.lints.clippy]\nstyle = \"allow\"\ndisallowed_types = \"allow\"\n")
            .len(),
        2
    );
    assert!(
        manifest(
            "[workspace.lints.clippy]\nall = { level = \"warn\", priority = -1 }\npanic = \"deny\"\n\
             [lints]\nworkspace = true\n"
        )
        .is_empty()
    );
    let cargo_config = |text: &str| -> Vec<String> {
        let mut found = cargo_config_lint_overrides(&text.parse::<toml::Table>().unwrap());
        found.sort();
        found
    };
    assert_eq!(
        cargo_config(
            "[build]\nrustflags = [\"-A\", \"clippy::disallowed_methods\", \"--cfg\", \"tokio_unstable\"]\n\
             [target.'cfg(unix)']\nrustflags = \"-Aclippy::style --allow=warnings -Dclippy::pedantic\"\n\
             [target.x86_64-unknown-linux-gnu]\n\
             rustflags = [\"--cap-lints\", \"warn\", \"--allow\", \"clippy::disallowed-types\"]\n\
             [profile.dev]\nrustflags = [\"-A\", \"clippy::all\"]\n\
             [env]\nCLIPPY_CONF_DIR = \"elsewhere\"\nRUST_MIN_STACK = \"8388608\"\n"
        ),
        [
            "build.rustflags: allows clippy::disallowed_methods",
            "env.CLIPPY_CONF_DIR",
            "profile.dev.rustflags: allows clippy::all",
            "target.cfg(unix).rustflags: allows clippy::style",
            "target.cfg(unix).rustflags: allows warnings",
            "target.x86_64-unknown-linux-gnu.rustflags: --cap-lints warn",
            "target.x86_64-unknown-linux-gnu.rustflags: allows clippy::disallowed-types",
        ]
    );
    assert!(
        cargo_config(
            "[build]\nrustflags = [\"-D\", \"warnings\", \"-A\", \"clippy::pedantic\", \"-Wclippy::style\"]\n\
             [env]\nRUST_MIN_STACK = \"8388608\"\n[alias]\nfast = [\"nextest\", \"run\"]\n"
        )
        .is_empty()
    );
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
