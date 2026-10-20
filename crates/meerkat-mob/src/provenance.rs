//! Where a mob came from: the typed provenance its creation records.

use crate::ids::{AgentIdentity, Generation, MobId};
use meerkat_core::SessionId;
use meerkat_core::service::MobIncarnationId;
use serde::{Deserialize, Serialize};

/// The creator of a mob, recorded once with the mob's creation and never
/// rewritten: [`crate::MobHandle::provenance`] reads it for a running mob,
/// [`crate::MobStorage::creation_provenance`] for a stored one.
///
/// Every variant is a positive fact. A creation whose creator could not be
/// read is refused instead
/// ([`crate::MobError::ProvenanceUnavailable`]), so no variant stands for a
/// failed read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum MobProvenance {
    /// A member created the mob: exactly one hosted roster named the caller
    /// session, as `creator_identity` at `creator_generation` of the mob
    /// `parent_mob_id` at `parent_incarnation`.
    Member {
        parent_mob_id: MobId,
        parent_incarnation: MobIncarnationId,
        creator_identity: AgentIdentity,
        creator_generation: Generation,
        owner_session: SessionId,
    },
    /// The host or console created the mob, with no caller session.
    Host,
    /// A caller session created the mob, and no hosted roster named that
    /// session: the mob set was restored and every roster was read.
    Session { session_id: SessionId },
    /// More than one hosted roster named the caller session. No roster is
    /// picked: the mob has no member creator.
    Ambiguous { matching_rosters: u32 },
    /// The mob was created before its creation was recorded (a store from
    /// before 0.9.4), so its creator is not known.
    Unrecorded,
}
