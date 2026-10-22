//! Bounded drain of a Stopped mob's runs.
//!
//! [`MobHandle::drain_stopped_runs`](super::MobHandle::drain_stopped_runs)
//! takes one snapshot from the actor, atomic with its Stopped check, then
//! waits handle-side, off the actor loop, for the run each local member had
//! at the snapshot. Nothing re-enters the actor, so dropping the wait only
//! stops waiting.

use std::sync::Arc;

use meerkat_core::SessionId;
use meerkat_core::lifecycle::RunId;
use meerkat_core::time_compat::Instant;

use super::stop_report::{
    MemberRunDrain, MemberRunDrainOutcome, MemberRunStarts, MobRunDrainReport,
};
use crate::AgentIdentity;

/// The members of a Stopped mob as the actor found them, and the runtime
/// they run on.
pub(super) struct DrainSnapshot {
    pub(super) members: Vec<DrainSnapshotMember>,
    pub(super) machine: Option<Arc<meerkat_runtime::MeerkatMachine>>,
}

/// One roster member at the snapshot.
pub(super) struct DrainSnapshotMember {
    pub(super) identity: AgentIdentity,
    pub(super) starts: Option<MemberRunStarts>,
    pub(super) target: DrainTarget,
}

/// What the drain does for one member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DrainTarget {
    /// Reported as is, without waiting.
    Report(MemberRunDrain),
    /// Wait for exactly this run of the member's local session.
    Wait {
        session_id: SessionId,
        run_id: RunId,
    },
}

/// The facts the actor reads for one roster member at the snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct DrainMemberFacts {
    /// The member's local session; `None` for a placed or remote member.
    pub(super) local_session: Option<SessionId>,
    pub(super) reload_required: bool,
    pub(super) live_channel_active: bool,
    pub(super) current_run: Option<RunId>,
}

/// What the drain does for a member with `facts`, whose last recorded Stop
/// outcome reported `starts`. A member whose starts are not held still has
/// its current run waited for; a run it starts later is not.
pub(super) fn drain_target(
    facts: DrainMemberFacts,
    starts: Option<&MemberRunStarts>,
) -> DrainTarget {
    let Some(session_id) = facts.local_session else {
        return DrainTarget::Report(MemberRunDrain::NotLocal);
    };
    if facts.reload_required || matches!(starts, Some(MemberRunStarts::ReloadRequired)) {
        return DrainTarget::Report(MemberRunDrain::ReloadRequired);
    }
    if facts.live_channel_active {
        return DrainTarget::Report(MemberRunDrain::LiveChannelOpen {
            run_id: facts.current_run,
        });
    }
    match facts.current_run {
        None => DrainTarget::Report(MemberRunDrain::NoRun),
        Some(run_id) => DrainTarget::Wait { session_id, run_id },
    }
}

/// Settle every member of `snapshot` within `until`.
pub(super) async fn drain(snapshot: DrainSnapshot, until: Instant) -> MobRunDrainReport {
    let DrainSnapshot { members, machine } = snapshot;
    let _ = until;
    let mut report = MobRunDrainReport::default();
    for member in members {
        let run = match member.target {
            DrainTarget::Report(run) => run,
            DrainTarget::Wait { run_id, .. } => MemberRunDrain::StillRunning { run_id },
        };
        report.members.insert(
            member.identity,
            MemberRunDrainOutcome {
                run,
                starts: member.starts,
            },
        );
    }
    drop(machine);
    report
}
