//! The authority-language tombstone gate of `xtask machine-check-drift`.
//!
//! It reads only documentation prose, so it lives outside the
//! machine-authority feature: the live-docs test that runs it builds with
//! xtask's default features, as CI's reader rows for a docs change do.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Deliberately text-level tombstone gate: these are retired
/// authority-language names banned from *documentation prose*
/// (`docs/` + spec READMEs, see `authority_language_paths`), where the
/// stale word itself is the violation regardless of context. This is not a
/// Rust-source structure check; structural source-shape governance lives in
/// the syn-AST gates in `machines` and `rmat_audit`/`effect_authority`.
pub fn collect_authority_language_mismatches(root: &Path) -> Result<Vec<String>> {
    let mut mismatches = Vec::new();
    let banned = ["schema.yaml", "PureHandKernel", "PureHand"];

    for path in authority_language_paths(root)? {
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("read authority-language file {}", path.display()))?;

        for token in banned {
            if contents.contains(token) {
                mismatches.push(format!(
                    "stale authority language `{token}` present in {}",
                    path.display()
                ));
            }
        }
    }

    Ok(mismatches)
}

fn authority_language_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    collect_text_paths(&root.join("docs"), &mut paths)?;

    for path in [
        root.join("specs/machines/README.md"),
        root.join("specs/compositions/README.md"),
    ] {
        if path.exists() {
            paths.push(path);
        }
    }

    Ok(paths)
}

fn collect_text_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("iterate {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("read file type {}", path.display()))?;

        if file_type.is_dir() {
            collect_text_paths(&path, paths)?;
            continue;
        }

        let is_text = matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("md" | "mdx" | "yaml" | "yml" | "txt")
        );
        if is_text {
            paths.push(path);
        }
    }

    Ok(())
}
