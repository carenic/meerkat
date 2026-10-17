//! Real-binary regression: the first OAuth login of an HTTP MCP server.
//!
//! `rkat mcp add` writes the server's OAuth account selection to the project
//! `mcp.toml`, and `rkat mcp login` reads it back from there. The server is a
//! local OAuth-protected MCP fixture in one of two shapes: an authorization
//! server that publishes OpenID Connect UserInfo, or one that publishes only
//! OAuth protected-resource and authorization-server metadata with a JWKS
//! (no account evidence). The test is the browser: no `xdg-open` is on
//! `PATH`, so the headless login prints the authorize URL and the test
//! follows it. Every root (`HOME`, `XDG_CONFIG_HOME`, state, context and
//! user config) is a temporary directory, and the file token store under
//! `XDG_CONFIG_HOME` is read directly.

#![cfg(all(feature = "mcp", target_os = "linux"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;

use meerkat::test_fixtures::mcp_oauth::{
    AccountEvidence, McpOAuthFixture, SUBJECT, follow_authorize_url,
};
use serde_json::Value;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

/// Bounds every `rkat` run, a login's wait for its callback included.
const COMMAND_DEADLINE: Duration = Duration::from_secs(90);
const SERVER: &str = "tracker";

fn rkat_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_rkat") {
        return path.into();
    }
    let mut path = std::env::current_exe().expect("current test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("rkat");
    path
}

/// The isolated roots of one regression.
struct Roots {
    _temp: TempDir,
    home: PathBuf,
    project: PathBuf,
    state_root: PathBuf,
    /// The only `PATH` entry. It is empty, so no browser opener is found.
    bin: PathBuf,
}

impl Roots {
    fn new() -> Self {
        let temp = TempDir::new().expect("temp dir");
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        let state_root = temp.path().join("state");
        let bin = temp.path().join("bin");
        for dir in [&home, &project, &state_root, &bin] {
            std::fs::create_dir_all(dir).expect("create root");
        }
        Self {
            _temp: temp,
            home,
            project,
            state_root,
            bin,
        }
    }

    fn rkat(&self, args: &[&str]) -> Command {
        let mut command = Command::new(rkat_binary());
        command
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("PATH", &self.bin)
            .current_dir(&self.project)
            .arg("--state-root")
            .arg(&self.state_root)
            .arg("--context-root")
            .arg(&self.project)
            .arg("--user-config-root")
            .arg(&self.home)
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        command
    }

    async fn run(&self, args: &[&str]) -> Output {
        tokio::time::timeout(COMMAND_DEADLINE, self.rkat(args).output())
            .await
            .unwrap_or_else(|_| panic!("rkat {args:?} did not finish"))
            .unwrap_or_else(|error| panic!("rkat {args:?} did not start: {error}"))
    }

    /// `rkat mcp add tracker --url <fixture> <selection...>`.
    async fn add(&self, fixture: &McpOAuthFixture, selection: &[&str]) -> Output {
        let url = fixture.mcp_url();
        let mut args = vec!["mcp", "add", SERVER, "--url", url.as_str()];
        args.extend_from_slice(selection);
        self.run(&args).await
    }

    /// `rkat mcp login tracker --allow-headless`, with the test as the
    /// browser: the authorize URL the login prints is followed, so the
    /// fixture redirects to the login's loopback callback.
    async fn login(&self) -> LoginRun {
        let mut child = self
            .rkat(&["mcp", "login", SERVER, "--allow-headless"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start rkat mcp login");
        let mut stdout = child.stdout.take().expect("piped stdout");
        let stdout = tokio::spawn(async move {
            let mut text = String::new();
            stdout.read_to_string(&mut text).await.map(|_| text)
        });
        let mut lines = BufReader::new(child.stderr.take().expect("piped stderr")).lines();
        let mut stderr = String::new();
        let mut authorize_url = None;
        let finished = tokio::time::timeout(COMMAND_DEADLINE, async {
            while let Some(line) = lines.next_line().await.expect("read login stderr") {
                let candidate = line.trim();
                if authorize_url.is_none()
                    && candidate.starts_with("http")
                    && candidate.contains("/authorize?")
                {
                    let url = candidate.to_owned();
                    tokio::spawn(async move {
                        let _ = follow_authorize_url(&url).await;
                    });
                    authorize_url = Some(candidate.to_owned());
                }
                stderr.push_str(&line);
                stderr.push('\n');
            }
            child.wait().await.expect("wait for rkat mcp login")
        })
        .await;
        let status = finished
            .unwrap_or_else(|_| panic!("rkat mcp login did not finish; stderr so far:\n{stderr}"));
        let stdout = stdout
            .await
            .expect("stdout reader")
            .expect("read login stdout");
        LoginRun {
            success: status.success(),
            stdout,
            stderr,
            authorize_url,
        }
    }

    /// The MCP OAuth credentials in the file token store.
    fn credentials(&self) -> Vec<PathBuf> {
        let dir = self.home.join("config/meerkat/credentials/mcp-oauth");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .map(|entry| entry.expect("credential entry").path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        files.sort();
        files
    }
}

#[derive(Debug)]
struct LoginRun {
    success: bool,
    stdout: String,
    stderr: String,
    /// The authorize URL the login printed, which the test followed.
    authorize_url: Option<String>,
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_success(step: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{step} failed: status={} stdout={} stderr={}",
        output.status,
        stdout_of(output),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn assert_mentions(step: &str, text: &str, expected: &[&str]) {
    for needle in expected {
        assert!(
            text.contains(needle),
            "{step} does not mention {needle:?}:\n{text}"
        );
    }
}

fn stored(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read credential")).expect("credential JSON")
}

fn requested(fixture: &McpOAuthFixture, path: &str) -> bool {
    fixture.request_paths().iter().any(|seen| seen == path)
}

/// Discover binds the subject UserInfo reports at the first login. A
/// selection change is a new credential slot: an unverified login of the
/// same server later stores its own credential and leaves the discovered
/// one byte for byte as it was.
#[tokio::test]
async fn first_login_with_discover_binds_the_userinfo_subject_in_its_own_slot() {
    let fixture = McpOAuthFixture::spawn().await.expect("OAuth MCP fixture");
    let roots = Roots::new();
    let added = roots
        .add(&fixture, &["--oauth-account-selection", "discover"])
        .await;
    assert_success("mcp add --oauth-account-selection discover", &added);
    assert_mentions(
        "mcp add",
        &stdout_of(&added),
        &["OAuth account: discovered at the first login"],
    );

    let discovered = roots.login().await;
    assert!(discovered.success, "discover login failed: {discovered:#?}");
    assert!(
        discovered.authorize_url.is_some(),
        "positive control: the login reached the browser: {discovered:#?}"
    );
    assert_mentions(
        "discover login",
        &discovered.stdout,
        &[&format!(
            "Logged in MCP server '{SERVER}' ({}) as {SUBJECT} (bound by discovery)",
            fixture.mcp_url()
        )],
    );
    let credentials = roots.credentials();
    assert_eq!(credentials.len(), 1, "one credential: {credentials:?}");
    let discovered_credential = credentials[0].clone();
    let discovered_bytes = std::fs::read(&discovered_credential).expect("read credential");
    assert_eq!(stored(&discovered_credential)["account_id"], SUBJECT);

    assert_success("mcp remove", &roots.run(&["mcp", "remove", SERVER]).await);
    assert_success(
        "mcp add --oauth-account-selection unverified",
        &roots
            .add(&fixture, &["--oauth-account-selection", "unverified"])
            .await,
    );
    let unverified = roots.login().await;
    assert!(
        unverified.success,
        "unverified login failed: {unverified:#?}"
    );
    let credentials = roots.credentials();
    assert_eq!(
        credentials.len(),
        2,
        "the unverified login stores its own credential: {credentials:?}"
    );
    assert_eq!(
        std::fs::read(&discovered_credential).expect("read credential"),
        discovered_bytes,
        "the discovered credential is untouched"
    );
    let unverified_credential = credentials
        .iter()
        .find(|path| **path != discovered_credential)
        .expect("the unverified credential");
    assert!(
        stored(unverified_credential).get("account_id").is_none(),
        "an unverified credential carries no account"
    );
}

/// An authorization server without account evidence cannot satisfy
/// discover. The login is refused before client registration and before any
/// browser step, stores nothing, names the deliberate unverified
/// alternative, and leaves the configured selection as it was.
#[tokio::test]
async fn first_login_with_discover_and_no_account_evidence_is_refused_before_registration() {
    let fixture = McpOAuthFixture::spawn_with(AccountEvidence::NoUserInfo)
        .await
        .expect("OAuth MCP fixture");
    let roots = Roots::new();
    assert_success(
        "mcp add --oauth-account-selection discover",
        &roots
            .add(&fixture, &["--oauth-account-selection", "discover"])
            .await,
    );

    let refused = roots.login().await;
    assert!(!refused.success, "discover without evidence must fail");
    assert!(
        refused.authorize_url.is_none(),
        "refused before any browser step: {refused:#?}"
    );
    assert_mentions(
        "discover refusal",
        &refused.stderr,
        &[
            &format!("MCP server '{SERVER}' gave no account evidence this host can verify"),
            "OpenID Connect UserInfo",
            "no credential was stored",
            "could not be reached or failed temporarily, retry first",
            &format!("rkat mcp login '{SERVER}' --scope project"),
            &format!("rkat mcp remove '{SERVER}' --scope project"),
            &format!(
                "rkat mcp add '{SERVER}' --scope project --url '{}' --oauth-account-selection unverified",
                fixture.mcp_url()
            ),
            "no account is verified, stored or reported",
        ],
    );
    assert!(
        requested(&fixture, "/.well-known/openid-configuration"),
        "positive control: the account evidence was probed: {:?}",
        fixture.request_paths()
    );
    for unreached in ["/register", "/authorize", "/token", "/userinfo"] {
        assert!(
            !requested(&fixture, unreached),
            "{unreached} was requested: {:?}",
            fixture.request_paths()
        );
    }
    assert!(roots.credentials().is_empty(), "nothing is stored");

    let configured = roots.run(&["mcp", "get", SERVER, "--json"]).await;
    assert_success("mcp get --json", &configured);
    let configured: Value = serde_json::from_slice(&configured.stdout).expect("mcp get JSON");
    assert_eq!(
        configured["oauth_account_selection"], "discover",
        "a refused login never changes the selection"
    );
}

/// A Known selection must be the subject the provider reports: another
/// signed-in account is refused after the browser step and nothing is
/// stored.
#[tokio::test]
async fn first_login_with_a_known_account_refuses_another_signed_in_subject() {
    let fixture = McpOAuthFixture::spawn().await.expect("OAuth MCP fixture");
    let roots = Roots::new();
    assert_success(
        "mcp add --oauth-account",
        &roots
            .add(&fixture, &["--oauth-account", "another-subject-9"])
            .await,
    );

    let refused = roots.login().await;
    assert!(!refused.success, "a mismatched account must fail");
    assert!(
        refused.authorize_url.is_some(),
        "positive control: the human signed in: {refused:#?}"
    );
    assert!(
        requested(&fixture, "/userinfo"),
        "positive control: the account was observed: {:?}",
        fixture.request_paths()
    );
    assert_mentions(
        "account mismatch",
        &refused.stderr,
        &[
            &format!(
                "The account signed in for MCP server '{SERVER}' is not the subject its --oauth-account names"
            ),
            "no credential was stored",
            "add the server again with the right subject ID",
        ],
    );
    assert!(roots.credentials().is_empty(), "nothing is stored");
}

/// Unverified is the deliberate resource-bound mode: it logs in against a
/// server without account evidence, runs no account check, stores no
/// account, and says it is unverified wherever the CLI shows it.
#[tokio::test]
async fn first_login_with_unverified_needs_no_account_evidence_and_says_so() {
    let fixture = McpOAuthFixture::spawn_with(AccountEvidence::NoUserInfo)
        .await
        .expect("OAuth MCP fixture");
    let roots = Roots::new();
    let added = roots
        .add(&fixture, &["--oauth-account-selection", "unverified"])
        .await;
    assert_success("mcp add --oauth-account-selection unverified", &added);
    assert_mentions(
        "mcp add",
        &stdout_of(&added),
        &["OAuth account: not verified (resource-bound access, no account evidence)"],
    );

    let unverified = roots.login().await;
    assert!(
        unverified.success,
        "unverified login failed: {unverified:#?}"
    );
    assert!(
        unverified.authorize_url.is_some(),
        "positive control: the login reached the browser: {unverified:#?}"
    );
    assert_mentions(
        "unverified login",
        &unverified.stdout,
        &[&format!(
            "Logged in MCP server '{SERVER}' ({}) without account verification (unverified, resource-bound access)",
            fixture.mcp_url()
        )],
    );
    assert!(
        requested(&fixture, "/token"),
        "positive control: the code was exchanged: {:?}",
        fixture.request_paths()
    );
    for unreached in ["/.well-known/openid-configuration", "/userinfo"] {
        assert!(
            !requested(&fixture, unreached),
            "no account check runs, but {unreached} was requested: {:?}",
            fixture.request_paths()
        );
    }
    let credentials = roots.credentials();
    assert_eq!(credentials.len(), 1, "one credential: {credentials:?}");
    assert!(
        stored(&credentials[0]).get("account_id").is_none(),
        "an unverified credential carries no account"
    );

    let shown = roots.run(&["mcp", "get", SERVER]).await;
    assert_success("mcp get", &shown);
    assert_mentions(
        "mcp get",
        &stdout_of(&shown),
        &["not verified (resource-bound access)"],
    );
    let listed = roots.run(&["mcp", "list", "--json"]).await;
    assert_success("mcp list --json", &listed);
    let listed: Value = serde_json::from_slice(&listed.stdout).expect("mcp list JSON");
    let entry = listed
        .as_array()
        .and_then(|servers| servers.iter().find(|server| server["name"] == SERVER))
        .expect("the listed server");
    assert_eq!(entry["oauth_account_selection"], "unverified");
}

/// A server added without a selection is refused before any request and
/// before the credential store is opened, with the server, its scope and
/// every way to choose a selection named. The printed commands paste into a
/// shell as shown, for a URL with `?` and `&`.
#[tokio::test]
async fn first_login_without_a_selection_names_every_remedy_before_any_request() {
    let fixture = McpOAuthFixture::spawn().await.expect("OAuth MCP fixture");
    let roots = Roots::new();
    let url = format!("{}?tenant=a&team=b", fixture.mcp_url());
    assert_success(
        "mcp add",
        &roots.run(&["mcp", "add", SERVER, "--url", &url]).await,
    );

    let refused = roots.login().await;
    assert!(!refused.success, "a server without a selection must fail");
    assert!(
        refused.authorize_url.is_none(),
        "refused before any browser step: {refused:#?}"
    );
    let example = format!(
        "rkat mcp add '{SERVER}' --scope project --url '{url}' --oauth-account-selection discover"
    );
    assert_mentions(
        "missing selection",
        &refused.stderr,
        &[
            &format!("MCP server '{SERVER}' has no OAuth account selection"),
            &format!("rkat mcp remove '{SERVER}' --scope project"),
            "--oauth-account-selection discover",
            "--oauth-account <SUBJECT_ID>",
            "--oauth-account-selection unverified",
            &format!("For example: {example}"),
        ],
    );
    let pasted = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("printf '%s\\n' {example}"))
        .output()
        .expect("run /bin/sh");
    assert_eq!(
        String::from_utf8(pasted.stdout)
            .expect("UTF-8 words")
            .lines()
            .collect::<Vec<_>>(),
        [
            "rkat",
            "mcp",
            "add",
            SERVER,
            "--scope",
            "project",
            "--url",
            url.as_str(),
            "--oauth-account-selection",
            "discover",
        ],
        "the printed command pastes as one word per argument"
    );
    assert!(
        fixture.request_paths().is_empty(),
        "refused before any request: {:?}",
        fixture.request_paths()
    );
    assert!(
        !roots.home.join("config/meerkat/credentials").exists(),
        "refused before the credential store is opened"
    );
}

/// `rkat mcp add` names the account choices for a streamable HTTP server
/// that could use OAuth sign-in (no selection, no static `Authorization`),
/// and never for stdio, SSE, a static `Authorization` header or a server
/// that already has a selection.
#[tokio::test]
async fn mcp_add_names_the_account_choices_only_where_oauth_sign_in_applies() {
    const HINT: [&str; 4] = [
        "uses OAuth sign-in",
        "--oauth-account-selection discover",
        "unverified",
        "--oauth-account <SUBJECT_ID>",
    ];
    let roots = Roots::new();
    let url = "https://mcp.example.test/mcp";
    let hinted = roots.run(&["mcp", "add", "plain-http", "--url", url]).await;
    assert_success("mcp add plain-http", &hinted);
    assert_mentions("mcp add plain-http", &stdout_of(&hinted), &HINT);

    for (name, args) in [
        (
            "static-auth",
            &["--url", url, "-H", "Authorization: Bearer fixture-token"][..],
        ),
        ("legacy-sse", &["--transport", "sse", "--url", url][..]),
        ("local-tool", &["--", "fixture-mcp-server", "--stdio"][..]),
        (
            "discovered",
            &["--url", url, "--oauth-account-selection", "discover"][..],
        ),
        ("known", &["--url", url, "--oauth-account", "subject-7"][..]),
    ] {
        let mut command = vec!["mcp", "add", name];
        command.extend_from_slice(args);
        let added = roots.run(&command).await;
        assert_success(name, &added);
        assert!(
            !stdout_of(&added).contains(HINT[0]),
            "{name} gets no OAuth sign-in hint:\n{}",
            stdout_of(&added)
        );
    }
}
