//! Stdio MCP server fixtures for process-custody tests (Linux only).
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt as _, BufReader};

/// A shell MCP server that answers `initialize` and `tools/list` (no tools),
/// then ignores stdin EOF: once its stdin closes it keeps running for 60 s,
/// like a server flushing state on EOF. With `grandchild_report`, it runs
/// behind a wrapper the way `sh -c`, `npx` or `uvx` launches do: it first
/// starts a `sleep 60` grandchild and writes `<own pid> <grandchild pid>` to
/// that FIFO.
pub(crate) fn sh_mcp_server_args(grandchild_report: Option<&Path>) -> Vec<String> {
    let prelude = grandchild_report.map_or_else(String::new, |fifo| {
        format!("sleep 60 & echo $$ $! > '{}'\n", fifo.display())
    });
    sh_mcp_server_script(&prelude, "sleep 60\n")
}

/// [`sh_mcp_server_args_with_prelude`] listing one tool per raw name.
pub(crate) fn sh_mcp_server_args_listing(prelude: &str, tools: &[&str]) -> Vec<String> {
    let listed: Vec<String> = tools
        .iter()
        .map(|name| format!(r#"{{"name":"{name}","inputSchema":{{"type":"object"}}}}"#))
        .collect();
    sh_mcp_server_args_with_prelude(&format!(
        "MCP_FIXTURE_TOOLS='[{}]'\n{prelude}",
        listed.join(",")
    ))
}

/// [`sh_mcp_server_args`] running `prelude` first.
pub(crate) fn sh_mcp_server_args_with_prelude(prelude: &str) -> Vec<String> {
    sh_mcp_server_script(prelude, "sleep 60\n")
}

/// A shell MCP server that runs `prelude`, answers `initialize` and
/// `tools/list` (no tools) until its stdin reaches EOF, then runs `on_eof`.
pub(crate) fn sh_mcp_server_script(prelude: &str, on_eof: &str) -> Vec<String> {
    let script = format!(
        r#"{prelude}while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":"2025-03-26","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"sh-fixture","version":"0"}}}}}}\n' "$id" ;;
    *'"method":"tools/list"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":{{"tools":%s}}}}\n' "$id" "${{MCP_FIXTURE_TOOLS:-[]}}" ;;
  esac
done
{on_eof}"#
    );
    vec!["-c".to_string(), script]
}

/// A FIFO a fixture process writes its pids to. Reading it waits for the
/// write itself, so tests learn the pids without polling.
pub(crate) struct PidReport {
    path: PathBuf,
    lines: BufReader<tokio::net::unix::pipe::Receiver>,
}

impl PidReport {
    pub(crate) fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "meerkat-mcp-{tag}-{}-{}.fifo",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        nix::unistd::mkfifo(&path, nix::sys::stat::Mode::S_IRWXU).expect("create pid FIFO");
        // Read-write on the receiver keeps a writer open, so reads wait for
        // the fixture's line instead of seeing EOF before it opens the FIFO.
        let receiver = tokio::net::unix::pipe::OpenOptions::new()
            .read_write(true)
            .open_receiver(&path)
            .expect("open pid FIFO");
        Self {
            path,
            lines: BufReader::new(receiver),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The fixture's next report line, as written.
    pub(crate) async fn line(&mut self) -> String {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(10), self.lines.read_line(&mut line))
            .await
            .expect("fixture must report")
            .expect("read report FIFO");
        line.trim_end_matches('\n').to_owned()
    }

    /// The whitespace-separated pids of the fixture's report line.
    pub(crate) async fn pids(&mut self) -> Vec<u32> {
        let mut line = String::new();
        // Bounded failure backstop only: the fixture writes at startup.
        tokio::time::timeout(Duration::from_secs(10), self.lines.read_line(&mut line))
            .await
            .expect("fixture must report its pids")
            .expect("read pid FIFO");
        line.split_whitespace()
            .map(|pid| pid.parse().expect("fixture reports numeric pids"))
            .collect()
    }
}

impl Drop for PidReport {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Whether `pid` has exited: reaped (no `/proc` entry), dead and awaiting
/// reaping (zombie or dead state), or past the release of its files in
/// `do_exit`. The last case is a SIGKILLed grandchild that has closed the
/// stdout pipe the custody waits on but that its new parent has not reaped
/// yet; it holds nothing and runs no more code.
pub(crate) fn process_exited(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return true;
    };
    let state = stat
        .rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().next());
    if matches!(state, Some("Z" | "X")) {
        return true;
    }
    std::fs::read_dir(format!("/proc/{pid}/fd")).map_or(true, |mut fds| fds.next().is_none())
}

/// Wait up to `bound` for `pid` to have exited ([`process_exited`]). For a
/// process signalled without being waited for, such as a group member killed
/// after its leader was reaped, whose death is asynchronous.
pub(crate) async fn exits_within(pid: u32, bound: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + bound;
    while !process_exited(pid) {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    true
}

/// Whether process group `pgid` has no members left.
pub(crate) fn process_group_gone(pgid: u32) -> bool {
    let group = nix::unistd::Pid::from_raw(i32::try_from(pgid).expect("pid fits i32"));
    nix::sys::signal::killpg(group, None) == Err(nix::errno::Errno::ESRCH)
}
