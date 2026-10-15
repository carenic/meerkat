//! The `rkat` startup report of OS process confinement, on stderr: each
//! launch kind's requested and effective confinement and what keeps the
//! host's own inputs from its confined processes, besides the warnings for
//! what the operator should act on.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

fn rkat_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_rkat") {
        return path.into();
    }
    let mut path = std::env::current_exe().expect("current test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push(if cfg!(windows) { "rkat.exe" } else { "rkat" });
    path
}

#[test]
fn rkat_reports_its_confinement_on_stderr() {
    let temp = tempfile::TempDir::new().unwrap();
    let project = temp.path().join("project");
    let state = temp.path().join("state");
    let user = temp.path().join("user");
    for dir in [&project, &state, &user] {
        std::fs::create_dir_all(dir).unwrap();
    }

    let output = Command::new(rkat_binary())
        .current_dir(&project)
        .env("HOME", temp.path())
        .env_remove("RUST_LOG")
        .arg("--context-root")
        .arg(&project)
        .arg("--state-root")
        .arg(&state)
        .arg("--user-config-root")
        .arg(&user)
        .args([
            "--realm",
            "confinement-report",
            "--confinement",
            "workspace",
            "session",
            "list",
        ])
        .output()
        .expect("run rkat");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "rkat session list under workspace confinement: status={} stderr:\n{stderr}",
        output.status
    );
    for kind in ["shell", "command_hooks", "mcp_stdio"] {
        assert!(
            stderr.contains(&format!("{kind} confinement: requested workspace")),
            "{kind}'s requested and effective confinement is reported; stderr:\n{stderr}"
        );
    }
    assert!(
        stderr.contains("kept from every confined process: the realm state at"),
        "what keeps the host's own inputs is reported, not only the warnings; stderr:\n{stderr}"
    );
    assert_eq!(
        stderr.matches("kept from every confined process").count(),
        1,
        "the report is printed once; stderr:\n{stderr}"
    );
}
