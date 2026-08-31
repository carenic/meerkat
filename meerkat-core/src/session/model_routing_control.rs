//! Durable append-only model-routing control history carried on [`Session`].
//!
//! [`Session`]: crate::Session
//!
//! # What this is
//!
//! A committed, ordered, append-only handoff log. A run that asks for a
//! permanent model change cannot perform that change itself: the request and
//! its later resolution are recorded here, on the authoritative session
//! document, so a *different* owner (the runtime pre-admission seam) can act on
//! it after the originating run has durably committed.
//!
//! # What this is NOT
//!
//! This history is **not** routing authority. Nothing reads it to decide which
//! model a provider call uses; the canonical baseline stays owned by the
//! generated `MeerkatMachine` model-routing state. This module owns exactly one
//! thing: the durable *representation* of the handoff, plus the fail-closed
//! well-formedness rules that make an incoherent log unrepresentable.
//!
//! Claim, conflict adjudication, application, and terminality are decided by
//! generated machine authority. The derivations exposed here answer only "what
//! does the committed log literally say", which is why they are named for
//! records rather than for lifecycle status.
//!
//! # Vocabulary
//!
//! The *intent* vocabulary is not new: a committed record carries the existing
//! [`SwitchTurnRequestId`] and [`SwitchTurnIntent`], so the durable log and the
//! live switch-turn control surface cannot drift into two dialects for one
//! fact. The only nouns minted here are the durable record, its history, its
//! record-level disposition, and the abandon reason — none of which existed.
//!
//! These nouns must never be conflated with input-lifecycle vocabulary: a
//! routing intent survives across runs and is carried by the session document,
//! whereas input lifecycle state is per-input runtime state that is explicitly
//! uncarryable mid-run.
//!
//! # Durable-compatibility decision record
//!
//! Carrying a new durable fact on `Session` has three shapes. They were
//! compared before choosing, because the wrong choice strands released state.
//!
//! * **(a) additive typed field under the current envelope version — CHOSEN.**
//!   The field is `#[serde(default)]` and skipped when empty, so a document
//!   that owes no handoff is byte-identical to one written before this existed.
//!   No released `Session` is rewritten, no importer is needed, and
//!   `SESSION_VERSION` stays 3.
//! * **(b) an exact v3 → v4 importer.** Rejected. Doctrine currently sanctions
//!   exactly one historical importer (the frozen released-0.8.10 lane), and a
//!   second one would have to re-derive every `HeadCanonical` head token to
//!   restamp the version. That is a whole-corpus rewrite bought for a property
//!   (a) already provides.
//! * **(c) move the handoff to a versioned `RuntimeStore` domain.** Rejected on
//!   authority grounds, not convenience: an intent is only actionable when read
//!   from the committed authoritative session, so putting the source anywhere
//!   else makes the session document a mirror of it — two owners for one fact.
//!
//! ## How each representation actually fails closed
//!
//! The mechanism differs per carrier, and saying "`deny_unknown_fields`
//! everywhere" would be wrong:
//!
//! * **Session envelope and WholeBlob** — the decode shape is
//!   `deny_unknown_fields`, so an old binary handed a document that carries an
//!   owed handoff refuses it outright rather than silently dropping it.
//! * **`SessionHead` row** — `SessionHead` is NOT `deny_unknown_fields`, so an
//!   old binary would strip the field silently. What actually protects it is
//!   the head CAS binding: the log participates in the digest-addressed head
//!   preimage, so a stripped or tampered head no longer re-derives its stored
//!   token and reads fail closed as `Corrupted`. That binding is therefore
//!   load-bearing, not decorative.
//! * **Durable save transition** — the committed log is append-only across
//!   saves, enforced by a prefix-extension check at the head transition. Without
//!   it a stale writer silently drops an owed handoff, or resurrects a settled
//!   one and causes a second model rotation.
//!
//! # Deliberately deferred (named, not silent)
//!
//! * Wiring [`ModelRoutingIntentBoundaryTerminalization::authorize_session_archive`]
//!   into the archive realization path is Phase 2 work; this module ships the
//!   decision seam only.
//! * The claim/pending lifecycle (a request that generated authority has
//!   claimed but not yet realized) is NOT represented here. It is machine
//!   state, not durable document state, and lands with the generated
//!   model-routing lifecycle in Phase 1.

use serde::{Deserialize, Serialize};

use crate::generated::session_document;
use crate::image_generation::{
    SwitchTurnDenialReason, SwitchTurnDuration, SwitchTurnIntent, SwitchTurnOrigin,
    SwitchTurnRequestId,
};
use crate::lifecycle::identifiers::RunId;

/// Why a committed routing intent was terminalized without a decision.
///
/// Abandonment is the authoritative session-boundary path: the boundary that
/// ends the document's life also ends any handoff it still owes. Ordinary
/// graceful teardown is deliberately not a reason — a process exit does not
/// settle an owed handoff.
///
/// Deliberately NOT `Default`: an abandon reason must always be produced by the
/// boundary that actually terminalized the document, never defaulted into
/// existence by a caller assembling a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ModelRoutingIntentAbandonReason {
    /// The authoritative archive boundary terminalized the document.
    SessionArchived,
}

/// Machine-minted permission to terminalize owed handoffs at a session
/// boundary.
///
/// Opaque and deliberately NOT `Clone`: it is a capability, not a value. The
/// only way to obtain one is
/// [`Self::authorize_session_archive`], which actually invokes the generated
/// `SessionDocumentMachineAuthority` and classifies the verdict it emits. A
/// public effect literal is explicitly NOT accepted as capability, because a
/// generated effect enum is constructible from any crate and would make the
/// seal decorative.
#[derive(Debug)]
pub struct SessionArchiveControlTerminalizationAuthorization {
    reason: ModelRoutingIntentAbandonReason,
}

impl SessionArchiveControlTerminalizationAuthorization {
    /// Drive the generated session-document archive decision and mint
    /// terminalization authority iff the machine itself authorizes an archive
    /// that rewrites the durable document.
    ///
    /// This applies the input to the real generated authority, so the verdict
    /// cannot be forged: a caller who has not driven the machine to an
    /// `Archive` verdict has nothing to hand us. An `AlreadyArchived` verdict,
    /// or an archive that does not rewrite the document, terminalizes nothing —
    /// the record either already landed with the first archive commit or there
    /// is no document to append to.
    ///
    /// The emitted effects are returned so the caller still realizes the rest
    /// of the archive; this is a decision seam, not a second archive owner.
    pub fn authorize_session_archive(
        authority: &mut session_document::SessionDocumentMachineAuthority,
        session_id: session_document::SessionDocumentKey,
        runtime_backed: bool,
        durable_document_present: bool,
        runtime_observation: session_document::SessionArchiveRuntimeObservation,
    ) -> Result<
        (Vec<session_document::SessionDocumentEffect>, Option<Self>),
        session_document::SessionDocumentError,
    > {
        let effects = authority.archive_session_document(
            session_id,
            runtime_backed,
            durable_document_present,
            runtime_observation,
        )?;
        let authorization = effects.iter().find_map(Self::from_machine_effect);
        Ok((effects, authorization))
    }

    /// Recognize a machine-emitted archive verdict that authorizes
    /// terminalization.
    ///
    /// Crate-private on purpose: the public door is
    /// [`Self::authorize_session_archive`], which produces the effect by
    /// driving the machine. A foreign crate cannot reach this with a literal.
    pub(crate) fn from_machine_effect(
        effect: &session_document::SessionDocumentEffect,
    ) -> Option<Self> {
        match effect {
            session_document::SessionDocumentEffect::SessionArchiveResolved {
                disposition: session_document::SessionArchiveDisposition::Archive,
                write_document: true,
                ..
            } => Some(Self {
                reason: ModelRoutingIntentAbandonReason::SessionArchived,
            }),
            _ => None,
        }
    }

    #[must_use]
    pub const fn reason(&self) -> ModelRoutingIntentAbandonReason {
        self.reason
    }
}

/// One durable record in the append-only model-routing control history.
///
/// Every variant names the exact request identity, the exact originating run,
/// and the full typed intent. Terminal variants repeat the intent rather than
/// referencing it, so a record is self-describing when read in isolation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SessionModelRoutingControlRecord {
    /// A run committed a request for a later routing change.
    ///
    /// This is a durable source and outbox entry, never active routing.
    ModelRoutingIntentRequested {
        request_id: SwitchTurnRequestId,
        originating_run_id: RunId,
        intent: SwitchTurnIntent,
    },
    /// The routing change was applied; the resolved identity is recorded.
    ///
    /// `applied_identity` is the identity that was actually installed, resolved
    /// fresh at realization. It is deliberately not constrained to equal
    /// `intent.target_model`: catalog aliasing and provider-side resolution can
    /// legitimately land on a differently-spelled model, and recording what was
    /// really installed is more useful than recording what was asked for. Both
    /// facts are kept so the divergence stays visible.
    ModelRoutingIntentRealized {
        request_id: SwitchTurnRequestId,
        originating_run_id: RunId,
        intent: SwitchTurnIntent,
        applied_identity: Box<crate::SessionLlmIdentity>,
    },
    /// The routing change was refused at its decision point.
    ModelRoutingIntentDenied {
        request_id: SwitchTurnRequestId,
        originating_run_id: RunId,
        intent: SwitchTurnIntent,
        reason: SwitchTurnDenialReason,
    },
    /// The routing change was terminalized at an authoritative session
    /// boundary without ever reaching a decision.
    ModelRoutingIntentAbandoned {
        request_id: SwitchTurnRequestId,
        originating_run_id: RunId,
        intent: SwitchTurnIntent,
        reason: ModelRoutingIntentAbandonReason,
    },
}

impl SessionModelRoutingControlRecord {
    /// Build the canonical request record a committed `brain_swap` stages.
    ///
    /// The intent is validated here so an unrepresentable durable handoff — one
    /// that is not an until-changed, model-origin switch — cannot be committed
    /// in the first place.
    pub fn request(
        request_id: SwitchTurnRequestId,
        originating_run_id: RunId,
        intent: SwitchTurnIntent,
    ) -> Result<Self, ModelRoutingControlAppendError> {
        if !intent_is_durable_handoff(&intent) {
            return Err(ModelRoutingControlAppendError::UnsupportedIntent { request_id });
        }
        Ok(Self::ModelRoutingIntentRequested {
            request_id,
            originating_run_id,
            intent,
        })
    }

    #[must_use]
    pub const fn request_id(&self) -> &SwitchTurnRequestId {
        match self {
            Self::ModelRoutingIntentRequested { request_id, .. }
            | Self::ModelRoutingIntentRealized { request_id, .. }
            | Self::ModelRoutingIntentDenied { request_id, .. }
            | Self::ModelRoutingIntentAbandoned { request_id, .. } => request_id,
        }
    }

    #[must_use]
    pub const fn originating_run_id(&self) -> &RunId {
        match self {
            Self::ModelRoutingIntentRequested {
                originating_run_id, ..
            }
            | Self::ModelRoutingIntentRealized {
                originating_run_id, ..
            }
            | Self::ModelRoutingIntentDenied {
                originating_run_id, ..
            }
            | Self::ModelRoutingIntentAbandoned {
                originating_run_id, ..
            } => originating_run_id,
        }
    }

    #[must_use]
    pub const fn intent(&self) -> &SwitchTurnIntent {
        match self {
            Self::ModelRoutingIntentRequested { intent, .. }
            | Self::ModelRoutingIntentRealized { intent, .. }
            | Self::ModelRoutingIntentDenied { intent, .. }
            | Self::ModelRoutingIntentAbandoned { intent, .. } => intent,
        }
    }

    /// The literal disposition this record expresses.
    #[must_use]
    pub const fn disposition(&self) -> ModelRoutingIntentRecordDisposition {
        match self {
            Self::ModelRoutingIntentRequested { .. } => {
                ModelRoutingIntentRecordDisposition::Requested
            }
            Self::ModelRoutingIntentRealized { .. } => {
                ModelRoutingIntentRecordDisposition::Realized
            }
            Self::ModelRoutingIntentDenied { .. } => ModelRoutingIntentRecordDisposition::Denied,
            Self::ModelRoutingIntentAbandoned { .. } => {
                ModelRoutingIntentRecordDisposition::Abandoned
            }
        }
    }
}

/// Whether an intent is expressible as a durable cross-run handoff.
///
/// Only the until-changed, model-origin switch is: a finite scoped override
/// belongs to the run that requested it and must never outlive it, and a
/// user/system-policy origin has a live control surface of its own.
#[must_use]
pub fn intent_is_durable_handoff(intent: &SwitchTurnIntent) -> bool {
    matches!(intent.duration, SwitchTurnDuration::UntilChanged)
        && matches!(intent.origin, SwitchTurnOrigin::Model { .. })
}

/// The literal disposition of the newest record for one request identity.
///
/// This is representation derivation, not lifecycle status: it reports what the
/// committed log says, and deliberately does not decide whether a request is
/// actionable. Actionability additionally requires an exact committed boundary
/// receipt for the originating run and generated machine authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ModelRoutingIntentRecordDisposition {
    /// A request is recorded and no terminal record followed it.
    Requested,
    /// The request was realized.
    Realized,
    /// The request was denied at its decision point.
    Denied,
    /// The request was abandoned at an authoritative session boundary.
    Abandoned,
}

impl ModelRoutingIntentRecordDisposition {
    /// Whether this disposition ends the request's recorded life.
    ///
    /// `Requested` is the one waiting disposition; everything else is terminal.
    /// Waiting and terminal are distinct variants rather than a boolean field so
    /// no caller can invent a fifth "maybe done" reading.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Requested)
    }

    /// Whether this disposition is still awaiting a decision.
    #[must_use]
    pub const fn is_awaiting_decision(self) -> bool {
        matches!(self, Self::Requested)
    }
}

/// Outcome of appending one record to the committed history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelRoutingControlAppendOutcome {
    /// The record extended the log.
    Appended,
    /// An exactly equal record was already committed; the log is unchanged.
    AlreadyRecorded,
}

/// Fail-closed refusals for an incoherent append.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum ModelRoutingControlAppendError {
    /// The request identity is already bound to a different originating run or
    /// a different typed intent.
    #[error(
        "model-routing intent {request_id:?} is already committed with a different originating run or target"
    )]
    ConflictingIntent { request_id: SwitchTurnRequestId },
    /// The request identity already reached a terminal disposition.
    #[error("model-routing intent {request_id:?} already terminalized as {disposition:?}")]
    AfterTerminal {
        request_id: SwitchTurnRequestId,
        disposition: ModelRoutingIntentRecordDisposition,
    },
    /// A terminal record was offered for a request that was never committed.
    #[error("model-routing intent {request_id:?} has no committed request to terminalize")]
    UnknownRequest { request_id: SwitchTurnRequestId },
    /// The intent cannot be expressed as a durable cross-run handoff.
    #[error("model-routing intent {request_id:?} is not an until-changed model-origin switch")]
    UnsupportedIntent { request_id: SwitchTurnRequestId },
    /// An abandon record was offered without boundary terminalization
    /// authority.
    ///
    /// Abandonment is reachable only through
    /// [`SessionModelRoutingControlHistory::terminalize_awaiting_for_boundary`],
    /// which demands a machine-minted
    /// [`ModelRoutingIntentBoundaryTerminalization`].
    #[error(
        "model-routing intent {request_id:?} cannot be abandoned without machine-minted boundary authority"
    )]
    UnauthorizedAbandon { request_id: SwitchTurnRequestId },
    /// A persisted log carried the same record twice.
    ///
    /// Silently collapsing it would make decode disagree with the bytes, so an
    /// incoherent persisted log fails closed instead of being normalized.
    #[error("persisted model-routing log carries a duplicate record for intent {request_id:?}")]
    DuplicateRecord { request_id: SwitchTurnRequestId },
}

/// Ordered, append-only committed model-routing control history.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionModelRoutingControlHistory {
    records: Vec<SessionModelRoutingControlRecord>,
}

impl SessionModelRoutingControlHistory {
    /// An empty history.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Every committed record, oldest first.
    #[must_use]
    pub fn records(&self) -> &[SessionModelRoutingControlRecord] {
        &self.records
    }

    /// Rebuild a history from persisted records, revalidating coherence.
    ///
    /// Deserialization and every store-side materialization route through this,
    /// so a hand-edited or corrupted log cannot enter memory as authority.
    ///
    /// Strict on purpose: a duplicate in a persisted vector is incoherent input
    /// and is refused. Collapsing it would make the decoded value disagree with
    /// the bytes it came from, which is the fail-OPEN twin of everything else
    /// this module does.
    pub fn from_records(
        records: Vec<SessionModelRoutingControlRecord>,
    ) -> Result<Self, ModelRoutingControlAppendError> {
        let mut history = Self::new();
        for record in records {
            let request_id = *record.request_id();
            match history.append_authorized(record)? {
                ModelRoutingControlAppendOutcome::Appended => {}
                ModelRoutingControlAppendOutcome::AlreadyRecorded => {
                    return Err(ModelRoutingControlAppendError::DuplicateRecord { request_id });
                }
            }
        }
        Ok(history)
    }

    /// Whether this log is a prefix extension of `predecessor`.
    ///
    /// The durable save boundary demands this: a successor may only add
    /// records, never drop, reorder, or rewrite a committed one.
    #[must_use]
    pub fn extends(&self, predecessor: &Self) -> bool {
        self.records.len() >= predecessor.records.len()
            && self.records[..predecessor.records.len()] == predecessor.records[..]
    }

    /// The newest record committed for `request_id`, if any.
    #[must_use]
    pub fn latest_record_for(
        &self,
        request_id: &SwitchTurnRequestId,
    ) -> Option<&SessionModelRoutingControlRecord> {
        self.records
            .iter()
            .rev()
            .find(|record| record.request_id() == request_id)
    }

    /// The literal disposition of `request_id` in the committed log.
    #[must_use]
    pub fn disposition_of(
        &self,
        request_id: &SwitchTurnRequestId,
    ) -> Option<ModelRoutingIntentRecordDisposition> {
        self.latest_record_for(request_id)
            .map(SessionModelRoutingControlRecord::disposition)
    }

    /// Every committed request that has not yet reached a terminal record.
    ///
    /// These are candidates only. A candidate becomes actionable exclusively
    /// when generated authority admits it and an exact committed boundary
    /// receipt proves its originating run committed successfully.
    pub fn awaiting_decision(
        &self,
    ) -> impl Iterator<Item = &SessionModelRoutingControlRecord> + '_ {
        let settled: std::collections::BTreeSet<&SwitchTurnRequestId> = self
            .records
            .iter()
            .filter(|record| record.disposition().is_terminal())
            .map(SessionModelRoutingControlRecord::request_id)
            .collect();
        self.records.iter().filter(move |record| {
            record.disposition().is_awaiting_decision() && !settled.contains(record.request_id())
        })
    }

    /// Append one record, enforcing the log's well-formedness rules.
    ///
    /// Exactly-equal duplicates are idempotent; anything that would make the
    /// committed log ambiguous is refused with a typed error.
    ///
    /// An `Abandoned` record is refused outright: abandonment is a session
    /// boundary decision and is reachable only through
    /// [`Self::terminalize_awaiting_for_boundary`], which demands machine-minted
    /// authority. This is what makes the seal real rather than advisory — a
    /// public enum variant is constructible from any crate, so the *append*
    /// must be the thing that refuses it.
    pub fn append(
        &mut self,
        record: SessionModelRoutingControlRecord,
    ) -> Result<ModelRoutingControlAppendOutcome, ModelRoutingControlAppendError> {
        if matches!(
            record,
            SessionModelRoutingControlRecord::ModelRoutingIntentAbandoned { .. }
        ) {
            return Err(ModelRoutingControlAppendError::UnauthorizedAbandon {
                request_id: *record.request_id(),
            });
        }
        self.append_authorized(record)
    }

    /// Append one already-authorized record.
    ///
    /// Crate-private: the public door is [`Self::append`], which refuses
    /// abandonment. Boundary terminalization and persisted-log revalidation are
    /// the only callers permitted to carry an `Abandoned` record.
    pub(crate) fn append_authorized(
        &mut self,
        record: SessionModelRoutingControlRecord,
    ) -> Result<ModelRoutingControlAppendOutcome, ModelRoutingControlAppendError> {
        let request_id = *record.request_id();
        if !intent_is_durable_handoff(record.intent()) {
            return Err(ModelRoutingControlAppendError::UnsupportedIntent { request_id });
        }
        match self.latest_record_for(&request_id) {
            None => {
                if record.disposition().is_terminal() {
                    return Err(ModelRoutingControlAppendError::UnknownRequest { request_id });
                }
                self.records.push(record);
                Ok(ModelRoutingControlAppendOutcome::Appended)
            }
            Some(existing) => {
                if existing == &record {
                    return Ok(ModelRoutingControlAppendOutcome::AlreadyRecorded);
                }
                if existing.originating_run_id() != record.originating_run_id()
                    || existing.intent() != record.intent()
                {
                    return Err(ModelRoutingControlAppendError::ConflictingIntent { request_id });
                }
                let disposition = existing.disposition();
                if disposition.is_terminal() {
                    return Err(ModelRoutingControlAppendError::AfterTerminal {
                        request_id,
                        disposition,
                    });
                }
                if record.disposition().is_awaiting_decision() {
                    // Unreachable by construction: an exactly-equal request
                    // returned above, and one differing in run or intent was
                    // already refused as a conflict. Fail closed rather than
                    // report a silent idempotent no-op.
                    return Err(ModelRoutingControlAppendError::ConflictingIntent { request_id });
                }
                self.records.push(record);
                Ok(ModelRoutingControlAppendOutcome::Appended)
            }
        }
    }

    /// Terminalize every still-waiting request at an authoritative session
    /// boundary, returning the records that were appended.
    ///
    /// The `authorization` argument is the whole point: only generated
    /// session-document archive authority can mint one, so no surface can
    /// hand-append an abandon record.
    pub fn terminalize_awaiting_for_boundary(
        &mut self,
        authorization: &SessionArchiveControlTerminalizationAuthorization,
    ) -> Vec<SessionModelRoutingControlRecord> {
        let reason = authorization.reason();
        let pending: Vec<SessionModelRoutingControlRecord> = self
            .awaiting_decision()
            .map(
                |record| SessionModelRoutingControlRecord::ModelRoutingIntentAbandoned {
                    request_id: *record.request_id(),
                    originating_run_id: record.originating_run_id().clone(),
                    intent: record.intent().clone(),
                    reason,
                },
            )
            .collect();
        let mut appended = Vec::with_capacity(pending.len());
        for record in pending {
            if matches!(
                self.append_authorized(record.clone()),
                Ok(ModelRoutingControlAppendOutcome::Appended)
            ) {
                appended.push(record);
            }
        }
        appended
    }
}
