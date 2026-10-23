//! The lifecycle of a mob as its handles observe it without asking its actor.

use std::sync::Arc;

use crate::{MobError, MobState};

/// The lifecycle of a mob as its handles observe it without asking its
/// actor: the last phase the actor published and, once the actor has
/// stopped, why.
///
/// [`MobHandle::status`](super::MobHandle::status) asks the actor, so it
/// waits behind the actor's work. An observation reads only what the actor
/// published, so observation surfaces (listings, consoles, address
/// observation) stay live while the actor is busy and still see that an
/// actor has stopped. The phase alone is
/// [`MobHandle::status_observation_snapshot`](super::MobHandle::status_observation_snapshot).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MobLifecycleObservation {
    /// The last lifecycle phase the mob's actor published: the machine
    /// phase, not whether the actor is alive.
    pub phase: MobState,
    /// Present once the mob's actor has stopped, so it admits no command
    /// until a restart recovers the mob from its durable state:
    /// - the cause the actor recorded when it fail-stopped (its startup
    ///   recovery failed, or a durable mutation's outcome stayed uncertain),
    ///   which [`MobHandle::status`](super::MobHandle::status) reports in
    ///   [`MobError::SharedLifecycleFailure`];
    /// - for an actor task that ended while `phase` was still live
    ///   (`Creating` or `Running`) and recorded no cause, the closed-actor
    ///   error that `status` reports.
    pub failure: Option<Arc<MobError>>,
}

impl MobLifecycleObservation {
    pub(super) fn new(phase: MobState, failure: Option<Arc<MobError>>) -> Self {
        Self { phase, failure }
    }
}
