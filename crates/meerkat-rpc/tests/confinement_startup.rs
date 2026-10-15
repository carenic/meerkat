//! OS process confinement on the rkat-rpc surface: the stock binary resolves
//! the composed `[confinement]` and its `--confinement*` flags at startup.
//!
//! The observables are the real binary's. The workspace preset without
//! `--context-root` refuses to start instead of confining nothing. A
//! requirement in the user document reaches the server's startup report even
//! without a `[realm.global]` section: a requirement no platform backend
//! enforces (descendant termination) is reported as unenforceable on every
//! OS, and the server still serves. The operator's `--confinement
//! trusted-host` replaces the documents, so nothing is reported.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const INITIALIZE_REQUEST: &str =
    "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n";

/// A requirement no platform backend enforces (descendant termination).
const UNENFORCEABLE_CONFINEMENT: &str = r#"[confinement.all]
mode = "custom"
working_directory = "/"
requirement = { baseline = "command_runtime_v1", read = { kind = "paths", paths = [] }, write = { kind = "paths", paths = [] }, deny_read = [], deny_write = [], network = { kind = "denied" }, unix_connect = [], require_descendant_termination = true }
"#;

/// The startup report of an installed requirement, and its warning for a
/// launch kind this host cannot confine.
const INSTALLED_REPORT: &str = "confinement: requested custom";
const UNENFORCEABLE_REPORT: &str = "cannot be enforced on this host";

/// Last-resort bound for a server that neither exits nor answers, as in
/// `realm_config_ingress.rs`: well above the observed startup latency under
/// a fully parallel crate run and below the CI unit profile's ceiling.
const EXIT_DEADLINE: Duration = Duration::from_secs(200);

/// Cargo and nextest advertise the crate's own binary to its integration
/// tests through the runtime environment.
fn rkat_rpc_binary() -> Option<PathBuf> {
    let advertised = std::env::var_os("CARGO_BIN_EXE_rkat-rpc").map(PathBuf::from)?;
    advertised.exists().then_some(advertised)
}

struct Run {
    /// Whether the server answered `initialize`.
    answered: bool,
    status: ExitStatus,
    stderr: String,
}

/// Run `rkat-rpc` with `args` in `cwd`, hand it one `initialize` request,
/// and close its stdin once it answers, so a booted server shuts down on EOF.
/// A server that refuses to start exits without answering.
fn run_rkat_rpc(binary: &Path, cwd: &Path, args: &[&str]) -> Run {
    run_rkat_rpc_with_requests(binary, cwd, args, &[]).0
}

/// [`run_rkat_rpc`], handing the server `requests` (ids 2, 3, ...) once it
/// answered `initialize` and closing its stdin once it answered every one.
/// Returns each answered request's response by id.
fn run_rkat_rpc_with_requests(
    binary: &Path,
    cwd: &Path,
    args: &[&str],
    requests: &[(&str, serde_json::Value)],
) -> (Run, BTreeMap<u64, serde_json::Value>) {
    let mut command = Command::new(binary);
    command.current_dir(cwd).args(args);
    run_rpc_command(command, requests)
}

/// Run the server `command` starts as [`run_rkat_rpc_with_requests`] does.
fn run_rpc_command(
    mut command: Command,
    requests: &[(&str, serde_json::Value)],
) -> (Run, BTreeMap<u64, serde_json::Value>) {
    let mut child = command
        .env_remove("RUST_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rkat-rpc should spawn");

    // A refusal may already have closed the pipe; that write failure is the
    // expected shape, not a test defect.
    let mut stdin = child.stdin.take();
    if let Some(pipe) = stdin.as_mut() {
        let _ = pipe.write_all(INITIALIZE_REQUEST.as_bytes());
        let _ = pipe.flush();
    }

    let mut stderr_pipe = child.stderr.take().expect("stderr pipe");
    let stderr_reader = std::thread::spawn(move || {
        let mut buffer = String::new();
        let _ = stderr_pipe.read_to_string(&mut buffer);
        buffer
    });
    let stdout_pipe = child.stdout.take().expect("stdout pipe");
    let (stdout_tx, stdout_rx) = mpsc::channel::<String>();
    let stdout_reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout_pipe).lines() {
            let Ok(line) = line else { break };
            if stdout_tx.send(line).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + EXIT_DEADLINE;
    let mut answered = false;
    let mut responses: BTreeMap<u64, serde_json::Value> = BTreeMap::new();
    let last_id = 1 + requests.len() as u64;
    let status = loop {
        while let Ok(line) = stdout_rx.try_recv() {
            let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            let Some(id) = message.get("id").and_then(serde_json::Value::as_u64) else {
                continue;
            };
            responses.insert(id, message);
            if id == 1 && !answered {
                answered = true;
                if let Some(pipe) = stdin.as_mut() {
                    for (offset, (method, params)) in requests.iter().enumerate() {
                        let request = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": 2 + offset as u64,
                            "method": method,
                            "params": params,
                        });
                        let _ = pipe.write_all(format!("{request}\n").as_bytes());
                    }
                    let _ = pipe.flush();
                }
            }
            if answered && (1..=last_id).all(|id| responses.contains_key(&id)) {
                // EOF on stdin shuts the booted server down.
                drop(stdin.take());
            }
        }
        if let Some(status) = child.try_wait().expect("try_wait should succeed") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let stderr = stderr_reader
                .join()
                .expect("stderr reader thread should finish");
            panic!(
                "rkat-rpc neither exited nor shut down after answering within \
                 {EXIT_DEADLINE:?} (answered: {answered}). stderr:\n{stderr}"
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stderr = stderr_reader
        .join()
        .expect("stderr reader thread should finish");
    stdout_reader
        .join()
        .expect("stdout reader thread should finish");
    (
        Run {
            answered,
            status,
            stderr,
        },
        responses,
    )
}

/// The scope arguments of one isolated run: a memory realm under `temp`,
/// with `temp/user` as the user config root.
fn scope_args<'a>(realm: &'a str, state_root: &'a Path, user_root: &'a Path) -> Vec<&'a str> {
    vec![
        "--realm",
        realm,
        "--realm-backend",
        "memory",
        "--state-root",
        state_root.to_str().expect("utf-8 state root"),
        "--user-config-root",
        user_root.to_str().expect("utf-8 user root"),
    ]
}

#[test]
fn workspace_confinement_without_a_context_root_refuses_rkat_rpc_startup() {
    let Some(binary) = rkat_rpc_binary() else {
        eprintln!(
            "skipping: rkat-rpc binary not advertised (CARGO_BIN_EXE_rkat-rpc unset or missing)"
        );
        return;
    };
    let temp = TempDir::new().expect("temp dir");
    let state_root = temp.path().join("state");
    let user_root = temp.path().join("user");
    std::fs::create_dir_all(&user_root).expect("user root should initialize");

    let mut args = scope_args("rpc-confinement-unrooted", &state_root, &user_root);
    args.extend(["--confinement", "workspace"]);
    let run = run_rkat_rpc(&binary, temp.path(), &args);

    assert!(
        !run.answered && !run.status.success(),
        "rkat-rpc must refuse workspace confinement without a workspace root instead of \
         serving unconfined; exit={:?} stderr:\n{}",
        run.status,
        run.stderr
    );
    assert!(
        run.stderr.contains("--context-root"),
        "the refusal names its remedy; stderr:\n{}",
        run.stderr
    );
}

/// A start whose confined processes could change the realm state is refused
/// before the server serves, naming the input and its remedy.
#[test]
fn confinement_that_could_change_the_realm_state_refuses_rkat_rpc_startup() {
    let Some(binary) = rkat_rpc_binary() else {
        eprintln!(
            "skipping: rkat-rpc binary not advertised (CARGO_BIN_EXE_rkat-rpc unset or missing)"
        );
        return;
    };
    let temp = TempDir::new().expect("temp dir");
    let base = temp.path().canonicalize().expect("canonical temp");
    let project = base.join("project");
    let state = base.join("state");
    let user_root = base.join("user");
    std::fs::create_dir_all(&project).expect("project root should initialize");
    std::fs::create_dir_all(&state).expect("state root should initialize");
    std::fs::create_dir_all(user_root.join(".rkat")).expect("user root should initialize");
    std::fs::write(
        user_root.join(".rkat").join("config.toml"),
        format!(
            "[confinement.all]\nmode = \"custom\"\nworking_directory = \"{state}\"\nrequirement = {{ baseline = \"command_runtime_v1\", read = {{ kind = \"paths\", paths = [{{ kind = \"subtree\", path = \"{state}\" }}] }}, write = {{ kind = \"paths\", paths = [{{ kind = \"subtree\", path = \"{state}\" }}] }}, deny_read = [], deny_write = [], network = {{ kind = \"denied\" }}, unix_connect = [], require_descendant_termination = false }}\n",
            state = state.display()
        ),
    )
    .expect("user document should be written");

    let mut args = scope_args("rpc-confinement-state-writer", &state, &user_root);
    args.extend([
        "--context-root",
        project.to_str().expect("utf-8 project root"),
    ]);
    let run = run_rkat_rpc(&binary, &project, &args);

    assert!(
        !run.answered && !run.status.success(),
        "rkat-rpc must refuse a start whose confined processes could change its state; \
         exit={:?} stderr:\n{}",
        run.status,
        run.stderr
    );
    assert!(
        run.stderr.contains("the realm state") && run.stderr.contains("--state-root"),
        "the refusal names the input and its remedy; stderr:\n{}",
        run.stderr
    );
}

#[test]
fn the_user_documents_confinement_reaches_the_rkat_rpc_startup_report() {
    let Some(binary) = rkat_rpc_binary() else {
        eprintln!(
            "skipping: rkat-rpc binary not advertised (CARGO_BIN_EXE_rkat-rpc unset or missing)"
        );
        return;
    };
    let temp = TempDir::new().expect("temp dir");
    let project = temp.path().join("project");
    let state_root = temp.path().join("state");
    let user_root = temp.path().join("user");
    std::fs::create_dir_all(&project).expect("project root should initialize");
    std::fs::create_dir_all(user_root.join(".rkat")).expect("user root should initialize");
    std::fs::write(
        user_root.join(".rkat").join("config.toml"),
        UNENFORCEABLE_CONFINEMENT,
    )
    .expect("user document should be written");
    let project_arg = project.to_str().expect("utf-8 project root");

    let mut args = scope_args("rpc-confinement-reported", &state_root, &user_root);
    args.extend(["--context-root", project_arg]);
    let configured = run_rkat_rpc(&binary, &project, &args);
    assert!(
        configured.answered,
        "a confined server still serves; exit={:?} stderr:\n{}",
        configured.status, configured.stderr
    );
    assert!(
        configured.stderr.contains(INSTALLED_REPORT)
            && configured.stderr.contains(UNENFORCEABLE_REPORT),
        "the user document's requirement reaches the startup report; stderr:\n{}",
        configured.stderr
    );

    let mut args = scope_args("rpc-confinement-flagged", &state_root, &user_root);
    args.extend([
        "--context-root",
        project_arg,
        "--confinement",
        "trusted-host",
    ]);
    let flagged = run_rkat_rpc(&binary, &project, &args);
    assert!(
        flagged.answered,
        "the trusted host serves; exit={:?} stderr:\n{}",
        flagged.status, flagged.stderr
    );
    assert!(
        !flagged.stderr.contains(INSTALLED_REPORT),
        "the operator's flag replaces the documents; stderr:\n{}",
        flagged.stderr
    );
}

/// A `config/patch` or `config/set` cannot cut the served realm from the
/// parent whose `[confinement]` it composes: the real server refuses the
/// write with the confinement discriminator and the realm document keeps its
/// bytes. A write that keeps the edge commits.
#[test]
fn rkat_rpc_config_writes_cannot_drop_inherited_confinement() {
    let Some(binary) = rkat_rpc_binary() else {
        eprintln!(
            "skipping: rkat-rpc binary not advertised (CARGO_BIN_EXE_rkat-rpc unset or missing)"
        );
        return;
    };
    let temp = TempDir::new().expect("temp dir");
    let state_root = temp.path().join("state");
    let user_root = temp.path().join("user");
    std::fs::create_dir_all(&user_root).expect("user root should initialize");
    let team_text = "[realm.team]\nparent = \"org\"\n";
    let org_text = format!("[realm.org]\n\n{UNENFORCEABLE_CONFINEMENT}");
    for (realm, text) in [("org", org_text.as_str()), ("team", team_text)] {
        let document = state_root.join(realm).join("config.toml");
        std::fs::create_dir_all(document.parent().expect("realm dir"))
            .expect("realm dir should initialize");
        std::fs::write(&document, text).expect("realm document should be written");
    }

    let args = scope_args("team", &state_root, &user_root);
    let (run, responses) = run_rkat_rpc_with_requests(
        &binary,
        temp.path(),
        &args,
        &[
            (
                "config/patch",
                serde_json::json!({ "patch": { "realm": { "team": { "parent": null } } } }),
            ),
            (
                "config/patch",
                serde_json::json!({ "patch": { "realm": { "team": null } } }),
            ),
            (
                "config/patch",
                serde_json::json!({ "patch": { "max_tokens": 2048 } }),
            ),
        ],
    );
    assert!(
        run.answered,
        "the team realm serves; exit={:?} stderr:\n{}",
        run.status, run.stderr
    );
    for id in [2, 3] {
        let error = &responses[&id]["error"];
        assert_eq!(
            error["data"],
            serde_json::json!({ "type": "confinement_administration_required" }),
            "request {id}: {}",
            responses[&id]
        );
    }
    assert!(
        responses[&4].get("error").is_none(),
        "a patch that keeps the edge commits: {}",
        responses[&4]
    );
    let committed = std::fs::read_to_string(state_root.join("team").join("config.toml"))
        .expect("team document");
    assert!(committed.contains("parent = \"org\""), "{committed}");
    assert!(committed.contains("max_tokens = 2048"), "{committed}");
}

/// Without a confinement flag that names a relative path the server never
/// reads its working directory, so it serves from one that was removed.
#[cfg(unix)]
#[test]
fn rkat_rpc_starts_without_a_working_directory_when_no_flag_needs_it() {
    let Some(binary) = rkat_rpc_binary() else {
        eprintln!(
            "skipping: rkat-rpc binary not advertised (CARGO_BIN_EXE_rkat-rpc unset or missing)"
        );
        return;
    };
    let temp = TempDir::new().expect("temp dir");
    let state_root = temp.path().join("state");
    let user_root = temp.path().join("user");
    let removed = temp.path().join("removed");
    for dir in [&user_root, &removed] {
        std::fs::create_dir_all(dir).expect("directory should initialize");
    }

    // The shell removes its working directory, then becomes the server.
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("cd \"$1\" && rmdir \"$1\" && shift && exec \"$@\"")
        .arg("sh")
        .arg(&removed)
        .arg(&binary)
        .args(scope_args("rpc-removed-cwd", &state_root, &user_root));
    let (run, _) = run_rpc_command(command, &[]);
    assert!(
        run.answered,
        "a server without confinement flags serves from a removed working directory; \
         exit={:?} stderr:\n{}",
        run.status, run.stderr
    );
}
