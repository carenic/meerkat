//! `rkat help` answers from the embedded skills and reads their references
//! through the help session's resource tools. A build without the `skills`
//! feature has neither, so it must refuse model help before it composes a
//! session or any tool, instead of running the question as an ordinary run.

#![cfg(not(feature = "skills"))]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Stdio};

fn files_under(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path.display().to_string());
            }
        }
    }
    found
}

#[test]
fn model_help_is_refused_before_any_session_without_the_skills_feature() {
    let scratch = tempfile::tempdir().expect("scratch dir");
    let home = scratch.path().join("home");
    let state_root = scratch.path().join("state");
    let context_root = scratch.path().join("context");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&context_root).expect("context root");

    let rkat = std::env::var_os("CARGO_BIN_EXE_rkat")
        .expect("Cargo or Nextest must provide the runtime rkat binary path");
    let out = Command::new(&rkat)
        .arg("--state-root")
        .arg(&state_root)
        .arg("--context-root")
        .arg(&context_root)
        .args(["help", "How do I add an MCP server?"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .current_dir(&context_root)
        .stdin(Stdio::null())
        .output()
        .expect("rkat help must spawn");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "model help must fail without the skills feature; stderr={stderr}"
    );
    assert!(
        stderr.contains("rkat help needs the `skills` feature"),
        "the refusal must name the missing feature; stderr={stderr}"
    );
    let written = files_under(&state_root);
    assert!(
        written.is_empty(),
        "the refusal comes before any realm or session is written: {written:?}"
    );
}
