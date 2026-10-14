#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! A stock host's resolved OS process confinement installed on its factory:
//! every launch kind takes the host's requirement, a launch the backend
//! cannot confine is refused and never runs unconfined, and the default
//! leaves the factory as the trusted host.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meerkat::{
    AgentBuildConfig, AgentFactory, HostConfinement, LlmDoneOutcome, LlmEvent, LlmRequest,
};
use meerkat_client::LlmClient;
use meerkat_core::confinement::{
    ConfinementRefusal, ConfinementSpec, ExecutionConfinement, FilesystemAccess, IpNetworkAccess,
    PathAccess, PlatformBaseline,
};
use meerkat_core::{
    AgentEvent, AgentToolDispatcher, Config, ConfinementConfig, CustomConfinement,
    HookAdapterConfig, HookCapability, HookEntryConfig, HookFailureReason, HookId, HookPoint,
    LaunchConfinement, Message, Provider, ToolCallView, ToolDef, ToolDispatchOutcome, ToolError,
    ToolResult,
};
use meerkat_tools::builtin::shell::ShellConfinement;
use serde_json::json;

/// Calls `tool` once with `args`, then ends the run with "continued".
struct ScriptedClient {
    tool: &'static str,
    args: serde_json::Value,
    requests: Mutex<Vec<Vec<Message>>>,
}

impl ScriptedClient {
    fn new(tool: &'static str, args: serde_json::Value) -> Self {
        Self {
            tool,
            args,
            requests: Mutex::new(Vec::new()),
        }
    }

    /// The tool results the model received on its second request.
    fn returned_results(&self) -> Vec<(bool, String)> {
        let requests = self.requests.lock().unwrap();
        assert_eq!(requests.len(), 2, "the run reaches the next model turn");
        requests[1]
            .iter()
            .filter_map(|message| match message {
                Message::ToolResults { results, .. } => Some(results.as_slice()),
                _ => None,
            })
            .flatten()
            .map(|result| (result.is_error, result.text_content()))
            .collect()
    }
}

#[async_trait::async_trait]
impl LlmClient for ScriptedClient {
    fn project_replay_messages(
        &self,
        messages: &[Message],
    ) -> Result<Vec<Message>, meerkat_client::LlmError> {
        Ok(messages.to_vec())
    }

    fn stream<'a>(&'a self, request: &'a LlmRequest) -> meerkat_llm_core::LlmStream<'a> {
        let first = {
            let mut requests = self.requests.lock().unwrap();
            let first = requests.is_empty();
            requests.push(request.messages.clone());
            first
        };
        let (event, stop_reason) = if first {
            assert!(
                request.tools.iter().any(|tool| tool.name == self.tool),
                "the model sees {}",
                self.tool
            );
            (
                LlmEvent::ToolCallComplete {
                    id: "confined-call".into(),
                    name: self.tool.into(),
                    args: self.args.clone(),
                    meta: None,
                },
                meerkat_core::StopReason::ToolUse,
            )
        } else {
            (
                LlmEvent::TextDelta {
                    delta: "continued".into(),
                    meta: None,
                },
                meerkat_core::StopReason::EndTurn,
            )
        };
        Box::pin(futures::stream::iter(vec![
            Ok(event),
            Ok(LlmEvent::UsageUpdate {
                usage: meerkat_core::TurnUsage::host_declared(
                    Provider::Other,
                    &request.model,
                    meerkat_core::Usage::default(),
                ),
            }),
            Ok(LlmEvent::Done {
                outcome: LlmDoneOutcome::Success { stop_reason },
            }),
        ]))
    }

    fn provider(&self) -> Provider {
        Provider::Other
    }

    async fn health_check(&self) -> Result<(), meerkat_client::LlmError> {
        Ok(())
    }
}

#[derive(Default)]
struct GuardedTool(AtomicUsize);

#[async_trait::async_trait]
impl AgentToolDispatcher for GuardedTool {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        vec![Arc::new(ToolDef::new(
            "guarded_tool",
            "A tool with a command-hook prerequisite",
            json!({"type":"object"}),
        ))]
        .into()
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::new(call.id.into(), "tool completed".into(), false).into())
    }
}

/// A requirement over `root` that no platform backend enforces: descendant
/// termination is unsupported everywhere, so every confined launch is
/// refused before its program runs, on any OS.
fn unenforceable(root: &Path) -> ExecutionConfinement {
    ConfinementSpec {
        baseline: PlatformBaseline::CommandRuntimeV1,
        read: FilesystemAccess::Paths(vec![PathAccess::Subtree(root.into())]),
        write: FilesystemAccess::Paths(vec![PathAccess::Subtree(root.into())]),
        deny_read: vec![],
        deny_write: vec![],
        network: IpNetworkAccess::Denied,
        unix_connect: vec![],
        require_descendant_termination: true,
    }
    .try_into()
    .unwrap()
}

/// The host confinement a `[confinement.all]` custom selection resolves to.
fn host_confinement(root: &Path) -> HostConfinement {
    HostConfinement::resolve(
        &ConfinementConfig::for_all(LaunchConfinement::Custom(CustomConfinement {
            working_directory: root.to_path_buf(),
            requirement: unenforceable(root),
        })),
        None,
    )
    .unwrap()
}

fn confined_factory(root: &Path) -> AgentFactory {
    AgentFactory::new(root.join("sessions"))
        .session_store(Arc::new(meerkat_store::MemoryStore::new()))
        .runtime_root(root.join("runtime"))
        .project_root(root)
        .builtins(false)
        .shell(true)
        .with_host_confinement(&host_confinement(root))
}

fn assert_confinement_refused(text: &str) {
    let body: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|error| panic!("typed refusal body ({error}): {text}"));
    assert_eq!(body["error"], "confinement_refused", "{text}");
    assert_eq!(
        body["data"]["refusal"],
        json!(ConfinementRefusal::UnsupportedRequirement),
        "{text}"
    );
}

#[test]
fn installs_the_requirement_for_every_launch_kind() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let factory = confined_factory(&root);
    assert_eq!(
        factory.shell_confinement(),
        &ShellConfinement::Required {
            requirement: unenforceable(&root)
        }
    );
    #[cfg(feature = "mcp")]
    assert_eq!(
        factory.mcp_stdio_launch_profile().capabilities().err(),
        Some(ConfinementRefusal::UnsupportedRequirement)
    );
}

#[test]
fn the_trusted_host_default_leaves_the_factory_unconfined() {
    let factory = AgentFactory::minimal().with_host_confinement(&HostConfinement::trusted_host());
    assert_eq!(factory.shell_confinement(), &ShellConfinement::TrustedHost);
    #[cfg(feature = "mcp")]
    assert!(matches!(
        factory.mcp_stdio_launch_profile().capabilities(),
        Ok(None)
    ));
}

#[test]
fn installing_the_trusted_host_never_downgrades_an_installed_requirement() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let factory = confined_factory(&root).with_host_confinement(&HostConfinement::trusted_host());
    assert_eq!(
        factory.shell_confinement(),
        &ShellConfinement::Required {
            requirement: unenforceable(&root)
        }
    );
    #[cfg(feature = "mcp")]
    assert_eq!(
        factory.mcp_stdio_launch_profile().capabilities().err(),
        Some(ConfinementRefusal::UnsupportedRequirement)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_refused_shell_launch_never_runs_unconfined() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let marker = root.join("shell-entered");
    let client = Arc::new(ScriptedClient::new(
        "shell",
        json!({"command": format!("printf entered > '{}'", marker.display())}),
    ));
    let mut build = AgentBuildConfig::new("claude-sonnet-4-5");
    build.llm_client_override = Some(client.clone());
    let mut agent = tokio::time::timeout(
        Duration::from_secs(15),
        confined_factory(&root).build_agent(build, &Config::default()),
    )
    .await
    .unwrap()
    .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        agent.run("Run the command".to_string().into()),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(result.text, "continued");
    let results = client.returned_results();
    assert_eq!(results.len(), 1);
    assert!(results[0].0, "the refused launch is an error result");
    assert_confinement_refused(&results[0].1);
    assert!(!marker.exists(), "the refused command never ran");
}

#[cfg(unix)]
#[tokio::test]
async fn a_refused_command_hook_launch_never_runs_unconfined() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let marker = root.join("hook-entered");
    let mut config = Config::default();
    config.hooks.entries.push(HookEntryConfig {
        id: HookId::new("confined-command-hook"),
        point: HookPoint::PreToolExecution,
        capability: HookCapability::Guardrail,
        timeout_ms: Some(5_000),
        runtime: HookAdapterConfig::command(
            "/bin/sh",
            vec![
                "-c".into(),
                "printf entered > \"$1\"; cat >/dev/null; printf '{}'".into(),
                "hook-probe".into(),
                marker.display().to_string(),
            ],
            HashMap::new(),
        ),
        ..Default::default()
    });
    let client = Arc::new(ScriptedClient::new("guarded_tool", json!({})));
    let tool = Arc::new(GuardedTool::default());
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let mut build = AgentBuildConfig::new("claude-sonnet-4-5");
    build.llm_client_override = Some(client.clone());
    build.external_tools = Some(tool.clone());
    build.event_tx = Some(tx);
    let factory = confined_factory(&root).shell(false);
    let mut agent =
        tokio::time::timeout(Duration::from_secs(15), factory.build_agent(build, &config))
            .await
            .unwrap()
            .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        agent.run("Attempt the guarded operation".to_string().into()),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(result.text, "continued");
    assert_eq!(
        tool.0.load(Ordering::SeqCst),
        0,
        "the refused hook blocks its tool"
    );
    let results = client.returned_results();
    assert_eq!(results.len(), 1);
    assert!(results[0].0);
    assert_confinement_refused(&results[0].1);
    let mut refusals = 0;
    while let Ok(event) = rx.try_recv() {
        if let AgentEvent::HookLaunchRefused {
            hook_id, reason, ..
        } = event
        {
            assert_eq!(hook_id, HookId::new("confined-command-hook"));
            assert_eq!(
                reason,
                HookFailureReason::ConfinementRefused {
                    refusal: ConfinementRefusal::UnsupportedRequirement
                }
            );
            refusals += 1;
        }
    }
    assert_eq!(refusals, 1);
    assert!(!marker.exists(), "the refused hook never ran");
}

#[cfg(all(unix, feature = "mcp"))]
#[tokio::test]
async fn a_refused_mcp_stdio_launch_never_runs_unconfined() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let marker = root.join("mcp-entered");
    let client = Arc::new(ScriptedClient::new("guarded_tool", json!({})));
    let mut build = AgentBuildConfig::new("claude-sonnet-4-5");
    build.llm_client_override = Some(client.clone());
    build.external_tools = Some(Arc::new(GuardedTool::default()));
    build.wait_for_mcp = true;
    build.mcp_servers = vec![meerkat_core::McpServerConfig::stdio(
        "confined-server",
        "/bin/sh",
        vec!["-c".into(), "printf entered > \"$MARKER\"".into()],
        HashMap::from([("MARKER".into(), marker.display().to_string())]),
    )];
    let factory = confined_factory(&root).shell(false);
    let mut agent = tokio::time::timeout(
        Duration::from_secs(15),
        factory.build_agent(build, &Config::default()),
    )
    .await
    .unwrap()
    .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        agent.run("Use the available tool".to_string().into()),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(result.text, "continued");
    assert!(!marker.exists(), "the refused server never started");
}
