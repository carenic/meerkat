//! MCP attempt guards: the host's per-attempt guard is begun before any
//! effect of a stdio attempt and finished exactly once, with the attempt's
//! open outcome kept apart from its process end.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::{
    McpAttemptCancelCause, McpAttemptDisposition, McpAttemptGuard, McpAttemptGuardBounds,
    McpAttemptGuardFactory, McpAttemptGuardRequest, McpAttemptGuards, McpAttemptKey,
    McpAttemptNotInstalled, McpAttemptOpenOutcome, McpAttemptProcessEnd, McpAttemptRefusal,
    McpConstructionOwner, McpGuardCompletion, McpGuardFinishError, McpGuardStage,
};
use meerkat_core::ExternalToolDeltaPhase;
use meerkat_core::mcp_attachment::{
    McpAttachmentAttempt, McpAttachmentReadiness, McpAttachmentRefusal, McpStdioSetupFailureClass,
    McpStdioSetupPhase,
};
use meerkat_core::types::SessionId;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicU64;

const WAIT: Duration = Duration::from_secs(40);

/// Serves `initialize` and `tools/list` (no tools) until stdin closes.
const SERVE: &str = r#"while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"guarded","version":"0"}}}\n' "$id" ;;
    *'"method":"tools/list"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[]}}\n' "$id" ;;
  esac
done
"#;

fn shell(name: &str, script: &str) -> McpServerConfig {
    McpServerConfig::stdio(
        name,
        "/bin/sh".to_string(),
        vec!["-c".to_string(), script.to_string()],
        HashMap::new(),
    )
}

fn timeout(mut config: McpServerConfig, seconds: u32) -> McpServerConfig {
    config.connect_timeout_secs = Some(seconds);
    config
}

/// One recorded guard call, in global order.
#[derive(Debug, Clone)]
enum Call {
    Begin {
        key: McpAttemptKey,
        request: McpAttemptGuardRequest,
    },
    BeginReturned {
        key: McpAttemptKey,
    },
    Settle(McpAttemptDisposition),
    Cancel(McpAttemptDisposition),
    Apply,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Finish {
    Ok,
    Err,
    Panic,
}

/// A recording guard factory whose begins and finishes a test can hold.
struct Probe {
    calls: StdMutex<Vec<(u64, Call)>>,
    sequence: AtomicU64,
    refuse: StdMutex<HashSet<String>>,
    unguarded: StdMutex<HashSet<String>>,
    hold_begins: tokio::sync::watch::Sender<bool>,
    hold_finishes: tokio::sync::watch::Sender<bool>,
    /// Servers whose finishes are held, each until it leaves the set.
    hold_servers: tokio::sync::watch::Sender<HashSet<String>>,
    finish: StdMutex<Finish>,
}

impl Probe {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: StdMutex::new(Vec::new()),
            sequence: AtomicU64::new(0),
            refuse: StdMutex::new(HashSet::new()),
            unguarded: StdMutex::new(HashSet::new()),
            hold_begins: tokio::sync::watch::channel(false).0,
            hold_finishes: tokio::sync::watch::channel(false).0,
            hold_servers: tokio::sync::watch::channel(HashSet::new()).0,
            finish: StdMutex::new(Finish::Ok),
        })
    }

    fn push(&self, call: Call) {
        let at = self.sequence.fetch_add(1, Ordering::SeqCst);
        self.calls.lock().unwrap().push((at, call));
    }

    fn calls(&self) -> Vec<(u64, Call)> {
        self.calls.lock().unwrap().clone()
    }

    fn begins(&self, server: &str) -> Vec<McpAttemptKey> {
        self.calls()
            .into_iter()
            .filter_map(|(_, call)| match call {
                Call::Begin { key, .. } if key.server == server => Some(key),
                _ => None,
            })
            .collect()
    }

    /// Every finish of `server`'s attempts: (settle?, disposition).
    fn finishes(&self, server: &str) -> Vec<(bool, McpAttemptDisposition)> {
        self.calls()
            .into_iter()
            .filter_map(|(_, call)| match call {
                Call::Settle(disposition) if disposition.key.server == server => {
                    Some((true, disposition))
                }
                Call::Cancel(disposition) if disposition.key.server == server => {
                    Some((false, disposition))
                }
                _ => None,
            })
            .collect()
    }

    async fn until(&self, what: &str, done: impl Fn(&Self) -> bool) {
        tokio::time::timeout(WAIT, async {
            while !done(self) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}: {:#?}", self.calls()));
    }

    /// Exactly one finish of `server`, once it happened.
    async fn one_finish(&self, server: &str) -> (bool, McpAttemptDisposition) {
        self.until("a finish", |probe| !probe.finishes(server).is_empty())
            .await;
        // Nothing else arrives for that attempt.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let finishes = self.finishes(server);
        assert_eq!(finishes.len(), 1, "exactly one finish: {finishes:#?}");
        finishes.into_iter().next().unwrap()
    }
}

struct ProbeGuard {
    probe: Arc<Probe>,
}

impl ProbeGuard {
    async fn finish(
        &self,
        settle: bool,
        disposition: McpAttemptDisposition,
    ) -> Result<(), McpGuardFinishError> {
        let server = disposition.key.server.clone();
        self.probe.push(if settle {
            Call::Settle(disposition)
        } else {
            Call::Cancel(disposition)
        });
        let mut held = self.probe.hold_finishes.subscribe();
        let _ = held.wait_for(|held| !*held).await;
        let mut held = self.probe.hold_servers.subscribe();
        let _ = held.wait_for(|held| !held.contains(&server)).await;
        let finish = *self.probe.finish.lock().unwrap();
        match finish {
            Finish::Ok => Ok(()),
            Finish::Err => Err(McpGuardFinishError::new("the host refused the finish")),
            Finish::Panic => panic!("the host guard panicked"),
        }
    }
}

#[async_trait]
impl McpAttemptGuard for ProbeGuard {
    async fn settle(
        self: Box<Self>,
        disposition: McpAttemptDisposition,
    ) -> Result<(), McpGuardFinishError> {
        self.finish(true, disposition).await
    }

    async fn cancel(
        self: Box<Self>,
        disposition: McpAttemptDisposition,
    ) -> Result<(), McpGuardFinishError> {
        self.finish(false, disposition).await
    }
}

struct ProbeFactory(Arc<Probe>);

#[async_trait]
impl McpAttemptGuardFactory for ProbeFactory {
    async fn begin(
        &self,
        request: McpAttemptGuardRequest,
        _cancel: crate::McpAttemptCancellation,
    ) -> Result<Option<Box<dyn McpAttemptGuard>>, McpAttachmentRefusal> {
        let key = request.key.clone();
        let server = key.server.clone();
        self.0.push(Call::Begin {
            key: key.clone(),
            request,
        });
        let mut held = self.0.hold_begins.subscribe();
        let _ = held.wait_for(|held| !*held).await;
        self.0.push(Call::BeginReturned { key });
        if self.0.refuse.lock().unwrap().contains(&server) {
            return Err(McpAttachmentRefusal::Denied);
        }
        if self.0.unguarded.lock().unwrap().contains(&server) {
            return Ok(None);
        }
        Ok(Some(Box::new(ProbeGuard {
            probe: Arc::clone(&self.0),
        })))
    }
}

fn bounds(begin_ms: u64, settle_ms: u64) -> McpAttemptGuardBounds {
    McpAttemptGuardBounds::new(
        Duration::from_millis(begin_ms),
        Duration::from_millis(settle_ms),
        Duration::from_millis(settle_ms),
    )
    .unwrap()
}

fn session_owner() -> McpConstructionOwner {
    McpConstructionOwner::new(SessionId::new(), None)
}

fn installation(probe: &Arc<Probe>, bounds: McpAttemptGuardBounds) -> McpAttemptGuards {
    McpAttemptGuards::new(Arc::new(ProbeFactory(Arc::clone(probe))), bounds)
}

fn surface_router() -> McpRouter {
    McpRouter::new_with_surface_handle(Arc::new(
        meerkat_runtime::RuntimeExternalToolSurfaceHandle::ephemeral(),
    ))
}

fn guarded(guards: &McpAttemptGuards, owner: McpConstructionOwner) -> Arc<McpRouterAdapter> {
    Arc::new(McpRouterAdapter::new(
        surface_router().with_attempt_guards(guards.clone(), owner),
    ))
}

/// A prepared control whose apply decision the test sets; it records when
/// it is asked.
struct Control {
    server: McpServerConfig,
    apply: Result<(), McpAttachmentRefusal>,
    probe: Arc<Probe>,
}

impl McpAttachmentAuthority for Control {
    fn server(&self) -> &McpServerConfig {
        &self.server
    }
    fn authorize_apply(&self) -> Result<(), McpAttachmentRefusal> {
        self.probe.push(Call::Apply);
        self.apply.clone()
    }
    fn authorize_persist(
        &self,
        _scope: meerkat_core::mcp_config::McpScope,
    ) -> Result<(), McpAttachmentRefusal> {
        Err(McpAttachmentRefusal::Denied)
    }
}

async fn add(adapter: &Arc<McpRouterAdapter>, config: McpServerConfig) {
    adapter.stage_add(config).await.expect("stage");
    adapter.apply_staged().await.expect("apply");
}

async fn remove(adapter: &Arc<McpRouterAdapter>, name: &str) {
    adapter.stage_remove(name).await.expect("stage remove");
    adapter.apply_staged().await.expect("apply remove");
}

async fn until_notice(adapter: &Arc<McpRouterAdapter>, name: &str, phase: ExternalToolDeltaPhase) {
    let mut seen = Vec::new();
    tokio::time::timeout(WAIT, async {
        loop {
            let update = adapter.poll_external_updates().await;
            let done = update
                .notices
                .iter()
                .any(|notice| notice.target == name && notice.phase == phase);
            seen.extend(update.notices);
            if done {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {phase:?} notice for {name}: {seen:?}"));
}

fn report_of(
    observation: crate::McpRouterShutdownObservation,
) -> Arc<crate::McpRouterShutdownReport> {
    match observation {
        crate::McpRouterShutdownObservation::Reported(report) => report,
        other => panic!("the shutdown reported: {other:?}"),
    }
}

fn closed_receipt(process: &McpAttemptProcessEnd) -> Arc<crate::McpStdioCloseReceipt> {
    match process {
        McpAttemptProcessEnd::Closed(receipt) => Arc::clone(receipt),
        other => panic!("closed by its custody: {other:?}"),
    }
}

/// r1 1: a begin refusal spawns nothing and finishes nothing; the begin is
/// recorded as completed.
#[tokio::test]
async fn a_begin_refusal_spawns_nothing_and_finishes_nothing() {
    let probe = Probe::new();
    probe.refuse.lock().unwrap().insert("refused".to_string());
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    let marker = tempfile::tempdir().unwrap();
    let marker_path = marker.path().join("spawned");
    add(
        &adapter,
        shell(
            "refused",
            &format!("touch '{}'\n{SERVE}", marker_path.display()),
        ),
    )
    .await;
    until_notice(&adapter, "refused", ExternalToolDeltaPhase::Failed).await;
    assert!(!marker_path.exists(), "nothing was spawned");
    assert_eq!(probe.begins("refused").len(), 1);
    assert!(probe.finishes("refused").is_empty(), "no guard, no finish");
    assert!(
        adapter.stdio_setup_failure("refused").await.is_none(),
        "a guard refusal leaves no setup failure"
    );
    let report = report_of(adapter.shutdown_with_report().await);
    let begin = report
        .guards()
        .iter()
        .find(|record| record.key.server == "refused")
        .expect("the begin is recorded");
    assert_eq!(begin.stage, McpGuardStage::Begin);
    assert_eq!(begin.completion, McpGuardCompletion::Completed);
    let disposition = begin.disposition.as_ref().expect("the refusal is recorded");
    assert_eq!(
        disposition.open,
        McpAttemptOpenOutcome::Refused {
            refusal: McpAttemptRefusal::Guard
        }
    );
    assert!(matches!(
        disposition.process,
        McpAttemptProcessEnd::NotSpawned
    ));
}

/// Ok(None): the attempt proceeds unguarded; no finish, and the begin is
/// recorded.
#[tokio::test]
async fn an_unguarded_begin_lets_the_attempt_proceed_without_a_finish() {
    let probe = Probe::new();
    probe.unguarded.lock().unwrap().insert("free".to_string());
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("free", SERVE)).await;
    until_notice(&adapter, "free", ExternalToolDeltaPhase::Applied).await;
    let report = report_of(adapter.shutdown_with_report().await);
    assert!(probe.finishes("free").is_empty());
    assert!(
        report
            .guards()
            .iter()
            .any(|record| record.stage == McpGuardStage::Begin
                && record.completion == McpGuardCompletion::Completed)
    );
}

/// r1 2 and r1 12: begin runs before the attachment control's physical
/// entry, on the staged descriptor (no resolved secret); a later control
/// refusal cancels the guard once, with nothing spawned.
#[tokio::test]
async fn begin_precedes_the_physical_entry_and_a_control_refusal_cancels_once() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    let config = shell("controlled", SERVE);
    let attempt: McpAttachmentAttempt = adapter
        .stage_add_governed_observed(
            config.clone(),
            Arc::new(Control {
                server: config.clone(),
                apply: Err(McpAttachmentRefusal::Denied),
                probe: Arc::clone(&probe),
            }),
        )
        .await
        .expect("stage");
    adapter.apply_staged().await.expect("apply");
    let readiness = attempt.wait("any", WAIT).await;
    assert!(
        matches!(readiness, McpAttachmentReadiness::Failed { .. }),
        "{readiness:?}"
    );
    let (settled, disposition) = probe.one_finish("controlled").await;
    assert!(!settled);
    assert_eq!(
        disposition.open,
        McpAttemptOpenOutcome::Refused {
            refusal: McpAttemptRefusal::Control
        }
    );
    assert!(matches!(
        disposition.process,
        McpAttemptProcessEnd::NotSpawned
    ));
    let calls = probe.calls();
    let begin_at = calls
        .iter()
        .find_map(|(at, call)| matches!(call, Call::Begin { .. }).then_some(*at))
        .unwrap();
    let apply_at = calls
        .iter()
        .find_map(|(at, call)| matches!(call, Call::Apply).then_some(*at))
        .expect("the control was asked");
    assert!(begin_at < apply_at, "begin precedes the physical entry");
    let Some(Call::Begin { request, .. }) = calls
        .iter()
        .map(|(_, call)| call.clone())
        .find(|call| matches!(call, Call::Begin { .. }))
    else {
        unreachable!()
    };
    assert_eq!(request.server, config, "the staged descriptor, as staged");
    assert!(
        adapter.stdio_setup_failure("controlled").await.is_none(),
        "a control refusal leaves no setup failure"
    );
    adapter.shutdown().await;
}

/// The service-secret refusal after begin cancels the guard as
/// `Refused{ServiceSecret}`, nothing spawned.
#[tokio::test]
async fn a_service_secret_refusal_cancels_as_refused() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    let mut config = shell("secretive", SERVE);
    if let meerkat_core::mcp_config::McpTransportConfig::Stdio(stdio) = &mut config.transport {
        stdio.secret_env.insert("API_KEY".to_string());
    }
    add(&adapter, config).await;
    until_notice(&adapter, "secretive", ExternalToolDeltaPhase::Failed).await;
    let (settled, disposition) = probe.one_finish("secretive").await;
    assert!(!settled);
    assert_eq!(
        disposition.open,
        McpAttemptOpenOutcome::Refused {
            refusal: McpAttemptRefusal::ServiceSecret
        }
    );
    assert!(matches!(
        disposition.process,
        McpAttemptProcessEnd::NotSpawned
    ));
    adapter.shutdown().await;
}

/// r1 12: a confinement refusal after begin cancels as
/// `Refused{Confinement}`, nothing spawned.
#[tokio::test]
async fn a_confinement_refusal_cancels_as_refused() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let requirement: meerkat_core::confinement::ExecutionConfinement =
        meerkat_core::confinement::ConfinementSpec {
            baseline: meerkat_core::confinement::PlatformBaseline::CommandRuntimeV1,
            read: meerkat_core::confinement::FilesystemAccess::Paths(vec![
                meerkat_core::confinement::PathAccess::Subtree(root.clone()),
            ]),
            write: meerkat_core::confinement::FilesystemAccess::Paths(vec![
                meerkat_core::confinement::PathAccess::Subtree(root.clone()),
            ]),
            deny_read: vec![],
            deny_write: vec![],
            network: meerkat_core::confinement::IpNetworkAccess::Denied,
            unix_connect: vec![],
            require_descendant_termination: true,
        }
        .try_into()
        .unwrap();
    let router = McpRouter::new_with_surface_handle_and_stdio_profile(
        Arc::new(meerkat_runtime::RuntimeExternalToolSurfaceHandle::ephemeral()),
        crate::McpStdioLaunchProfile::required(requirement, root),
    )
    .with_attempt_guards(guards.clone(), session_owner());
    let adapter = Arc::new(McpRouterAdapter::new(router));
    add(&adapter, shell("confined", SERVE)).await;
    until_notice(&adapter, "confined", ExternalToolDeltaPhase::Failed).await;
    let (settled, disposition) = probe.one_finish("confined").await;
    assert!(!settled);
    assert_eq!(
        disposition.open,
        McpAttemptOpenOutcome::Refused {
            refusal: McpAttemptRefusal::Confinement
        }
    );
    assert!(matches!(
        disposition.process,
        McpAttemptProcessEnd::NotSpawned
    ));
    adapter.shutdown().await;
}

/// A spawn failure cancels as `SetupFailed{SpawnFailed}` in the launch
/// phase, nothing spawned.
#[tokio::test]
async fn a_spawn_failure_cancels_as_setup_failed_not_spawned() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    add(
        &adapter,
        McpServerConfig::stdio(
            "missing",
            "/nonexistent/never-run-attempt-guard",
            vec![],
            HashMap::new(),
        ),
    )
    .await;
    until_notice(&adapter, "missing", ExternalToolDeltaPhase::Failed).await;
    let (settled, disposition) = probe.one_finish("missing").await;
    assert!(!settled);
    let McpAttemptOpenOutcome::SetupFailed {
        failure: Some(failure),
    } = &disposition.open
    else {
        panic!("{disposition:?}");
    };
    assert_eq!(failure.class, McpStdioSetupFailureClass::SpawnFailed);
    assert_eq!(failure.phase, McpStdioSetupPhase::Launch);
    assert!(matches!(
        disposition.process,
        McpAttemptProcessEnd::NotSpawned
    ));
    adapter.shutdown().await;
}

/// r1 4 and r2 8: an initialize crash cancels with the attempt's own #230
/// failure (same attempt number, no tail) and its close receipt.
#[tokio::test]
async fn an_initialize_crash_cancels_with_its_setup_failure_and_close() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    add(
        &adapter,
        shell("crashes", "printf 'crash\\n' >&2\nexit 3\n"),
    )
    .await;
    until_notice(&adapter, "crashes", ExternalToolDeltaPhase::Failed).await;
    let (settled, disposition) = probe.one_finish("crashes").await;
    assert!(!settled);
    let diagnostics = adapter
        .stdio_setup_failure("crashes")
        .await
        .expect("the #230 failure");
    assert_eq!(disposition.key.attempt, diagnostics.attempt);
    let McpAttemptOpenOutcome::SetupFailed {
        failure: Some(failure),
    } = &disposition.open
    else {
        panic!("{disposition:?}");
    };
    assert_eq!(failure.attempt, diagnostics.attempt);
    assert_eq!(failure.class, diagnostics.class);
    assert_eq!(failure.phase, McpStdioSetupPhase::Initialize);
    assert!(
        !format!("{disposition:?}").contains("crash\\n"),
        "the disposition carries no stderr"
    );
    let receipt = closed_receipt(&disposition.process);
    assert!(matches!(
        receipt.leader,
        crate::McpStdioLeaderOutcome::Reaped(_)
    ));
    adapter.shutdown().await;
}

/// r1 3: an installed attempt settles only once its process was reaped by
/// a later remove, with the remove's own receipt.
#[tokio::test]
async fn an_installed_attempt_settles_after_its_remove_closes_it() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("installed", SERVE)).await;
    until_notice(&adapter, "installed", ExternalToolDeltaPhase::Applied).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        probe.finishes("installed").is_empty(),
        "no finish while the process is alive"
    );
    adapter
        .stage_remove("installed")
        .await
        .expect("stage remove");
    adapter.apply_staged().await.expect("apply remove");
    let (settled, disposition) = probe.one_finish("installed").await;
    assert!(settled, "only an installed attempt settles");
    assert!(matches!(
        disposition.open,
        McpAttemptOpenOutcome::Installed { .. }
    ));
    let receipt = closed_receipt(&disposition.process);
    assert!(matches!(
        receipt.leader,
        crate::McpStdioLeaderOutcome::Reaped(_)
    ));
    adapter.shutdown().await;
}

/// r1 6 (process held): shutdown settles an installed attempt with the very
/// receipt the shutdown report holds.
#[tokio::test]
async fn shutdown_settles_an_installed_attempt_with_the_reported_receipt() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("held", SERVE)).await;
    until_notice(&adapter, "held", ExternalToolDeltaPhase::Applied).await;
    let report = report_of(adapter.shutdown_with_report().await);
    let (settled, disposition) = probe.one_finish("held").await;
    assert!(settled);
    let receipt = closed_receipt(&disposition.process);
    let reported = report
        .servers()
        .iter()
        .find_map(|record| match &record.outcome {
            crate::McpServerCloseOutcome::Stdio(receipt) if record.server == "held" => {
                Some(Arc::clone(receipt))
            }
            _ => None,
        })
        .expect("the reported close");
    assert!(Arc::ptr_eq(&receipt, &reported), "the same receipt");
    assert!(report.guards_completed());
    assert!(
        report
            .guards()
            .iter()
            .any(|record| record.stage == McpGuardStage::Settle
                && record.completion == McpGuardCompletion::Completed)
    );
}

/// A success the router refuses to install (its lifecycle mirror rejects
/// the connected transition) cancels as `NotInstalled{InstallRefused}` with
/// its close.
#[tokio::test]
async fn an_install_refusal_cancels_as_not_installed() {
    struct RejectsConnected;
    impl meerkat_core::handles::McpServerLifecycleHandle for RejectsConnected {
        fn apply_connect_pending(&self, _: &str) -> Result<(), DslTransitionError> {
            Ok(())
        }
        fn apply_connected(&self, _: &str) -> Result<(), DslTransitionError> {
            Err(DslTransitionError::guard_rejected(
                "test",
                "refuses the install",
            ))
        }
        fn apply_failed(&self, _: &str, _: &str) -> Result<(), DslTransitionError> {
            Ok(())
        }
        fn apply_disconnected(&self, _: &str) -> Result<(), DslTransitionError> {
            Ok(())
        }
        fn apply_reload(&self, _: &str) -> Result<(), DslTransitionError> {
            Ok(())
        }
        fn pending_server_ids(&self) -> std::collections::BTreeSet<String> {
            std::collections::BTreeSet::new()
        }
    }
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let router = surface_router().with_attempt_guards(guards.clone(), session_owner());
    *router.mcp_lifecycle_handle_slot().write().unwrap() = Some(Arc::new(RejectsConnected));
    let adapter = Arc::new(McpRouterAdapter::new(router));
    add(&adapter, shell("refused-install", SERVE)).await;
    until_notice(&adapter, "refused-install", ExternalToolDeltaPhase::Failed).await;
    let (settled, disposition) = probe.one_finish("refused-install").await;
    assert!(!settled);
    assert_eq!(
        disposition.open,
        McpAttemptOpenOutcome::NotInstalled {
            reason: McpAttemptNotInstalled::InstallRefused
        }
    );
    closed_receipt(&disposition.process);
    adapter.shutdown().await;
}

/// r1 6 and r2 1: a shutdown while begin is pending does not drop it: the
/// attempt is cancelled as `RouterClosed`, nothing spawns, and the guard
/// the begin returns later gets exactly one cancel.
#[tokio::test]
async fn shutdown_during_begin_cancels_the_late_guard_once() {
    let probe = Probe::new();
    probe.hold_begins.send_replace(true);
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("pending", SERVE)).await;
    probe
        .until("the begin", |probe| !probe.begins("pending").is_empty())
        .await;
    adapter.shutdown().await;
    probe.hold_begins.send_replace(false);
    let (settled, disposition) = probe.one_finish("pending").await;
    assert!(!settled);
    assert_eq!(
        disposition.open,
        McpAttemptOpenOutcome::Cancelled {
            cause: McpAttemptCancelCause::RouterClosed
        }
    );
    assert!(matches!(
        disposition.process,
        McpAttemptProcessEnd::NotSpawned
    ));
    assert!(
        probe.calls().iter().any(
            |(_, call)| matches!(call, Call::BeginReturned { key } if key.server == "pending")
        ),
        "the begin ran to its end"
    );
}

/// r1 10 and r2 1: past its bound the attempt stops (`BeginBound`), nothing
/// spawns, and the late guard gets exactly one cancel.
#[tokio::test]
async fn a_begin_past_its_bound_is_cancelled_and_its_late_guard_cancelled_once() {
    let probe = Probe::new();
    probe.hold_begins.send_replace(true);
    let guards = installation(&probe, bounds(200, 2_000));
    let adapter = guarded(&guards, session_owner());
    let marker = tempfile::tempdir().unwrap();
    let marker_path = marker.path().join("spawned");
    add(
        &adapter,
        shell(
            "slow-begin",
            &format!("touch '{}'\n{SERVE}", marker_path.display()),
        ),
    )
    .await;
    until_notice(&adapter, "slow-begin", ExternalToolDeltaPhase::Failed).await;
    assert!(!marker_path.exists());
    probe.hold_begins.send_replace(false);
    let (settled, disposition) = probe.one_finish("slow-begin").await;
    assert!(!settled);
    assert_eq!(
        disposition.open,
        McpAttemptOpenOutcome::Cancelled {
            cause: McpAttemptCancelCause::BeginBound
        }
    );
    assert!(matches!(
        disposition.process,
        McpAttemptProcessEnd::NotSpawned
    ));
    adapter.shutdown().await;
}

/// r2 1: the close index stopping an attempt mid-begin cancels it; and an
/// adapter dropped mid-begin cancels it as `RouterDropped`. The late guards
/// are cancelled once, and `close_and_join` reports them.
#[tokio::test]
async fn a_dropped_adapter_mid_begin_cancels_and_close_and_join_reports_it() {
    let probe = Probe::new();
    probe.hold_begins.send_replace(true);
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("orphan", SERVE)).await;
    probe
        .until("the begin", |probe| !probe.begins("orphan").is_empty())
        .await;
    drop(adapter);
    probe.hold_begins.send_replace(false);
    let records = guards
        .close_and_join(tokio::time::Instant::now() + WAIT)
        .await;
    let (settled, disposition) = probe.one_finish("orphan").await;
    assert!(!settled);
    assert_eq!(
        disposition.open,
        McpAttemptOpenOutcome::Cancelled {
            cause: McpAttemptCancelCause::RouterDropped
        }
    );
    assert!(
        records
            .iter()
            .any(|record| record.key.server == "orphan" && record.stage == McpGuardStage::Begin),
        "{records:#?}"
    );
}

/// r1 7: dropping the adapter without shutdown abandons an installed
/// process (its group killed by the drop) and settles its guard with
/// `Abandoned`.
#[tokio::test]
async fn a_dropped_adapter_settles_an_installed_guard_as_abandoned() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("abandoned", SERVE)).await;
    until_notice(&adapter, "abandoned", ExternalToolDeltaPhase::Applied).await;
    drop(adapter);
    let (settled, disposition) = probe.one_finish("abandoned").await;
    assert!(settled, "an installed attempt settles");
    assert!(matches!(
        disposition.process,
        McpAttemptProcessEnd::Abandoned
    ));
    let _ = guards
        .close_and_join(tokio::time::Instant::now() + WAIT)
        .await;
}

/// r2 2: the next begin for the same owner and server waits for the prior
/// finish; past the settle bound it is refused (`PriorGuardUnsettled`, no
/// begin); the prior finish is recorded completed once released.
#[tokio::test]
async fn a_prior_unsettled_finish_refuses_the_next_begin() {
    let probe = Probe::new();
    probe.hold_finishes.send_replace(true);
    let guards = installation(&probe, bounds(5_000, 300));
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("serial", "exit 3\n")).await;
    until_notice(&adapter, "serial", ExternalToolDeltaPhase::Failed).await;
    probe
        .until("the first finish starts", |probe| {
            !probe.finishes("serial").is_empty()
        })
        .await;
    add(&adapter, shell("serial", SERVE)).await;
    until_notice(&adapter, "serial", ExternalToolDeltaPhase::Failed).await;
    // Give the second attempt time to be refused.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(probe.begins("serial").len(), 1, "no second begin");
    assert!(
        adapter.stdio_setup_failure("serial").await.is_none(),
        "the refused attempt leaves no setup failure, and released the first one's"
    );
    probe.hold_finishes.send_replace(false);
    let report = report_of(adapter.shutdown_with_report().await);
    let refused = report
        .guards()
        .iter()
        .find(|record| {
            record.disposition.as_ref().is_some_and(|disposition| {
                disposition.open
                    == McpAttemptOpenOutcome::Refused {
                        refusal: McpAttemptRefusal::PriorGuardUnsettled,
                    }
            })
        })
        .expect("the refused attempt is recorded");
    assert_eq!(refused.stage, McpGuardStage::Begin);
    let records = guards
        .close_and_join(tokio::time::Instant::now() + WAIT)
        .await;
    let all: Vec<_> = report.guards().iter().chain(records.iter()).collect();
    assert!(
        all.iter()
            .any(|record| record.stage == McpGuardStage::Cancel
                && record.completion == McpGuardCompletion::Completed),
        "{all:#?}"
    );
}

/// r2 3: a reload while installed begins promptly (no wait on a live
/// guard); the host refuses it, the reload fails, and the installed
/// attempt settles after its later close. A wait on the live guard would
/// last the whole settle bound and then refuse the reload unbegun.
#[tokio::test]
async fn a_reload_never_waits_for_a_live_guard() {
    let probe = Probe::new();
    let guards = installation(&probe, bounds(5_000, 5_000));
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("live", SERVE)).await;
    until_notice(&adapter, "live", ExternalToolDeltaPhase::Applied).await;
    probe.refuse.lock().unwrap().insert("live".to_string());
    adapter
        .stage_reload(crate::McpReloadTarget::ServerName("live".to_string()))
        .await
        .expect("stage reload");
    let started = tokio::time::Instant::now();
    adapter.apply_staged().await.expect("apply");
    probe
        .until("the second begin", |probe| probe.begins("live").len() == 2)
        .await;
    assert!(
        started.elapsed() < Duration::from_millis(2_500),
        "the begin did not wait for the live guard"
    );
    until_notice(&adapter, "live", ExternalToolDeltaPhase::Failed).await;
    assert!(
        probe.finishes("live").is_empty(),
        "the installed guard is live"
    );
    adapter.shutdown().await;
    let (settled, disposition) = probe.one_finish("live").await;
    assert!(settled);
    closed_receipt(&disposition.process);
}

/// r2 4: a rebuilt construction for the same owner awaits its
/// predecessor's running finish before beginning.
#[tokio::test]
async fn a_rebuild_awaits_the_predecessor_finish() {
    let probe = Probe::new();
    probe.hold_finishes.send_replace(true);
    let guards = installation(&probe, bounds(5_000, 5_000));
    let owner = session_owner();
    let first = guarded(&guards, owner.clone());
    add(&first, shell("rebuilt", "exit 3\n")).await;
    until_notice(&first, "rebuilt", ExternalToolDeltaPhase::Failed).await;
    probe
        .until("the predecessor finish starts", |probe| {
            !probe.finishes("rebuilt").is_empty()
        })
        .await;
    let second = guarded(&guards, owner);
    assert_ne!(first.construction_id(), second.construction_id());
    add(&second, shell("rebuilt", SERVE)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(probe.begins("rebuilt").len(), 1, "the rebuild waits");
    probe.hold_finishes.send_replace(false);
    probe
        .until("the rebuild begins", |probe| {
            probe.begins("rebuilt").len() == 2
        })
        .await;
    first.shutdown().await;
    second.shutdown().await;
}

/// r2 5: a shutdown awaits finishes by the settle bound; a slow one is
/// reported `TimedOut` (settled closes unchanged), and `close_and_join`
/// later reports it completed.
#[tokio::test]
async fn a_slow_finish_times_out_at_shutdown_and_completes_at_join() {
    let probe = Probe::new();
    probe.hold_finishes.send_replace(true);
    let guards = installation(&probe, bounds(5_000, 300));
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("slow", SERVE)).await;
    until_notice(&adapter, "slow", ExternalToolDeltaPhase::Applied).await;
    let report = report_of(adapter.shutdown_with_report().await);
    assert!(report.is_settled(), "the close settled");
    assert!(!report.guards_completed());
    assert!(
        report
            .guards()
            .iter()
            .any(|record| record.stage == McpGuardStage::Settle
                && record.completion == McpGuardCompletion::TimedOut)
    );
    probe.hold_finishes.send_replace(false);
    let records = guards
        .close_and_join(tokio::time::Instant::now() + WAIT)
        .await;
    assert!(
        records
            .iter()
            .any(|record| record.stage == McpGuardStage::Settle
                && record.completion == McpGuardCompletion::Completed),
        "{records:#?}"
    );
}

/// r2 7: a finish that returns `Err` is `Failed`, one that panics is
/// `Panicked`.
#[tokio::test]
async fn failed_and_panicked_finishes_are_recorded() {
    for (finish, expected) in [
        (Finish::Err, McpGuardCompletion::Failed),
        (Finish::Panic, McpGuardCompletion::Panicked),
    ] {
        let probe = Probe::new();
        *probe.finish.lock().unwrap() = finish;
        let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
        let adapter = guarded(&guards, session_owner());
        add(&adapter, shell("finishing", "exit 3\n")).await;
        until_notice(&adapter, "finishing", ExternalToolDeltaPhase::Failed).await;
        probe.one_finish("finishing").await;
        let report = report_of(adapter.shutdown_with_report().await);
        assert!(
            report.guards().iter().any(
                |record| record.stage == McpGuardStage::Cancel && record.completion == expected
            ),
            "{:#?}",
            report.guards()
        );
    }
}

/// r1 9 and r2 7: a shutdown racing a remove finishes the guard exactly
/// once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shutdown_racing_a_remove_finishes_exactly_once() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("raced-close", SERVE)).await;
    until_notice(&adapter, "raced-close", ExternalToolDeltaPhase::Applied).await;
    let remover = {
        let adapter = Arc::clone(&adapter);
        tokio::spawn(async move {
            if adapter.stage_remove("raced-close").await.is_ok() {
                let _ = adapter.apply_staged().await;
            }
        })
    };
    adapter.shutdown().await;
    let _ = remover.await;
    let (settled, _) = probe.one_finish("raced-close").await;
    assert!(settled);
}

/// r1 13: attempt numbers are the #230 ones, and a guard refusal leaves no
/// #230 failure.
#[tokio::test]
async fn attempt_numbers_match_the_setup_diagnostics() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("numbered", "exit 3\n")).await;
    until_notice(&adapter, "numbered", ExternalToolDeltaPhase::Failed).await;
    let failure = adapter.stdio_setup_failure("numbered").await.expect("#230");
    assert_eq!(probe.begins("numbered")[0].attempt, failure.attempt);
    adapter.shutdown().await;
}

/// The guard scope is stdio: an HTTP attempt on a guarded router never
/// begins a guard, and is neither guarded nor refused by it.
#[tokio::test]
async fn an_http_attempt_never_begins_a_guard() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    let config = timeout(
        McpServerConfig::streamable_http("remote", "http://127.0.0.1:9/mcp", HashMap::new()),
        2,
    );
    add(&adapter, config).await;
    until_notice(&adapter, "remote", ExternalToolDeltaPhase::Failed).await;
    assert!(probe.begins("remote").is_empty());
    adapter.shutdown().await;
}

/// r1 14: without installed guards nothing is begun or reported.
#[tokio::test]
async fn without_guards_nothing_is_reported() {
    let adapter = Arc::new(McpRouterAdapter::new(surface_router()));
    add(&adapter, shell("plain", SERVE)).await;
    until_notice(&adapter, "plain", ExternalToolDeltaPhase::Applied).await;
    let report = report_of(adapter.shutdown_with_report().await);
    assert!(report.guards().is_empty());
    assert!(report.guards_completed());
}

/// Bounds over the maximum are refused, not clamped, and so are zero bounds
/// (a zero begin bound would cancel every stdio attempt).
#[test]
fn bounds_over_the_maximum_are_refused() {
    let second = Duration::from_secs(1);
    for (begin, cancel, settle) in [
        (Duration::ZERO, second, second),
        (second, Duration::ZERO, second),
        (second, second, Duration::ZERO),
    ] {
        assert!(McpAttemptGuardBounds::new(begin, cancel, settle).is_err());
    }
    assert!(
        McpAttemptGuardBounds::new(
            McpAttemptGuardBounds::MAX + Duration::from_millis(1),
            Duration::ZERO,
            Duration::ZERO
        )
        .is_err()
    );
    assert!(
        McpAttemptGuardBounds::new(
            McpAttemptGuardBounds::MAX,
            McpAttemptGuardBounds::MAX,
            McpAttemptGuardBounds::MAX
        )
        .is_ok()
    );
}

/// M1: a close of the installation while an attempt still awaits its prior
/// guard's finish refuses that attempt's begin: the factory is never asked,
/// and the close reports the begin `Cancelled{InstallationClosed}`.
#[tokio::test]
async fn close_and_join_refuses_a_begin_still_awaiting_its_prior_finish() {
    let probe = Probe::new();
    probe.hold_finishes.send_replace(true);
    let guards = installation(&probe, bounds(5_000, 3_000));
    let adapter = guarded(&guards, session_owner());
    add(&adapter, shell("waiting", "exit 3\n")).await;
    until_notice(&adapter, "waiting", ExternalToolDeltaPhase::Failed).await;
    probe
        .until("the first finish starts", |probe| {
            !probe.finishes("waiting").is_empty()
        })
        .await;
    let first = probe.begins("waiting")[0].clone();
    // The second attempt awaits the first one's running finish.
    add(&adapter, shell("waiting", SERVE)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let closing = {
        let guards = guards.clone();
        tokio::spawn(async move {
            guards
                .close_and_join(tokio::time::Instant::now() + WAIT)
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    probe.hold_finishes.send_replace(false);
    let records = closing.await.unwrap();
    // A begin started after the close would show by now.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        probe.begins("waiting"),
        vec![first.clone()],
        "no begin after the installation closed"
    );
    assert!(
        records.iter().any(|record| record.key.server == "waiting"
            && record.key.attempt != first.attempt
            && record.stage == McpGuardStage::Begin
            && record.disposition.as_ref().is_some_and(|disposition| {
                disposition.open
                    == McpAttemptOpenOutcome::Cancelled {
                        cause: McpAttemptCancelCause::InstallationClosed,
                    }
            })),
        "the close reports the refused begin: {records:#?}"
    );
    assert!(
        records.iter().any(|record| record.key == first
            && record.stage == McpGuardStage::Cancel
            && record.completion == McpGuardCompletion::Completed),
        "{records:#?}"
    );
    adapter.shutdown().await;
}

/// M1: a finish that starts while `close_and_join` waits for another one is
/// reported too, and no shutdown reports the guards completed while it runs.
#[tokio::test]
async fn close_and_join_reports_a_finish_that_starts_while_it_waits() {
    let probe = Probe::new();
    probe.hold_servers.send_modify(|held| {
        held.insert("first".to_string());
        held.insert("second".to_string());
    });
    let guards = installation(&probe, bounds(5_000, 300));
    let adapter = guarded(&guards, session_owner());
    for name in ["first", "second"] {
        add(&adapter, shell(name, SERVE)).await;
        until_notice(&adapter, name, ExternalToolDeltaPhase::Applied).await;
    }
    remove(&adapter, "first").await;
    probe
        .until("the first finish starts", |probe| {
            !probe.finishes("first").is_empty()
        })
        .await;
    let closing = {
        let guards = guards.clone();
        tokio::spawn(async move {
            guards
                .close_and_join(tokio::time::Instant::now() + Duration::from_secs(3))
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    // This finish starts while the close waits for the first one.
    remove(&adapter, "second").await;
    probe
        .until("the second finish starts", |probe| {
            !probe.finishes("second").is_empty()
        })
        .await;
    probe.hold_servers.send_modify(|held| {
        held.remove("first");
    });
    let records = closing.await.unwrap();
    assert!(
        records
            .iter()
            .any(|record| record.key.server == "second" && record.stage == McpGuardStage::Settle),
        "the finish that started while the close waited is reported: {records:#?}"
    );
    let report = report_of(adapter.shutdown_with_report().await);
    assert!(
        !report.guards_completed(),
        "a finish still runs: {:#?}",
        report.guards()
    );
    probe.hold_servers.send_modify(|held| {
        held.remove("second");
    });
    let records = guards
        .close_and_join(tokio::time::Instant::now() + WAIT)
        .await;
    assert!(
        records.iter().any(|record| record.key.server == "second"
            && record.stage == McpGuardStage::Settle
            && record.completion == McpGuardCompletion::Completed),
        "a later close reports its end: {records:#?}"
    );
}

/// L1: the immediate add path keeps a guard refusal typed.
#[tokio::test]
async fn add_server_keeps_a_guard_refusal_typed() {
    let probe = Probe::new();
    probe.refuse.lock().unwrap().insert("immediate".to_string());
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let mut router = surface_router().with_attempt_guards(guards, session_owner());
    let added = router.add_server(shell("immediate", SERVE)).await;
    assert!(
        matches!(
            added,
            Err(crate::McpError::AttachmentRefused(
                McpAttachmentRefusal::Denied
            ))
        ),
        "{added:?}"
    );
    assert_eq!(probe.begins("immediate").len(), 1);
    router.shutdown().await;
}

/// L3: an attempt dropped while it waits for its begin cancels the guard the
/// begin returns later as `AttemptDropped`, nothing spawned.
#[tokio::test]
async fn a_dropped_attempt_cancels_its_late_guard_as_attempt_dropped() {
    let probe = Probe::new();
    probe.hold_begins.send_replace(true);
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let mut router = surface_router().with_attempt_guards(guards, session_owner());
    let added = tokio::time::timeout(
        Duration::from_millis(300),
        router.add_server(shell("dropped", SERVE)),
    )
    .await;
    assert!(added.is_err(), "the add still waited for its begin");
    probe.hold_begins.send_replace(false);
    let (settled, disposition) = probe.one_finish("dropped").await;
    assert!(!settled);
    assert_eq!(
        disposition.open,
        McpAttemptOpenOutcome::Cancelled {
            cause: McpAttemptCancelCause::AttemptDropped
        }
    );
    assert!(matches!(
        disposition.process,
        McpAttemptProcessEnd::NotSpawned
    ));
    router.shutdown().await;
}

/// L3: a result the router had not processed when its shutdown began is
/// cancelled `RouterClosed`, with its process's close.
#[tokio::test]
async fn shutdown_cancels_an_unprocessed_result_as_router_closed() {
    let probe = Probe::new();
    let guards = installation(&probe, McpAttemptGuardBounds::DEFAULT);
    let adapter = guarded(&guards, session_owner());
    let marker = tempfile::tempdir().unwrap();
    let started = marker.path().join("started");
    add(
        &adapter,
        shell(
            "unprocessed",
            &format!("touch '{}'\n{SERVE}", started.display()),
        ),
    )
    .await;
    tokio::time::timeout(WAIT, async {
        while !started.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the server process started");
    // Nothing drives the router: its result waits, unprocessed.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let report = report_of(adapter.shutdown_with_report().await);
    let (settled, disposition) = probe.one_finish("unprocessed").await;
    assert!(
        !settled,
        "a result first processed by the shutdown is cancelled"
    );
    assert_eq!(
        disposition.open,
        McpAttemptOpenOutcome::Cancelled {
            cause: McpAttemptCancelCause::RouterClosed
        }
    );
    closed_receipt(&disposition.process);
    assert!(report.guards_completed(), "{:#?}", report.guards());
}

/// N3: a second installation on one router is refused: the first stands.
#[tokio::test]
async fn a_second_guard_installation_is_refused() {
    let first = Probe::new();
    let second = Probe::new();
    let router = surface_router()
        .with_attempt_guards(
            installation(&first, McpAttemptGuardBounds::DEFAULT),
            session_owner(),
        )
        .with_attempt_guards(
            installation(&second, McpAttemptGuardBounds::DEFAULT),
            session_owner(),
        );
    let adapter = Arc::new(McpRouterAdapter::new(router));
    add(&adapter, shell("twice", SERVE)).await;
    until_notice(&adapter, "twice", ExternalToolDeltaPhase::Applied).await;
    assert_eq!(
        first.begins("twice").len(),
        1,
        "the first installation stands"
    );
    assert!(second.begins("twice").is_empty());
    adapter.shutdown().await;
    let (settled, _) = first.one_finish("twice").await;
    assert!(settled);
}
