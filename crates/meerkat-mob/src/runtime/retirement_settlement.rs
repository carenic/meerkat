//! Typed settlement of a member retirement, and the per-member outcome of a
//! mob Shutdown.
//!
//! A retirement that has durably started (its `MemberRetirementStarted` fact
//! is journaled) always has an owner that drives it to a terminal state. The
//! caller of [`MobHandle::retire`](super::MobHandle::retire) may stop waiting;
//! the retirement does not stop. Its progress and terminal outcome are
//! published on a per-member watch that any handle can observe through
//! [`MobHandle::retirement_settlement`](super::MobHandle::retirement_settlement).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Weak};

use meerkat_core::time_compat::{Duration, Instant};

use crate::error::MobError;
use crate::ids::{AgentIdentity, Generation};

/// The retirement stage a member is in, named by the stage's effect.
///
/// Stage names are stable diagnostics, not a protocol: compare a stage with
/// [`RetirementStage::as_str`] only for display or logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RetirementStage(&'static str);

impl RetirementStage {
    /// The stage before the actor has dispatched any effect.
    pub const ADMISSION: Self = Self("retire-admission");

    pub(crate) const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// The stage's stable name, for display and logging.
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for RetirementStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Where a member's retirement stands.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum RetirementSettlement {
    /// The retirement is running and owned by the mob actor.
    InProgress { stage: RetirementStage },
    /// The member is retired: its terminal fact is journaled and its roster
    /// entry removed.
    Retired,
    /// The retirement durably started, then a stage failed (or never received
    /// the signal it waits on within the lifecycle hang guard). The member
    /// stays `Retiring`, owned by the mob actor's stuck-retirement registry;
    /// [`MobHandle::redrive_retirement`](super::MobHandle::redrive_retirement)
    /// or a mob resume drives it again.
    Stuck {
        stage: RetirementStage,
        cause: Arc<MobError>,
    },
    /// The retirement failed before it durably started; the member is
    /// unchanged.
    NotStarted { cause: Arc<MobError> },
}

impl RetirementSettlement {
    /// Whether this is a terminal outcome rather than progress.
    pub fn is_settled(&self) -> bool {
        !matches!(self, Self::InProgress { .. })
    }
}

/// Observer of one retirement of one member incarnation.
///
/// Each retirement of an incarnation publishes on its own channel. A terminal
/// value is final for that channel: a re-drive of a `Stuck` retirement is a
/// new retirement on a new channel, which
/// [`MobHandle::retirement_settlement`](super::MobHandle::retirement_settlement)
/// returns from then on. The terminal value of the latest retirement stays
/// readable (a tombstone) until the identity's next retirement, so "retired
/// and gone" is distinguishable from "never retired".
#[derive(Debug, Clone)]
pub struct RetirementSettlementWatch {
    generation: Generation,
    rx: crate::tokio::sync::watch::Receiver<RetirementSettlement>,
}

impl RetirementSettlementWatch {
    /// The member incarnation (roster generation) this retirement belongs to.
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// The settlement as currently published.
    pub fn current(&self) -> RetirementSettlement {
        self.rx.borrow().clone()
    }

    /// Wait until this retirement reaches a terminal outcome and return it:
    /// `Retired`, `Stuck` or `NotStarted`, never `InProgress`.
    ///
    /// Every terminal outcome is published before the actor releases the
    /// retirement, so this resolves on the actor's own transition. `None`
    /// means the actor exited with this retirement still in progress (a
    /// durably started one resumes on the next start).
    pub async fn settled(&mut self) -> Option<RetirementSettlement> {
        self.rx
            .wait_for(RetirementSettlement::is_settled)
            .await
            .ok()
            .map(|settlement| settlement.clone())
    }
}

/// How the mob's actor task ended, published once by the task's spawn
/// wrapper after the actor future returned or was dropped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ActorTaskEnd {
    #[default]
    Running,
    /// The actor future returned: the actor, and every store handle it
    /// owned, has dropped.
    Returned,
    /// The task ended without its future returning (a panic or an abort).
    Lost,
}

/// Member lifecycle observations written only by the mob actor (and its
/// task's spawn wrapper) and shared read-only with its handles: per-member
/// retirement settlements, the report of the most recent Shutdown, and how
/// the actor task ended.
#[derive(Debug, Default)]
pub(crate) struct MemberLifecycleObservations {
    members: std::sync::Mutex<BTreeMap<AgentIdentity, SettlementChannel>>,
    last_shutdown_report: std::sync::Mutex<Option<MobShutdownReport>>,
    actor_task: crate::tokio::sync::watch::Sender<ActorTaskEnd>,
    /// Test probe: set once a Shutdown parks on its off-actor teardown.
    #[cfg(test)]
    pub(crate) shutdown_teardown_parked: crate::tokio::sync::watch::Sender<bool>,
    /// Test probe: set once a Shutdown passes its admission, before its
    /// lifecycle drains.
    #[cfg(test)]
    pub(crate) shutdown_admitted: crate::tokio::sync::watch::Sender<bool>,
    /// Test probe: how many Retire/Respawn/Reload controls the actor has
    /// parked in its deferred-command queue, per target member, counted when
    /// it parks them.
    #[cfg(test)]
    pub(crate) parked_member_controls:
        crate::tokio::sync::watch::Sender<BTreeMap<AgentIdentity, u64>>,
}

#[derive(Debug)]
struct SettlementChannel {
    generation: Generation,
    sender: crate::tokio::sync::watch::Sender<RetirementSettlement>,
}

impl MemberLifecycleObservations {
    fn members(&self) -> std::sync::MutexGuard<'_, BTreeMap<AgentIdentity, SettlementChannel>> {
        self.members
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Publish `settlement` for the retirement of `identity`'s `generation`.
    /// Progress after a terminal value, or any value for another incarnation,
    /// starts a new channel, so observers of the previous retirement keep its
    /// terminal value. The latest channel is kept (a tombstone) until the
    /// identity's next retirement.
    pub(crate) fn publish(
        &self,
        identity: &AgentIdentity,
        generation: Generation,
        settlement: RetirementSettlement,
    ) {
        let mut members = self.members();
        match members.get(identity) {
            Some(channel)
                if channel.generation == generation
                    && (!channel.sender.borrow().is_settled() || settlement.is_settled()) =>
            {
                channel.sender.send_replace(settlement);
            }
            _ => {
                let (sender, _) = crate::tokio::sync::watch::channel(settlement);
                members.insert(identity.clone(), SettlementChannel { generation, sender });
            }
        }
    }

    /// Current settlement of `identity`'s latest retirement, if any.
    pub(crate) fn current(&self, identity: &AgentIdentity) -> Option<RetirementSettlement> {
        self.members()
            .get(identity)
            .map(|channel| channel.sender.borrow().clone())
    }

    /// Record the report of a completed Shutdown, before its reply is sent.
    pub(crate) fn store_shutdown_report(&self, report: MobShutdownReport) {
        *self
            .last_shutdown_report
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(report);
    }

    /// The report of the most recent completed Shutdown.
    pub(crate) fn last_shutdown_report(&self) -> Option<MobShutdownReport> {
        self.last_shutdown_report
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Record how the actor task ended. Written once, by its spawn wrapper.
    pub(crate) fn record_actor_task_end(&self, end: ActorTaskEnd) {
        self.actor_task.send_replace(end);
    }

    /// Observe how the actor task ends.
    pub(crate) fn subscribe_actor_task(&self) -> crate::tokio::sync::watch::Receiver<ActorTaskEnd> {
        self.actor_task.subscribe()
    }

    pub(crate) fn subscribe(&self, identity: &AgentIdentity) -> Option<RetirementSettlementWatch> {
        self.members()
            .get(identity)
            .map(|channel| RetirementSettlementWatch {
                generation: channel.generation,
                rx: channel.sender.subscribe(),
            })
    }
}

/// What a mob Shutdown did for one member.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum MemberShutdownOutcome {
    /// The member's runtime binding was unregistered.
    Unregistered,
    /// The member's in-flight retirement was interrupted at `stage`; it stays
    /// `Retiring` and the next resume drives it again.
    RetirementInterrupted { stage: RetirementStage },
    /// The member's retirement was already stuck at `stage`.
    RetirementStuck {
        stage: RetirementStage,
        cause: Arc<MobError>,
    },
    /// The runtime refused or has not yet completed the unregister; `stage`
    /// names the unregister step. `settlement` follows the member's exact
    /// registration after Shutdown returned, also after the mob's actor
    /// exited.
    #[non_exhaustive]
    UnregisterPending {
        stage: String,
        settlement: MemberUnregisterSettlement,
    },
    /// An effect owned by the member did not settle before teardown and its
    /// custody is retained.
    EffectCustodyRetained { context: &'static str },
}

impl MemberShutdownOutcome {
    /// Whether Shutdown left nothing outstanding for the member.
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Unregistered)
    }
}

/// How often a [`MemberUnregisterSettlement`] whose mob runtime handle is
/// gone re-reads whether its registration has ended: nothing announces it.
const ENDED_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Where a member's runtime unregister that a Shutdown reported pending
/// stands ([`MemberUnregisterSettlement`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MemberUnregisterState {
    /// The member's registration is gone: its exact runtime unregister
    /// completed and no registration of the session is current, or the
    /// registration has ended for good.
    Settled,
    /// The unregister has not completed yet.
    Pending,
    /// The unregister failed. Nothing settles it later.
    Failed(String),
}

/// The settlement of one member's runtime unregister that a mob Shutdown
/// reported as [`MemberShutdownOutcome::UnregisterPending`]. It follows the
/// member's exact registration after Shutdown returned, also after the mob's
/// actor exited.
///
/// It owns no teardown. It joins the runtime's unregister coordinator for
/// that exact registration. When Shutdown's deadline passed before that
/// unregister was admitted, it admits it through the same exact-registration
/// path Shutdown uses, which never forces and never reaches a registration
/// the session gains later. A durability-degraded registration first goes
/// through the runtime's reload-required discard, as in Shutdown's own
/// teardown, and the cold successor that discard publishes is the
/// registration then unregistered and followed. It holds the mob's runtime
/// handle only weakly, so it keeps nothing alive.
#[derive(Clone)]
pub struct MemberUnregisterSettlement {
    /// The exact registration followed: the member's own, or the cold
    /// successor a reload-required discard replaced it with.
    registration: meerkat_runtime::RuntimeSessionRegistrationWitness,
    /// The coordinator's observer, once the unregister was admitted.
    observer: Option<meerkat_runtime::RuntimeSessionUnregisterObserver>,
    runtime: Weak<meerkat_runtime::MeerkatMachine>,
    /// The mob's session-operation bindings, so a reload-required discard
    /// retains the member's bound operation as Shutdown's does.
    #[cfg(feature = "runtime-adapter")]
    ops_adapter: Weak<super::ops_adapter::MobOpsAdapter>,
    /// `Settled` or `Failed`, once reached.
    terminal: Option<MemberUnregisterState>,
}

impl MemberUnregisterSettlement {
    #[cfg(feature = "runtime-adapter")]
    pub(crate) fn new(
        registration: meerkat_runtime::RuntimeSessionRegistrationWitness,
        observer: Option<meerkat_runtime::RuntimeSessionUnregisterObserver>,
        runtime: &Arc<meerkat_runtime::MeerkatMachine>,
        ops_adapter: Option<&Arc<super::ops_adapter::MobOpsAdapter>>,
    ) -> Self {
        Self {
            registration,
            observer,
            runtime: Arc::downgrade(runtime),
            ops_adapter: ops_adapter.map_or_else(Weak::new, Arc::downgrade),
            terminal: None,
        }
    }

    /// The state as known now, without waiting: the terminal state
    /// [`Self::settle_within`] reached, `Failed` once the coordinator
    /// published a failure, `Settled` once the registration has provably
    /// ended, `Pending` otherwise. A completed unregister reads `Settled`
    /// here only when the registration's own end proves it;
    /// `settle_within` verifies it through the runtime.
    pub fn try_state(&self) -> MemberUnregisterState {
        if let Some(terminal) = &self.terminal {
            return terminal.clone();
        }
        if let Some(observer) = &self.observer {
            match observer.try_result() {
                // Refused until a durability repair: `settle_within` runs the
                // discard and admits the unregister again.
                Ok(Some(Err(error))) if unregister_refused_until_repair(&error) => {}
                Ok(Some(Err(error))) | Err(error) => return self.failed(&error),
                Ok(Some(Ok(())) | None) => {}
            }
        }
        self.ended_or_pending()
    }

    /// Wait until `deadline` for the unregister to settle: join the exact
    /// registration's coordinator (admitting its unregister first when none
    /// was admitted yet), then prove the registration gone the way Shutdown's
    /// own teardown does. Dropping the future cancels only this wait, never
    /// the unregister. `Pending` at the deadline; a later call continues. A
    /// terminal state is kept.
    pub async fn settle_within(&mut self, deadline: Instant) -> MemberUnregisterState {
        if let Some(terminal) = &self.terminal {
            return terminal.clone();
        }
        let state = self.settle(deadline).await;
        if state != MemberUnregisterState::Pending {
            self.terminal = Some(state.clone());
        }
        state
    }

    async fn settle(&mut self, deadline: Instant) -> MemberUnregisterState {
        let remaining = || deadline.saturating_duration_since(Instant::now());
        // A refusal until a durability repair, from the admission or from the
        // coordinator, is followed by the discard and a fresh admission once
        // per call; a second one waits for a later call.
        let mut repair_retried = false;
        loop {
            if let Some(observer) = self.observer.as_mut() {
                match crate::tokio::time::timeout(remaining(), observer.wait_for_result()).await {
                    Err(_) => return MemberUnregisterState::Pending,
                    Ok(Ok(())) => return self.verify_ended(deadline).await,
                    // The coordinator ended without unregistering and released
                    // the registration for the discard.
                    Ok(Err(error)) if unregister_refused_until_repair(&error) => {
                        self.observer = None;
                    }
                    Ok(Err(error)) => return self.failed(&error),
                }
            } else {
                // Shutdown's deadline passed before the unregister was
                // admitted. Admit it now, or join the coordinator already
                // running for exactly this registration.
                let Some(runtime) = self.runtime.upgrade() else {
                    return self.ended_within(deadline).await;
                };
                #[cfg(feature = "runtime-adapter")]
                if let Some(state) = self.discard_degraded_first(&runtime, deadline).await {
                    return state;
                }
                let admitted = crate::tokio::time::timeout(
                    remaining(),
                    runtime.observe_unregister_session_registration_if_current(&self.registration),
                )
                .await;
                match admitted {
                    Err(_) => return MemberUnregisterState::Pending,
                    Ok(Ok(
                        meerkat_runtime::RuntimeSessionUnregisterAdmission::Completed
                        | meerkat_runtime::RuntimeSessionUnregisterAdmission::NotCurrent,
                    )) => return self.verify_ended(deadline).await,
                    Ok(Ok(meerkat_runtime::RuntimeSessionUnregisterAdmission::Pending(
                        observer,
                    ))) => {
                        self.observer = Some(observer);
                        continue;
                    }
                    Ok(Err(error)) if unregister_refused_until_repair(&error) => {}
                    Ok(Err(error)) => return self.failed(&error),
                }
            }
            if repair_retried {
                return MemberUnregisterState::Pending;
            }
            repair_retried = true;
        }
    }

    /// Before its exact unregister is admitted, a durability-degraded
    /// registration goes through the runtime's reload-required discard: its
    /// exited runtime loop can hold the session's mutation fence, which only
    /// that discard releases. After a discard the cold successor is the
    /// registration followed. `Some` ends this call with that state.
    #[cfg(feature = "runtime-adapter")]
    async fn discard_degraded_first(
        &mut self,
        runtime: &Arc<meerkat_runtime::MeerkatMachine>,
        deadline: Instant,
    ) -> Option<MemberUnregisterState> {
        use super::provisioner::{DegradedUnregisterStep, MemberSessionDisposalArc};

        let ops_adapter = self.ops_adapter.upgrade();
        match MemberSessionDisposalArc::discard_degraded_registration_first(
            runtime,
            ops_adapter.as_ref(),
            &self.registration,
            deadline,
        )
        .await
        {
            DegradedUnregisterStep::Unchanged => None,
            DegradedUnregisterStep::Replaced(successor) => {
                self.registration = successor;
                None
            }
            DegradedUnregisterStep::Gone => Some(MemberUnregisterState::Settled),
            DegradedUnregisterStep::TimedOut => Some(MemberUnregisterState::Pending),
            DegradedUnregisterStep::Failed(failure) => Some(MemberUnregisterState::Failed(failure)),
        }
    }

    /// The exact unregister completed, or its registration is no longer
    /// current: prove the registration gone, as Shutdown's own teardown
    /// does. The registration still current after its coordinator completed
    /// is a failure. A later registration of the session is not this
    /// settlement's: it settles once the exact one has provably ended.
    async fn verify_ended(&self, deadline: Instant) -> MemberUnregisterState {
        let Some(runtime) = self.runtime.upgrade() else {
            return self.ended_within(deadline).await;
        };
        let session_id = self.registration.session_id();
        match runtime
            .current_session_registration_witness(session_id)
            .await
        {
            None => MemberUnregisterState::Settled,
            Some(current) if current == self.registration => {
                MemberUnregisterState::Failed(format!(
                    "runtime unregister coordinator for session {session_id} completed while its exact registration remained current"
                ))
            }
            Some(_) => self.ended_within(deadline).await,
        }
    }

    /// Without the mob's runtime handle, only the registration's own end
    /// proves the unregister settled. Dropping that handle ends nothing: the
    /// runtime lives on in every other handle, its unregister coordinator
    /// among them (the coordinator runs on a clone, so dropping a caller's
    /// handle never cancels a teardown), and the registration can still be
    /// current there. So a dropped handle settles only once the registration
    /// has ended for good, which no current entry anywhere allows; until
    /// then the unregister stays pending.
    fn ended_or_pending(&self) -> MemberUnregisterState {
        if self.registration.has_ended() {
            MemberUnregisterState::Settled
        } else {
            MemberUnregisterState::Pending
        }
    }

    /// [`Self::ended_or_pending`], watched until `deadline`: nothing
    /// announces the end, so it is polled.
    async fn ended_within(&self, deadline: Instant) -> MemberUnregisterState {
        loop {
            let state = self.ended_or_pending();
            let remaining = deadline.saturating_duration_since(Instant::now());
            if state != MemberUnregisterState::Pending || remaining.is_zero() {
                return state;
            }
            crate::tokio::time::sleep(remaining.min(ENDED_POLL_INTERVAL)).await;
        }
    }

    fn failed(&self, error: &impl fmt::Display) -> MemberUnregisterState {
        MemberUnregisterState::Failed(format!(
            "runtime unregister of session {} failed: {error}",
            self.registration.session_id()
        ))
    }
}

/// Whether the runtime refused a member's unregister only until its
/// registration's durability is repaired: the reload-required discard
/// releases it, so the unregister is pending, not failed.
pub(crate) fn unregister_refused_until_repair(error: &meerkat_runtime::RuntimeDriverError) -> bool {
    matches!(
        error,
        meerkat_runtime::RuntimeDriverError::RecoveryRepairBlocked { .. }
    )
}

impl fmt::Debug for MemberUnregisterSettlement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemberUnregisterSettlement")
            .field("session_id", self.registration.session_id())
            .field("coordinator_admitted", &self.observer.is_some())
            .field("terminal", &self.terminal)
            .finish_non_exhaustive()
    }
}

/// Caller options for [`MobHandle::shutdown_with_report`](super::MobHandle::shutdown_with_report).
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct ShutdownOptions {
    /// The caller's own bound for the Shutdown's waits. When it elapses,
    /// every member still being waited on is reported and Shutdown returns.
    /// `None` uses the member lifecycle hang guard as the failure bound.
    pub deadline: Option<meerkat_core::time_compat::Instant>,
}

impl ShutdownOptions {
    /// Bound the Shutdown's waits by `deadline`.
    #[must_use]
    pub fn with_deadline(mut self, deadline: meerkat_core::time_compat::Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }
}

/// Per-member account of a completed mob Shutdown.
#[derive(Debug, Clone, Default)]
pub struct MobShutdownReport {
    pub members: BTreeMap<AgentIdentity, MemberShutdownOutcome>,
    /// What the Shutdown did to each autonomous member's run: a run current
    /// when it reached the member is cancelled immediately
    /// ([`MemberStopRun::CancelledByShutdown`](super::stop_report::MemberStopRun::CancelledByShutdown)).
    pub runs: BTreeMap<AgentIdentity, super::stop_report::MemberStopRun>,
    /// Whether each roster member's run starts were held by the Shutdown:
    /// `Held` (locally, or through a remote member's host when the Shutdown
    /// stopped that member), `NotHoldable`, or `DelegatedToHost` for a remote
    /// member the Shutdown did not contact.
    pub run_starts: BTreeMap<AgentIdentity, super::stop_report::MemberRunStarts>,
}

impl MobShutdownReport {
    /// Whether every member's outcome is clean.
    pub fn is_clean(&self) -> bool {
        self.members.values().all(MemberShutdownOutcome::is_clean)
    }

    pub(crate) fn record(&mut self, identity: AgentIdentity, outcome: MemberShutdownOutcome) {
        // An outstanding outcome is never overwritten by a later clean one.
        match self.members.get(&identity) {
            Some(existing) if !existing.is_clean() && outcome.is_clean() => {}
            _ => {
                self.members.insert(identity, outcome);
            }
        }
    }
}
