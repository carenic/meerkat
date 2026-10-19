#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods
)]

use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Stdio;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{Duration, timeout};

/// Cargo, nextest, the e2e lane harness and the Bazel graph advertise the
/// crate's own binary through `CARGO_BIN_EXE_rkat-mcp`, so a missing binary
/// fails the test instead of skipping it.
fn mcp_binary_path() -> PathBuf {
    let advertised = PathBuf::from(
        std::env::var_os("CARGO_BIN_EXE_rkat-mcp")
            .expect("cargo advertises CARGO_BIN_EXE_rkat-mcp to the crate's integration tests"),
    );
    // Bazel wires a runfiles-relative path; the server below is spawned
    // with its own working directory, so resolve it first.
    advertised.canonicalize().unwrap_or_else(|error| {
        panic!(
            "CARGO_BIN_EXE_rkat-mcp names a missing binary {}: {error}",
            advertised.display()
        )
    })
}

struct McpProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl McpProcess {
    async fn send(&mut self, request: &Value) -> Result<(), Box<dyn std::error::Error>> {
        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read_response(&mut self, id: u64) -> Result<Value, Box<dyn std::error::Error>> {
        loop {
            let mut line = String::new();
            timeout(Duration::from_secs(30), self.stdout.read_line(&mut line)).await??;
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line.trim())?;
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                return Ok(value);
            }
        }
    }
}

fn unwrap_tool_payload(value: &Value) -> Value {
    let raw = value["result"]["content"][0]["text"]
        .as_str()
        .expect("wrapped MCP payload text");
    serde_json::from_str(raw).expect("wrapped payload json")
}

async fn spawn_mcp_stdio(
    cwd: &std::path::Path,
    state_root: &std::path::Path,
) -> Result<McpProcess, Box<dyn std::error::Error>> {
    let binary = mcp_binary_path();
    let mut child = Command::new(binary)
        .current_dir(cwd)
        .env("HOME", cwd)
        .env("XDG_DATA_HOME", cwd.join("data"))
        .env("RKAT_TEST_CLIENT", "1")
        .args([
            "--realm",
            "mcp-binary-e2e",
            "--state-root",
            state_root.to_str().ok_or("invalid state root")?,
            "--context-root",
            cwd.to_str().ok_or("invalid cwd")?,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdin = child.stdin.take().ok_or("missing stdin")?;
    let stdout = BufReader::new(child.stdout.take().ok_or("missing stdout")?);
    Ok(McpProcess {
        child,
        stdin,
        stdout,
    })
}

#[tokio::test]
#[ignore = "lane:e2e-build"]
async fn stdio_e2e_run_and_resume() -> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let project_dir = temp.path().join("project");
    let state_root = temp.path().join("state");
    tokio::fs::create_dir_all(project_dir.join(".rkat")).await?;
    tokio::fs::create_dir_all(project_dir.join("data")).await?;
    tokio::fs::create_dir_all(&state_root).await?;
    tokio::fs::write(
        project_dir.join(".rkat/config.toml"),
        "[agent]\nmodel = \"claude-sonnet-4-5\"\nmax_tokens_per_turn = 128\n",
    )
    .await?;

    let mut mcp = spawn_mcp_stdio(&project_dir, &state_root).await?;

    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    }))
    .await?;
    let initialize = mcp.read_response(1).await?;
    assert_eq!(initialize["result"]["serverInfo"]["name"], "rkat-mcp");

    mcp.send(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    }))
    .await?;

    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    }))
    .await?;
    let tools = mcp.read_response(2).await?;
    assert!(
        tools["result"]["tools"]
            .as_array()
            .is_some_and(|entries| entries.iter().any(|entry| entry["name"] == "meerkat_run"))
    );

    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "meerkat_run",
            "arguments": {
                "prompt": "Remember the token BINARY_MCP_42 and reply with ok.",
                "model": "claude-sonnet-4-5"
            }
        }
    }))
    .await?;
    let created = mcp.read_response(3).await?;
    let created_payload = unwrap_tool_payload(&created);
    let session_id = created_payload["session_id"]
        .as_str()
        .ok_or("missing session_id")?
        .to_string();
    assert_eq!(created_payload["content"][0]["text"], "ok");

    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 35,
        "method": "tools/call",
        "params": {
            "name": "meerkat_sessions",
            "arguments": {}
        }
    }))
    .await?;
    let listed_after_run = unwrap_tool_payload(&mcp.read_response(35).await?);
    assert!(
        listed_after_run["sessions"]
            .as_array()
            .is_some_and(|sessions| sessions
                .iter()
                .any(|entry| entry["session_id"].as_str() == Some(session_id.as_str())))
    );

    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "meerkat_resume",
            "arguments": {
                "session_id": session_id,
                "prompt": "What token was I asked to remember? Reply with the token only."
            }
        }
    }))
    .await?;
    let resumed = mcp.read_response(4).await?;
    let resumed_payload = unwrap_tool_payload(&resumed);
    assert_eq!(
        resumed_payload["session_id"].as_str(),
        Some(session_id.as_str())
    );
    assert_eq!(resumed_payload["content"][0]["text"], "ok");

    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 36,
        "method": "tools/call",
        "params": {
            "name": "meerkat_sessions",
            "arguments": {}
        }
    }))
    .await?;
    let listed_after_resume = unwrap_tool_payload(&mcp.read_response(36).await?);
    assert!(
        listed_after_resume["sessions"]
            .as_array()
            .is_some_and(|sessions| sessions
                .iter()
                .filter(|entry| entry["session_id"].as_str() == Some(session_id.as_str()))
                .count()
                == 1),
        "resume should not create a duplicate session entry: {listed_after_resume}"
    );

    let _ = mcp.child.kill().await;
    Ok(())
}
