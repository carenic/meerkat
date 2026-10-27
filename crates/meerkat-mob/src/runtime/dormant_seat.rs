//! Dormant seats: an identity becomes a member without being materialized
//! (design section 10 and 4.3j; rows R5, R6, R13, R14, G12, F8, F9).
//!
//! [`super::MobHandle::seat_dormant_members`] takes each chunk of specs
//! through three steps:
//! 1. The actor plans each spec, reading nothing durable beyond the profile
//!    resolution a spawn also makes there. The plan is a typed answer, the
//!    spawn of a fresh spec that owes a kickoff (R13: such a member is
//!    embodied exactly as a spawn is), or what the seat needs.
//! 2. The handle prepares each seat off the actor: the session's archived
//!    state from the session owner's pinned observation (no body load), a
//!    fresh seat's empty session through the fresh-spawn create path when it
//!    does not exist yet (R14, under its name-based id, G12), and the
//!    endpoint from the session's stored comms key.
//! 3. The actor commits the chunk in one step (F8): on one prepared
//!    authority, per identity `AuthorizeSpawnProfile`, `BeginSpawnExec` and
//!    `CommitDormantSeat` as one batch (an identity the machine refuses is
//!    undone and answered alone), then one runtime obligation scan and each
//!    seated identity's `PlanBootMaterialization` (F9), then the
//!    `MemberSpawned` rows in one `append_batch` before the base-checked
//!    commit.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use meerkat_core::comms::TrustedPeerDescriptor;
use meerkat_core::service::{CreateSessionRequest, MobIncarnationId};
use meerkat_core::types::SessionId;

use crate::error::{MobError, SessionResumeUnavailableReason};
use crate::ids::{AgentIdentity, Generation, MobId, ProfileName};
use crate::machines::mob_machine as mob_dsl;

/// Specs one seat step commits at most: one `append_batch` per chunk.
pub(crate) const DORMANT_SEAT_CHUNK: usize = 256;

/// Seats a chunk prepares at once off the actor.
pub(crate) const DORMANT_SEAT_PREPARE_CONCURRENCY: usize = 16;

/// What [`super::MobHandle::seat_dormant_members`] did with one spec.
#[derive(Debug)]
#[non_exhaustive]
pub enum DormantSeatOutcome {
    /// Seated Dormant on `session_id`: a member, wireable and reachable, not
    /// materialized. Its first delivery wakes it (or MobMachine's boot
    /// verdict does at once, for a member with runtime work).
    Seated {
        agent_identity: AgentIdentity,
        session_id: SessionId,
    },
    /// Already seated on exactly this session (an earlier seat of the same
    /// spec): nothing was written.
    AlreadySeated {
        agent_identity: AgentIdentity,
        session_id: SessionId,
    },
    /// The spec's session is archived. Nothing was written: an archived
    /// session is never bound.
    SessionArchived {
        agent_identity: AgentIdentity,
        session_id: SessionId,
    },
    /// A fresh spec that owes a kickoff (an `AutonomousHost` member, or a
    /// member with an initial message) is not seated: it was spawned through
    /// the ordinary spawn path, live, and its kickoff runs once.
    Embodied {
        agent_identity: AgentIdentity,
        session_id: SessionId,
    },
    /// Refused for this spec alone: nothing of it was written.
    Refused {
        agent_identity: AgentIdentity,
        error: MobError,
    },
}

impl DormantSeatOutcome {
    /// The identity this outcome answers for.
    pub fn agent_identity(&self) -> &AgentIdentity {
        match self {
            Self::Seated { agent_identity, .. }
            | Self::AlreadySeated { agent_identity, .. }
            | Self::SessionArchived { agent_identity, .. }
            | Self::Embodied { agent_identity, .. }
            | Self::Refused { agent_identity, .. } => agent_identity,
        }
    }
}

/// Where a seat takes its member's peer endpoint from: the comms transport
/// the host's members run on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MemberSeatEndpointSource {
    /// Inproc comms with session-scoped identities: a member's key lives
    /// under `<identity_root>/<session id>`, and its endpoint is the inproc
    /// address of its comms name, as its woken runtime reports it.
    SessionScopedInproc { identity_root: PathBuf },
    /// Network comms (Tcp or Uds): the key belongs to the realm, not to the
    /// session, so a seat records no endpoint. The member is seated with its
    /// endpoint owed (`EndpointPending`), and its boot wake registers it.
    Network,
}

/// The name-based id of a fresh seat's session (G12): the same mob, mob
/// incarnation, identity and generation always name the same session, so a
/// seat re-driven after a crash finds the session its earlier attempt
/// created, and no ownerless session can exist.
pub(crate) fn fresh_seat_session_id(
    mob_id: &MobId,
    incarnation: MobIncarnationId,
    identity: &AgentIdentity,
    generation: Generation,
) -> SessionId {
    let incarnation = incarnation.to_string();
    let generation = generation.get().to_string();
    let mut name = b"meerkat.mob.fresh-seat.v1".to_vec();
    for component in [
        mob_id.as_str(),
        incarnation.as_str(),
        identity.as_str(),
        generation.as_str(),
    ] {
        let bytes = component.as_bytes();
        name.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        name.extend_from_slice(bytes);
    }
    SessionId::from_uuid(uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, &name))
}

/// How a seat binds its session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime) enum DormantSeatLaunch {
    /// A new empty session under the fresh seat's name-based id.
    Fresh,
    /// The existing session the spec names.
    Resume,
}

/// One spec of a seat chunk, as the actor planned it.
pub(in crate::runtime) enum DormantSeatPlanEntry {
    /// Answered at planning: nothing of it is written.
    Done(DormantSeatOutcome),
    /// Seated on exactly this session already: the commit step clears the
    /// identity's host preparation and plans it, writing nothing.
    AlreadySeated {
        agent_identity: AgentIdentity,
        session_id: SessionId,
    },
    /// A fresh spec that owes a kickoff: spawned through the ordinary spawn
    /// path, on its name-based session.
    Embody(Box<super::handle::SpawnMemberSpec>),
    /// A seat to prepare and commit.
    Seat(Box<DormantSeatCandidate>),
}

/// The plan of one seat chunk.
pub(in crate::runtime) struct DormantSeatPlan {
    pub(in crate::runtime) entries: Vec<DormantSeatPlanEntry>,
    pub(in crate::runtime) endpoint_source: Option<MemberSeatEndpointSource>,
    pub(in crate::runtime) provisioner: Arc<dyn super::provisioner::MobProvisioner>,
}

/// What one seat's commit needs, as the actor planned it.
pub(in crate::runtime) struct DormantSeatCandidate {
    pub(in crate::runtime) agent_identity: AgentIdentity,
    pub(in crate::runtime) profile_name: ProfileName,
    pub(in crate::runtime) launch: DormantSeatLaunch,
    pub(in crate::runtime) session_id: SessionId,
    pub(in crate::runtime) generation: Generation,
    /// A fresh seat's create request, built as a fresh spawn builds it.
    /// `None` for a resume, and for a service that keeps no durable member
    /// sessions (its wake's fresh recreation creates the session there).
    pub(in crate::runtime) create_session: Option<Box<CreateSessionRequest>>,
    pub(in crate::runtime) runtime_mode: crate::MobRuntimeMode,
    pub(in crate::runtime) labels: BTreeMap<String, String>,
    pub(in crate::runtime) continuity_intent: super::SpawnContinuityIntent,
    /// The spec's creation origin: a host-root seat is a root creation.
    pub(in crate::runtime) creation_origin: crate::member_creation::MemberCreationOrigin,
    pub(in crate::runtime) effective_profile_override: Option<crate::profile::Profile>,
    pub(in crate::runtime) effective_model_override: Option<String>,
    /// The generated `AuthorizeSpawnProfile` input and the authorization it
    /// must answer with.
    pub(in crate::runtime) authorize_input: mob_dsl::MobMachineInput,
    pub(in crate::runtime) authorized:
        super::spawn_profile_authority::AuthorizedSpawnProfileMaterial,
    pub(in crate::runtime) observations: super::actor::SpawnExecObservations,
}

/// One entry of a seat chunk's commit, in the chunk's order.
pub(in crate::runtime) enum DormantSeatCommitEntry {
    /// Seated on exactly this session already.
    AlreadySeated {
        agent_identity: AgentIdentity,
        session_id: SessionId,
    },
    /// Spawned live by the ordinary spawn path: the commit step clears its
    /// host preparation, which the spawn does not.
    Embodied {
        agent_identity: AgentIdentity,
        session_id: SessionId,
    },
    /// A prepared seat: its session exists (or is recreated at its wake),
    /// and `endpoint` is its stored-key endpoint (`None` under network
    /// comms).
    Seat {
        candidate: Box<DormantSeatCandidate>,
        endpoint: Option<TrustedPeerDescriptor>,
    },
}

/// Prepare one seat off the actor: its session's archived state, a fresh
/// seat's empty session, and its endpoint. A spec this refuses is answered
/// with its typed outcome, and nothing of it is written.
pub(in crate::runtime) async fn prepare_dormant_seat(
    session_service: &dyn super::session_service::MobSessionService,
    provisioner: &dyn super::provisioner::MobProvisioner,
    endpoint_source: Option<&MemberSeatEndpointSource>,
    mob_id: &MobId,
    mut candidate: Box<DormantSeatCandidate>,
) -> Result<DormantSeatCommitEntry, Box<DormantSeatOutcome>> {
    let agent_identity = candidate.agent_identity.clone();
    let session_id = candidate.session_id.clone();
    let refused = |error: MobError| {
        Box::new(DormantSeatOutcome::Refused {
            agent_identity: agent_identity.clone(),
            error,
        })
    };
    let Some(endpoint_source) = endpoint_source else {
        return Err(refused(MobError::WiringError(format!(
            "seating '{agent_identity}' dormant needs the members' comms identity source \
             (MobBuilder::with_member_comms_identity_root or with_network_member_comms)"
        ))));
    };
    // The session owner's pinned observation: authority, catalog lifecycle
    // and runtime lifecycle in one snapshot, no body load.
    let lifecycle = session_service
        .observe_session_resume_authority(&session_id)
        .await
        .map_err(|error| refused(MobError::from(error)))?
        .lifecycle();
    let created_key = match lifecycle {
        super::session_service::SessionResumeLifecycle::Archived { .. } => {
            return Err(Box::new(DormantSeatOutcome::SessionArchived {
                agent_identity: agent_identity.clone(),
                session_id,
            }));
        }
        super::session_service::SessionResumeLifecycle::ContradictoryDurableAuthority {
            runtime_state,
            ..
        } => {
            return Err(refused(MobError::SessionUnavailableForResume {
                session_id,
                reason: SessionResumeUnavailableReason::ContradictoryDurableAuthority,
                runtime_state: runtime_state.map(|state| state.to_string()),
                verdict: None,
            }));
        }
        // The session exists: a resume's, or the one an earlier attempt of
        // this fresh seat created.
        super::session_service::SessionResumeLifecycle::Active { .. } => None,
        super::session_service::SessionResumeLifecycle::NoCurrentDurableAuthority => {
            match (candidate.launch, candidate.create_session.take()) {
                (DormantSeatLaunch::Fresh, Some(create_session)) => {
                    created_dormant_session_key(provisioner, *create_session)
                        .await
                        .map_err(&refused)?
                }
                // A service with no durable member sessions: the member's
                // wake recreates its session fresh.
                (DormantSeatLaunch::Fresh, None) => None,
                (DormantSeatLaunch::Resume, _) => {
                    return Err(refused(MobError::SessionUnavailableForResume {
                        session_id,
                        reason: SessionResumeUnavailableReason::Absent,
                        runtime_state: None,
                        verdict: None,
                    }));
                }
            }
        }
    };
    let endpoint = match endpoint_source {
        MemberSeatEndpointSource::Network => None,
        MemberSeatEndpointSource::SessionScopedInproc { identity_root } => {
            let public_key = stored_member_comms_key(identity_root, &session_id)
                .await
                .map_err(&refused)?;
            // The key the session's own build loaded must be the stored one:
            // a different one means the configured root is not the session
            // service's, and the seat would record an endpoint its wake
            // disagrees with.
            if created_key.is_some_and(|created| created != public_key) {
                return Err(refused(MobError::WiringError(format!(
                    "the comms identity session '{session_id}' was built with differs from \
                     the one under the configured member comms identity root"
                ))));
            }
            Some(
                inproc_member_endpoint(
                    mob_id,
                    &candidate.profile_name,
                    &agent_identity,
                    public_key,
                )
                .map_err(&refused)?,
            )
        }
    };
    Ok(DormantSeatCommitEntry::Seat {
        candidate,
        endpoint,
    })
}

/// Create a fresh seat's empty session; the public key its build used.
#[cfg(feature = "runtime-adapter")]
async fn created_dormant_session_key(
    provisioner: &dyn super::provisioner::MobProvisioner,
    create_session: CreateSessionRequest,
) -> Result<Option<[u8; 32]>, MobError> {
    provisioner
        .create_dormant_member_session(create_session)
        .await
        .map(|created| created.public_key)
}

#[cfg(not(feature = "runtime-adapter"))]
async fn created_dormant_session_key(
    _provisioner: &dyn super::provisioner::MobProvisioner,
    _create_session: CreateSessionRequest,
) -> Result<Option<[u8; 32]>, MobError> {
    Err(MobError::Internal(
        "a dormant seat's session is created through the runtime adapter".into(),
    ))
}

/// The public key of a member session's stored comms identity: one local
/// key-store read, no session load. A session whose identity does not exist
/// yet gets the one its first build will load.
#[cfg(not(target_arch = "wasm32"))]
async fn stored_member_comms_key(
    identity_root: &std::path::Path,
    session_id: &SessionId,
) -> Result<[u8; 32], MobError> {
    let keypair =
        meerkat_comms::Keypair::load_or_generate(&identity_root.join(session_id.to_string()))
            .await
            .map_err(|error| {
                MobError::WiringError(format!(
                    "the comms identity of session '{session_id}' could not be read: {error}"
                ))
            })?;
    Ok(*keypair.public_key().as_bytes())
}

#[cfg(target_arch = "wasm32")]
async fn stored_member_comms_key(
    _identity_root: &std::path::Path,
    session_id: &SessionId,
) -> Result<[u8; 32], MobError> {
    Err(MobError::WiringError(format!(
        "a browser runtime keeps no durable comms identity for session '{session_id}'"
    )))
}

/// The inproc endpoint a member's woken runtime reports: its comms name, the
/// inproc address of that name, and its session's key.
pub(in crate::runtime) fn inproc_member_endpoint(
    mob_id: &MobId,
    role: &ProfileName,
    identity: &AgentIdentity,
    public_key: [u8; 32],
) -> Result<TrustedPeerDescriptor, MobError> {
    let comms_name =
        super::actor::render_member_comms_name(mob_id.as_str(), role.as_str(), identity.as_str())?;
    let peer_id = meerkat_comms::PubKey::new(public_key).to_peer_id();
    TrustedPeerDescriptor::unsigned_with_pubkey(
        comms_name.clone(),
        peer_id.to_string(),
        public_key,
        format!("inproc://{comms_name}"),
    )
    .map_err(|error| {
        MobError::WiringError(format!(
            "invalid dormant seat endpoint for '{identity}': {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_seat_session_ids_are_name_based() {
        let mob = MobId::from("mob");
        let incarnation = MobIncarnationId::mint();
        let identity = AgentIdentity::from("worker");
        let first = fresh_seat_session_id(&mob, incarnation, &identity, Generation::new(3));
        assert_eq!(
            first,
            fresh_seat_session_id(&mob, incarnation, &identity, Generation::new(3)),
            "a re-driven seat derives the same session"
        );
        assert_ne!(
            first,
            fresh_seat_session_id(&mob, incarnation, &identity, Generation::new(4)),
            "the next generation is a new session"
        );
        assert_ne!(
            first,
            fresh_seat_session_id(
                &mob,
                incarnation,
                &AgentIdentity::from("worker2"),
                Generation::new(3)
            ),
        );
        assert_ne!(
            first,
            fresh_seat_session_id(
                &mob,
                MobIncarnationId::mint(),
                &identity,
                Generation::new(3)
            ),
            "a new mob incarnation never reuses a session"
        );
    }

    #[test]
    fn inproc_member_endpoint_is_the_woken_runtime_endpoint() {
        let key = meerkat_comms::Keypair::generate().public_key();
        let endpoint = inproc_member_endpoint(
            &MobId::from("mob"),
            &ProfileName::from("worker"),
            &AgentIdentity::from("w1"),
            *key.as_bytes(),
        )
        .expect("endpoint");
        assert_eq!(endpoint.peer_id, key.to_peer_id());
        assert_eq!(endpoint.pubkey, *key.as_bytes());
        assert_eq!(
            endpoint.address.to_string(),
            format!("inproc://{}", endpoint.name)
        );
        assert_eq!(
            mob_dsl::MemberPeerEndpoint::from(&endpoint).transport,
            mob_dsl::PeerTransportKind::Inproc
        );
    }
}
