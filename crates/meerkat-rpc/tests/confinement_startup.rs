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
const INSTALLED_REPORT: &str = "OS process confinement installed";
const UNENFORCEABLE_REPORT: &str = "confinement cannot be enforced on this host";

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
    let mut child = Command::new(binary)
        .current_dir(cwd)
        .args(args)
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
    let status = loop {
        while let Ok(line) = stdout_rx.try_recv() {
            if line.contains("\"id\":1") {
                answered = true;
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
    Run {
        answered,
        status,
        stderr,
    }
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
