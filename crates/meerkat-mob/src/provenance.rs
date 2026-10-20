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
///
/// A create for a caller session records whether that session made it with
/// its own create capability (the agent mob tools' `mob_create`, whose
/// answer grants the session manage over the mob): such a create records
/// [`Self::OperatorSession`] or [`Self::OperatorMember`]. A create a host
/// makes for a caller session records [`Self::Session`] or
/// [`Self::Member`]: the session is the mob's caller and owner, and the
/// create grants it nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum MobProvenance {
    /// A create for a member's session: exactly one hosted roster named the
    /// caller session, as `creator_identity` at `creator_generation` of the
    /// mob `parent_mob_id` at `parent_incarnation`.
    Member {
        parent_mob_id: MobId,
        parent_incarnation: MobIncarnationId,
        creator_identity: AgentIdentity,
        creator_generation: Generation,
        owner_session: SessionId,
    },
    /// The host or console created the mob, with no caller session.
    Host,
    /// A create for a caller session that no hosted roster named: the mob
    /// set was restored and every roster was read.
    Session { session_id: SessionId },
    /// More than one hosted roster named the caller session. No roster is
    /// picked: the mob has no member creator.
    Ambiguous { matching_rosters: u32 },
    /// The mob was created before its creation was recorded (a store from
    /// before 0.9.4), so its creator is not known.
    Unrecorded,
    /// The caller session created the mob with its own create capability,
    /// and no hosted roster named that session: [`Self::Session`] for a
    /// session's own create.
    OperatorSession { session_id: SessionId },
    /// The caller session created the mob with its own create capability as
    /// a member: [`Self::Member`] for a member session's own create.
    OperatorMember {
        parent_mob_id: MobId,
        parent_incarnation: MobIncarnationId,
        creator_identity: AgentIdentity,
        creator_generation: Generation,
        owner_session: SessionId,
    },
}
