//! OS process confinement on the rkat-rest surface: what the stock binary
//! reads at startup to resolve its `--confinement*` flags.
//!
//! The observables are the real binary's output. A server started without a
//! confinement flag that needs the working directory starts even when that
//! directory was removed.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Last-resort bound for a server that neither listens nor exits: well above
/// the observed startup latency under a fully parallel crate run.
const STARTUP_DEADLINE: Duration = Duration::from_secs(200);

/// Cargo and nextest advertise the crate's own binary to its integration
/// tests through the runtime environment.
fn rkat_rest_binary() -> Option<PathBuf> {
    let advertised = std::env::var_os("CARGO_BIN_EXE_rkat-rest").map(PathBuf::from)?;
    advertised.exists().then_some(advertised)
}

/// A free local port for the server's `[rest]` section.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

/// One isolated server scope under `temp`: realm `realm` under `temp/state`,
/// listening on a free local port (returned with the arguments), with
/// `temp/user` as the user config root.
fn scope_args(temp: &Path, realm: &str) -> (Vec<String>, u16) {
    let state_root = temp.join("state");
    let realm_doc = state_root.join(realm).join("config.toml");
    std::fs::create_dir_all(realm_doc.parent().expect("realm dir")).expect("realm dir");
    let port = free_port();
    std::fs::write(
        &realm_doc,
        format!("[rest]\nhost = \"127.0.0.1\"\nport = {port}\n"),
    )
    .expect("realm document");
    std::fs::create_dir_all(temp.join("user")).expect("user root");
    let args = [
        "--realm",
        realm,
        "--realm-backend",
        "memory",
        "--state-root",
        state_root.to_str().expect("utf-8 state root"),
        "--user-config-root",
        temp.join("user").to_str().expect("utf-8 user root"),
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect();
    (args, port)
}

struct Startup {
    /// Whether the server accepted a connection on its port.
    listening: bool,
    /// Everything it wrote on stdout and stderr until then.
    output: String,
}

/// Run `command` until the server accepts a connection on `port` or exits,
/// then stop it. The default log filter is measured: `RUST_LOG` is removed.
fn start(mut command: Command, port: u16) -> Startup {
    let mut child = command
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rkat-rest should spawn");
    let (lines_tx, lines_rx) = mpsc::channel::<String>();
    let readers = [
        Box::new(child.stdout.take().expect("stdout pipe")) as Box<dyn std::io::Read + Send>,
        Box::new(child.stderr.take().expect("stderr pipe")),
    ]
    .map(|pipe| {
        let lines_tx = lines_tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines() {
                let Ok(line) = line else { break };
                if lines_tx.send(line).is_err() {
                    break;
                }
            }
        })
    });
    drop(lines_tx);

    let deadline = Instant::now() + STARTUP_DEADLINE;
    let mut output = String::new();
    let mut listening = false;
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    loop {
        while let Ok(line) = lines_rx.try_recv() {
            output.push_str(&line);
            output.push('\n');
        }
        if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            listening = true;
            break;
        }
        if child.try_wait().expect("try_wait should succeed").is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "rkat-rest neither listened nor exited within {STARTUP_DEADLINE:?}. output:\n{output}"
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    for reader in readers {
        reader.join().expect("output reader thread should finish");
    }
    while let Ok(line) = lines_rx.try_recv() {
        output.push_str(&line);
        output.push('\n');
    }
    Startup { listening, output }
}

/// A command that runs `binary` with `args` from a working directory that
/// was removed before the binary starts.
fn in_a_removed_directory(binary: &Path, removed: &Path, args: &[String]) -> Command {
    std::fs::create_dir_all(removed).expect("working directory");
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("cd \"$1\" && rmdir \"$1\" && shift && exec \"$@\"")
        .arg("sh")
        .arg(removed)
        .arg(binary)
        .args(args);
    command
}

/// Without a confinement flag that names a relative path the server never
/// reads its working directory, so it starts when that directory was
/// removed. A relative flag path still needs it, and its absence refuses the
/// start instead of guessing a base.
#[test]
fn rkat_rest_starts_without_a_working_directory_when_no_flag_needs_it() {
    let Some(binary) = rkat_rest_binary() else {
        eprintln!(
            "skipping: rkat-rest binary not advertised (CARGO_BIN_EXE_rkat-rest unset or missing)"
        );
        return;
    };
    let temp = TempDir::new().expect("temp dir");

    let (args, port) = scope_args(temp.path(), "rest-removed-cwd");
    let started = start(
        in_a_removed_directory(&binary, &temp.path().join("removed"), &args),
        port,
    );
    assert!(
        started.listening,
        "a server without confinement flags starts in a removed working directory; output:\n{}",
        started.output
    );

    // An existing context root, so only the working directory can refuse it.
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).expect("project root");
    let (mut relative, relative_port) = scope_args(temp.path(), "rest-removed-cwd-relative");
    relative.extend(
        [
            "--context-root",
            project.to_str().expect("utf-8 project root"),
            "--confinement",
            "workspace",
            "--confinement-write",
            "build",
        ]
        .into_iter()
        .map(ToOwned::to_owned),
    );
    let refused = start(
        in_a_removed_directory(&binary, &temp.path().join("removed-again"), &relative),
        relative_port,
    );
    assert!(
        !refused.listening,
        "a relative confinement path needs the working directory; output:\n{}",
        refused.output
    );
}
