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

/// `rkat skill list` loads skills as a run does, under the host's
/// confinement: a project skills directory a confined process can write is
/// not trusted, so a skill planted there is not listed.
#[test]
fn rkat_skill_list_does_not_load_skills_from_an_untrusted_project() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let project = base.join("project");
    let state = base.join("state");
    let user = base.join("user");
    let home = base.join("home");
    let skill = project.join(".rkat").join("skills").join("planted-skill");
    for dir in [&skill, &state, &user.join(".rkat"), &home] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: planted-skill\ndescription: A skill a confined process planted\n---\n\n# Planted",
    )
    .unwrap();
    std::fs::write(
        user.join(".rkat").join("config.toml"),
        format!(
            "[confinement.all]\nmode = \"custom\"\nworking_directory = \"{dir}\"\nrequirement = {{ baseline = \"command_runtime_v1\", read = {{ kind = \"paths\", paths = [{{ kind = \"subtree\", path = \"{dir}\" }}] }}, write = {{ kind = \"paths\", paths = [{{ kind = \"subtree\", path = \"{dir}\" }}] }}, deny_read = [], deny_write = [], network = {{ kind = \"denied\" }}, unix_connect = [], require_descendant_termination = false }}\n",
            dir = project.display()
        ),
    )
    .unwrap();

    let output = Command::new(rkat_binary())
        .current_dir(&project)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env_remove("RUST_LOG")
        .arg("--context-root")
        .arg(&project)
        .arg("--state-root")
        .arg(&state)
        .arg("--user-config-root")
        .arg(&user)
        .args(["--realm", "skills-untrusted", "skill", "list", "--json"])
        .output()
        .expect("run rkat");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "rkat skill list: status={} stderr:\n{stderr}",
        output.status
    );
    assert!(
        !stdout.contains("planted-skill"),
        "an untrusted project skills directory is not loaded; stdout:\n{stdout}"
    );
}
