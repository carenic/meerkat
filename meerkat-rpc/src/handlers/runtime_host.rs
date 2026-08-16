//! Handlers for `runtime/*` host projection methods.

use std::sync::Arc;

use meerkat_core::ConfigStore;

use crate::protocol::{RpcId, RpcResponse};
use crate::session_runtime::SessionRuntime;

fn host_surface_options(
    runtime_available: bool,
    live_enabled: bool,
    live_webrtc_enabled: bool,
    event_replay: bool,
    approvals_available: bool,
    skills_enabled: bool,
) -> meerkat::surface::RuntimeHostSurfaceOptions {
    let catalog_options = meerkat_contracts::RpcMethodCatalogOptions {
        runtime_available,
        live_enabled,
        mob_enabled: cfg!(feature = "mob"),
        mcp_enabled: cfg!(feature = "mcp"),
        comms_enabled: cfg!(feature = "comms"),
        blob_enabled: true,
        session_events_enabled: true,
        session_streams_enabled: true,
        schedule_enabled: cfg!(feature = "schedule"),
        workgraph_enabled: cfg!(feature = "workgraph"),
        skills_enabled,
        live_webrtc_enabled,
    };
    let mut options = meerkat::surface::RuntimeHostSurfaceOptions::process(
        "meerkat-rpc",
        env!("CARGO_PKG_VERSION"),
    );
    options.runtime_backed_sessions = runtime_available;
    options.mobs = cfg!(feature = "mob");
    options.multi_host_mobs = cfg!(feature = "mob");
    options.mcp_live = cfg!(feature = "mcp");
    options.comms = cfg!(feature = "comms");
    options.blobs = true;
    options.artifacts = true;
    options.session_events = true;
    options.event_replay = event_replay;
    options.session_streams = true;
    options.schedules = cfg!(feature = "schedule");
    options.skills = skills_enabled;
    options.approvals = approvals_available;
    options.durable_jobs = true;
    options.rpc_transport = Some("json_rpc".to_string());
    options.rpc_methods = meerkat_contracts::rpc_method_names(catalog_options);
    options
}

pub async fn handle_info(
    id: Option<RpcId>,
    runtime: &Arc<SessionRuntime>,
    config_store: &Arc<dyn ConfigStore>,
    runtime_available: bool,
    live_enabled: bool,
    live_webrtc_enabled: bool,
    skills_enabled: bool,
) -> RpcResponse {
    let options = host_surface_options(
        runtime_available,
        live_enabled,
        live_webrtc_enabled,
        runtime.supports_event_replay(),
        runtime.approval_service().is_persistent(),
        skills_enabled,
    );
    let (context_root, _) = runtime.skill_identity_roots();
    let metadata = config_store.metadata();
    let metadata = metadata
        .as_ref()
        .map(meerkat::surface::RuntimeHostMetadataProjection::from);
    let mut info =
        meerkat::surface::build_runtime_host_info(&options, metadata.as_ref(), context_root);
    info.health = runtime_health(runtime).await;
    RpcResponse::success(id, &info)
}

pub fn handle_capabilities(
    id: Option<RpcId>,
    runtime: &Arc<SessionRuntime>,
    runtime_available: bool,
    live_enabled: bool,
    live_webrtc_enabled: bool,
    skills_enabled: bool,
) -> RpcResponse {
    let options = host_surface_options(
        runtime_available,
        live_enabled,
        live_webrtc_enabled,
        runtime.supports_event_replay(),
        runtime.approval_service().is_persistent(),
        skills_enabled,
    );
    let capabilities = meerkat::surface::build_runtime_host_capabilities(&options);
    RpcResponse::success(id, &capabilities)
}

pub async fn handle_health(id: Option<RpcId>, runtime: &Arc<SessionRuntime>) -> RpcResponse {
    let health = runtime_health(runtime).await;
    RpcResponse::success(id, &health)
}

/// Probe the runtime dimensions this surface can probe, and say what each probe
/// established.
///
/// **The contract for anyone adding a check here: assert only what you
/// observed.** `status` is not this handler's opinion of the process; it is a
/// rollup that [`meerkat::surface::build_runtime_host_health_from_observations`]
/// computes over the observations handed in. Hand in
/// [`meerkat::surface::RuntimeHealthObservation::Measured`] and the dimension is
/// covered and rolled up at the rung you saw; hand in
/// [`meerkat::surface::RuntimeHealthObservation::Unreadable`] and it is
/// published as `unreadable:<dimension>` and rolled up at `Degraded`; hand in
/// nothing at all and it is published as `unmeasured:<dimension>` and left out
/// of the rollup. Never mint a `Measured` rung for a dimension you did not
/// actually read, and never overwrite `status` after the fact.
///
/// The consequence worth internalizing: because `status` speaks for the
/// attempted set, **a check key you add here is a promise an operator can page
/// on**. A probe that can report a fault it did not observe turns this endpoint
/// back into noise, which is why every observation below is either a fact about
/// a session or an explicit `Unreadable`.
///
/// ## Covered
///
/// - `jobs` - detached-job service health for the active realm (or process-wide
///   when no realm is bound) plus the in-process delivery backlog. `Degraded`
///   when a snapshot that was read says the service is degraded, or a backlog
///   that was read is non-empty.
/// - `session_durability` - registered sessions whose shared durability gate
///   demands a cold reload before they may execute or mutate durable state.
///   `Degraded` when any session is in that state. A storeless session has no
///   durability contract and is never counted.
/// - `session_runtime_loop` - registered sessions still claiming a runtime loop
///   whose task is gone or whose channels are closed. `Degraded` when any
///   session is in that state.
/// - `session_run_start` - registered sessions holding a staged run that is
///   overdue to begin executing: staged past the watchdog's notice bound while
///   machine authority still shows the run current with its primitive
///   un-applied. `Degraded` when any session is in that state. The verdict is
///   recomputed from machine truth per scrape - the same classification the
///   staged-run watchdog logs - so this key and that log line cannot disagree.
///   Runs whose execution start is honestly unobservable are never counted.
///
/// **Every one of the four may instead answer "I could not look", and every
/// one of the four reports that as `unreadable:<dimension>` rather than as a
/// `Measured` rung.** For the session probes the failed read is an unreadable
/// session registry - and for `session_run_start` also a past-bound window
/// whose machine authority could not be read without blocking, the holder of
/// which is the prime suspect for the wedge itself; for `jobs` it is a
/// job-service snapshot or a delivery backlog that returned an error. This is the same rule the paragraph above
/// states, applied to this function's own checks: a `jobs: degraded` published
/// off a failed snapshot read would be asserting a specific and actionable
/// fault - the job service is in trouble - that nobody observed, and it is a
/// key an operator would page on.
///
/// The `unreadable:*` class does reach `status`, because a scrape that could
/// not look at a dimension has not established that the dimension is healthy.
/// It is deliberately *not* the same key as an unbuilt dimension: this one is
/// per-scrape and clears itself on the next successful read, so an alert window
/// absorbs a blip while a dimension that stays unreadable across scrapes is
/// itself worth paging on. Note that the rung is identical either way -
/// `Unreadable` folds in at `Degraded` - so this distinction never changes
/// `status`; it changes only which key in `checks` carries the claim, which is
/// the difference between telling an operator where to look and sending them
/// after a fault that does not exist.
///
/// ## Not covered (published as `unmeasured:*`, never as healthy)
///
/// - `session_liveness` - nothing here observes the PRE-staging class: a
///   session whose loop task is alive and whose channels are open, but which
///   is parked while machine-owned lane truth still holds selectable queued
///   work that never gets staged at all. `session_run_start` does not cover
///   it and cannot: the staged-run window opens at the durable `StageForRun`
///   commit, so work that never reaches staging never opens a window. (0.8.23
///   claimed this class needed "a watchdog bridge"; that was wrong - the
///   watchdog is also downstream of staging. It needs a lane-truth probe that
///   does not exist yet.) No probe exists, so this one stays out of the
///   rollup: a permanent amber light is a muted alarm.
async fn runtime_health(runtime: &SessionRuntime) -> meerkat_contracts::RuntimeHostHealth {
    let observed_at_ms = meerkat_core::time_compat::SystemTime::now()
        .duration_since(meerkat_core::time_compat::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(u64::MAX);
    let service = runtime.detached_job_service();
    let job_snapshot = match runtime.realm_id() {
        Some(realm_id) => {
            service
                .health_snapshot_for_realm(realm_id.as_str(), observed_at_ms, 10_000)
                .await
        }
        None => service.health_snapshot(observed_at_ms, 10_000).await,
    };
    // A snapshot this probe could not read is not a degraded job service. The
    // two sub-reads are folded worst-wins, but a failure of EITHER read makes
    // the dimension unreadable rather than degraded: only a reading that came
    // back may publish a rung under the plain `jobs` key.
    let jobs = match job_snapshot {
        Err(_) => meerkat::surface::RuntimeHealthObservation::Unreadable,
        Ok(snapshot) => match runtime.runtime_job_delivery_backlog().await {
            Err(_) => meerkat::surface::RuntimeHealthObservation::Unreadable,
            Ok(backlog) => meerkat::surface::RuntimeHealthObservation::Measured(
                if snapshot.is_degraded() || backlog != 0 {
                    meerkat_contracts::RuntimeHostHealthStatus::Degraded
                } else {
                    meerkat_contracts::RuntimeHostHealthStatus::Ok
                },
            ),
        },
    };
    // All four probes are attempted unconditionally, so every dimension this
    // handler owns is in the attempted set on every scrape. The only thing that
    // varies is whether the probe came back with a reading.
    let observations = vec![
        ("jobs".to_string(), jobs),
        (
            "session_durability".to_string(),
            observed_session_population(runtime.reload_required_session_count()),
        ),
        (
            "session_runtime_loop".to_string(),
            observed_session_population(runtime.dead_runtime_loop_session_count()),
        ),
        (
            "session_run_start".to_string(),
            observed_session_population(runtime.overdue_run_start_session_count()),
        ),
    ];
    meerkat::surface::build_runtime_host_health_from_observations(observations)
}

/// A session-population probe's answer, as a health observation.
///
/// `None` from the probe means the session registry was not readable, not that
/// the count was zero, and the two must not share a representation: reporting
/// it as `Measured(Ok)` would publish a health claim nobody established, and
/// reporting it as `Measured(Degraded)` would assert a specific and actionable
/// fault - some named population of sessions is in a bad state - that nobody
/// observed either. `Unreadable` says the only true thing available: the probe
/// ran and came back without a reading.
///
/// `n > 0` is `Degraded` rather than `Unhealthy`: both conditions this maps are
/// per session and fenced, so the host keeps serving every session that is not
/// affected. `Unhealthy` is reserved for a fault of the host itself.
fn observed_session_population(count: Option<usize>) -> meerkat::surface::RuntimeHealthObservation {
    match count {
        Some(0) => meerkat::surface::RuntimeHealthObservation::Measured(
            meerkat_contracts::RuntimeHostHealthStatus::Ok,
        ),
        Some(_) => meerkat::surface::RuntimeHealthObservation::Measured(
            meerkat_contracts::RuntimeHostHealthStatus::Degraded,
        ),
        None => meerkat::surface::RuntimeHealthObservation::Unreadable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meerkat::surface::RuntimeHealthObservation;
    use meerkat_contracts::RuntimeHostHealthStatus;

    /// The single mapping that makes `runtime/health` honest about its two
    /// session dimensions, pinned arm by arm.
    ///
    /// The `None` arm is the load-bearing one and the reason this module
    /// exists. Every other assertion in the tree exercises a host whose session
    /// registry answers, so `None` is reachable in production and unreachable
    /// in the endpoint-level fixtures: changing it to `Measured(Ok)` would
    /// republish the exact defect this lane was opened for - a health payload
    /// reporting `status: ok` on the strength of dimensions it never read -
    /// and, without this test, would do so with the whole suite green.
    ///
    /// The expectations are literals rather than values re-derived from the
    /// function under test, so each arm has to be independently right.
    #[test]
    fn a_session_population_probe_that_could_not_read_is_never_reported_as_measured() {
        assert_eq!(
            observed_session_population(None),
            RuntimeHealthObservation::Unreadable,
            "an unreadable session registry is not a reading; publishing it as \
             `Measured(Ok)` mints a health claim nobody established, and as \
             `Measured(Degraded)` asserts a named population of bad sessions \
             nobody observed"
        );
        assert_eq!(
            observed_session_population(Some(0)),
            RuntimeHealthObservation::Measured(RuntimeHostHealthStatus::Ok),
            "a probe that read the registry and found nobody affected has \
             earned its `ok`"
        );
        for count in [1usize, 2, 97] {
            assert_eq!(
                observed_session_population(Some(count)),
                RuntimeHealthObservation::Measured(RuntimeHostHealthStatus::Degraded),
                "{count} affected session(s) is a measured fault, and it is \
                 `Degraded` rather than `Unhealthy` because both conditions \
                 this maps are per session and fenced: the host keeps serving \
                 everything else"
            );
        }
    }
}
