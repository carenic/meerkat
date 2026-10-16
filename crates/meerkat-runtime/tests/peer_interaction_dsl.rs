//! W1-A contract tests for the peer-interaction DSL lifecycle.
//!
//! Each test exercises a single typed DSL transition on an ephemeral
//! `RuntimePeerInteractionHandle` sharing a DSL authority bootstrapped to
//! the `Idle` phase (Initialize + RegisterSession). Verifies that
//! `outbound_state` / `inbound_state` advance as the transition describes
//! and (for terminal-shaped transitions) that the projection-cleanup
//! invariant holds — the map entry is removed so downstream subscriber /
//! stream channels can be dropped deterministically.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use meerkat_core::comms::PeerId;
use meerkat_core::handles::{PeerInteractionHandle, PeerTerminalDisposition};
use meerkat_core::{
    InboundPeerRequestState, OutboundPeerRequestState, PeerCorrelationId,
    PeerResponseAddresseeVerdict,
};
use meerkat_runtime::handles::{HandleDslAuthority, RuntimePeerInteractionHandle};
use meerkat_runtime::meerkat_machine::dsl as mm_dsl;

/// The peer every outbound request in these tests is sent to: the only peer
/// whose responses the guarded response transitions accept.
fn addressee() -> PeerId {
    PeerId::from_ed25519_pubkey(&[7u8; 32])
}

/// Build an ephemeral peer-interaction handle with the DSL authority driven
/// into the Idle phase via `Initialize` + `RegisterSession`. Without this
/// bootstrap the authority sits in `Initializing`, where peer-interaction
/// transitions do not match.
fn new_handle() -> RuntimePeerInteractionHandle {
    RuntimePeerInteractionHandle::new(new_registered_authority())
}

fn new_registered_authority() -> Arc<HandleDslAuthority> {
    let dsl = Arc::new(HandleDslAuthority::ephemeral());
    dsl.apply_signal(mm_dsl::MeerkatMachineSignal::Initialize, "test::initialize")
        .expect("Initialize signal");
    dsl.apply_input(
        mm_dsl::MeerkatMachineInput::RegisterSession {
            session_id: mm_dsl::SessionId::from("peer-interaction-test".to_string()),
            runtime_epoch_id: None,
            initial_run_start_holds: std::collections::BTreeSet::new(),
        },
        "test::register_session",
    )
    .expect("RegisterSession input");
    dsl
}

#[test]
fn request_sent_advances_pending_map_to_sent() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    assert!(handle.outbound_state(corr_id).is_none());
    handle.request_sent(corr_id, addressee()).unwrap();
    assert_eq!(
        handle.outbound_state(corr_id),
        Some(OutboundPeerRequestState::Sent)
    );
}

#[test]
fn request_sent_rejects_duplicate() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    handle.request_sent(corr_id, addressee()).unwrap();
    let err = handle
        .request_sent(corr_id, addressee())
        .expect_err("duplicate send must reject");
    assert_eq!(err.context, "PeerInteractionHandle::request_sent");
}

#[test]
fn response_reply_terminality_comes_from_machine_signal() {
    let handle = new_handle();
    assert_eq!(
        handle
            .classify_response_reply(meerkat_core::ResponseStatus::Accepted)
            .unwrap(),
        meerkat_core::TerminalityClass::Progress
    );
    assert_eq!(
        handle
            .classify_response_reply(meerkat_core::ResponseStatus::Completed)
            .unwrap(),
        meerkat_core::TerminalityClass::Terminal {
            disposition: meerkat_core::TerminalDisposition::Completed,
        }
    );
    assert_eq!(
        handle
            .classify_response_reply(meerkat_core::ResponseStatus::Failed)
            .unwrap(),
        meerkat_core::TerminalityClass::Terminal {
            disposition: meerkat_core::TerminalDisposition::Failed,
        }
    );
}

#[test]
fn progress_advances_state_to_accepted() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    handle.request_sent(corr_id, addressee()).unwrap();
    handle.response_progress(corr_id, addressee()).unwrap();
    assert_eq!(
        handle.outbound_state(corr_id),
        Some(OutboundPeerRequestState::AcceptedProgress)
    );
}

#[test]
fn progress_rejects_unknown_corr_id() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    let err = handle
        .response_progress(corr_id, addressee())
        .expect_err("progress on unknown corr_id must reject");
    assert_eq!(err.context, "PeerInteractionHandle::response_progress");
}

#[test]
fn terminal_completed_removes_entry_and_emits_cleanup() {
    // Load-bearing proof that `pending_peer_requests` is a real projection:
    // terminal transition removes the entry so any channel keyed on the same
    // corr_id can be dropped by the shell-side cleanup observer.
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    handle.request_sent(corr_id, addressee()).unwrap();
    handle
        .response_terminal(corr_id, PeerTerminalDisposition::Completed, addressee())
        .unwrap();
    // Map entry gone — second terminal is rejected by the `pending_exists`
    // guard (structural shape of the cleanup effect).
    assert!(handle.outbound_state(corr_id).is_none());
    let err = handle
        .response_terminal(corr_id, PeerTerminalDisposition::Completed, addressee())
        .expect_err("second terminal must reject");
    assert_eq!(err.context, "PeerInteractionHandle::response_terminal");
}

#[test]
fn terminal_failed_removes_entry() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    handle.request_sent(corr_id, addressee()).unwrap();
    handle
        .response_terminal(corr_id, PeerTerminalDisposition::Failed, addressee())
        .unwrap();
    assert!(handle.outbound_state(corr_id).is_none());
}

#[test]
fn response_rejected_removes_entry() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    handle.request_sent(corr_id, addressee()).unwrap();
    handle.response_rejected(corr_id, addressee()).unwrap();
    assert!(handle.outbound_state(corr_id).is_none());
}

#[test]
fn response_rejected_rejects_unknown_corr_id() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    let err = handle
        .response_rejected(corr_id, addressee())
        .expect_err("rejection on unknown corr_id must reject");
    assert_eq!(err.context, "PeerInteractionHandle::response_rejected");
}

#[test]
fn timeout_removes_entry_and_emits_cleanup() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    handle.request_sent(corr_id, addressee()).unwrap();
    handle.request_timed_out(corr_id).unwrap();
    assert!(handle.outbound_state(corr_id).is_none());
    // Second timeout rejects — cleanup is exactly once.
    let err = handle
        .request_timed_out(corr_id)
        .expect_err("second timeout must reject");
    assert_eq!(err.context, "PeerInteractionHandle::request_timed_out");
}

#[test]
fn inbound_received_then_replied_advances_and_removes() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    assert!(handle.inbound_state(corr_id).is_none());
    handle
        .request_received(
            corr_id,
            meerkat_core::types::HandlingMode::Queue,
            PeerId::new(),
        )
        .unwrap();
    assert_eq!(
        handle.inbound_state(corr_id),
        Some(InboundPeerRequestState::Received)
    );
    assert_eq!(
        handle.inbound_handling_mode(corr_id),
        Some(meerkat_core::types::HandlingMode::Queue)
    );
    handle.response_replied(corr_id).unwrap();
    assert!(handle.inbound_state(corr_id).is_none());
    assert!(handle.inbound_handling_mode(corr_id).is_none());
}

#[test]
fn inbound_received_rejects_duplicate() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    handle
        .request_received(
            corr_id,
            meerkat_core::types::HandlingMode::Queue,
            PeerId::new(),
        )
        .unwrap();
    let err = handle
        .request_received(
            corr_id,
            meerkat_core::types::HandlingMode::Queue,
            PeerId::new(),
        )
        .expect_err("duplicate inbound receipt must reject");
    assert_eq!(err.context, "PeerInteractionHandle::request_received");
}

#[test]
fn inbound_replied_rejects_unknown_corr_id() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    let err = handle
        .response_replied(corr_id)
        .expect_err("reply on unknown corr_id must reject");
    assert_eq!(err.context, "PeerInteractionHandle::response_replied");
}

#[test]
fn cleanup_observer_fires_on_terminal_transitions() {
    use meerkat_core::handles::PeerInteractionCleanupObserver;
    use std::sync::Mutex;

    struct Recorder(Mutex<Vec<PeerCorrelationId>>);
    impl PeerInteractionCleanupObserver for Recorder {
        fn on_peer_interaction_cleanup(&self, corr_id: PeerCorrelationId) {
            self.0.lock().unwrap().push(corr_id);
        }
    }

    let handle = new_handle();
    let rec = Arc::new(Recorder(Mutex::new(Vec::new())));
    handle.install_cleanup_observer(Arc::clone(&rec) as Arc<dyn PeerInteractionCleanupObserver>);

    // Sent / Progress do NOT fire cleanup.
    let a = PeerCorrelationId::new();
    handle.request_sent(a, addressee()).unwrap();
    handle.response_progress(a, addressee()).unwrap();
    assert!(
        rec.0.lock().unwrap().is_empty(),
        "non-terminal transitions must not fire cleanup"
    );

    // Terminal Completed fires cleanup once.
    handle
        .response_terminal(a, PeerTerminalDisposition::Completed, addressee())
        .unwrap();
    assert_eq!(rec.0.lock().unwrap().clone(), vec![a]);

    // Terminal Failed fires cleanup once.
    let b = PeerCorrelationId::new();
    handle.request_sent(b, addressee()).unwrap();
    handle
        .response_terminal(b, PeerTerminalDisposition::Failed, addressee())
        .unwrap();
    assert_eq!(rec.0.lock().unwrap().clone(), vec![a, b]);

    // TimedOut fires cleanup once.
    let c = PeerCorrelationId::new();
    handle.request_sent(c, addressee()).unwrap();
    handle.request_timed_out(c).unwrap();
    assert_eq!(rec.0.lock().unwrap().clone(), vec![a, b, c]);

    // Inbound reply does NOT fire `PeerInteractionCleanup` (it emits the
    // inbound state-change effect on a different variant).
    let d = PeerCorrelationId::new();
    handle
        .request_received(d, meerkat_core::types::HandlingMode::Steer, PeerId::new())
        .unwrap();
    assert_eq!(
        handle.inbound_handling_mode(d),
        Some(meerkat_core::types::HandlingMode::Steer)
    );
    handle.response_replied(d).unwrap();
    assert_eq!(
        rec.0.lock().unwrap().clone(),
        vec![a, b, c],
        "inbound lifecycle does not emit PeerInteractionCleanup"
    );
}

#[test]
fn cleanup_observer_is_weakly_held_no_arc_cycle() {
    // P1 regression guard: the runtime impl stores the cleanup observer as
    // a `Weak`, so installing it does not keep the owner alive. Drop the
    // strong `Arc` and confirm subsequent cleanup fires no longer reach the
    // observer — i.e., the handle held the Weak, the strong count dropped
    // to zero with the `Arc`, and the handle's `upgrade()` now returns
    // `None`. Without the fix this test would either keep the recorder
    // alive (and see new records) or, in the production cycle, leak the
    // `CommsRuntime`.
    use meerkat_core::handles::PeerInteractionCleanupObserver;
    use std::sync::Mutex;

    struct Recorder(Mutex<Vec<PeerCorrelationId>>);
    impl PeerInteractionCleanupObserver for Recorder {
        fn on_peer_interaction_cleanup(&self, corr_id: PeerCorrelationId) {
            self.0.lock().unwrap().push(corr_id);
        }
    }

    let handle = new_handle();
    let rec: Arc<Recorder> = Arc::new(Recorder(Mutex::new(Vec::new())));
    let rec_weak = Arc::downgrade(&rec);
    handle.install_cleanup_observer(Arc::clone(&rec) as Arc<dyn PeerInteractionCleanupObserver>);
    assert_eq!(
        Arc::strong_count(&rec),
        1,
        "handle must not hold a strong ref"
    );

    // Drop the caller's last strong ref; observer should vanish.
    drop(rec);
    assert!(
        rec_weak.upgrade().is_none(),
        "observer must be fully dropped once the caller releases its Arc — no cycle"
    );

    // A terminal transition after observer drop must still succeed on the
    // DSL side and must not panic in the dispatch path when the weak fails
    // to upgrade.
    let corr_id = PeerCorrelationId::new();
    handle.request_sent(corr_id, addressee()).unwrap();
    handle
        .response_terminal(corr_id, PeerTerminalDisposition::Completed, addressee())
        .expect("terminal transition must succeed with dropped observer");
    assert!(handle.outbound_state(corr_id).is_none());
}

#[test]
fn outbound_inbound_are_independent_namespaces() {
    // Mixing outbound and inbound on the same corr_id must not alias.
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    handle.request_sent(corr_id, addressee()).unwrap();
    handle
        .request_received(
            corr_id,
            meerkat_core::types::HandlingMode::Queue,
            PeerId::new(),
        )
        .unwrap();
    assert_eq!(
        handle.outbound_state(corr_id),
        Some(OutboundPeerRequestState::Sent)
    );
    assert_eq!(
        handle.inbound_state(corr_id),
        Some(InboundPeerRequestState::Received)
    );
    handle
        .response_terminal(corr_id, PeerTerminalDisposition::Completed, addressee())
        .unwrap();
    // Inbound entry is untouched by outbound terminal.
    assert!(handle.outbound_state(corr_id).is_none());
    assert_eq!(
        handle.inbound_state(corr_id),
        Some(InboundPeerRequestState::Received)
    );
}

fn resolve(
    handle: &RuntimePeerInteractionHandle,
    corr_id: PeerCorrelationId,
    addressee: PeerId,
) -> PeerResponseAddresseeVerdict {
    handle
        .resolve_response_addressee(corr_id, addressee)
        .expect("the generated authority judges every addressee")
}

#[test]
fn inbound_request_records_its_requester_and_judges_response_addressees() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    let requester = PeerId::new();
    let bystander = PeerId::new();

    assert_eq!(
        resolve(&handle, corr_id, requester),
        PeerResponseAddresseeVerdict::UnknownRequest,
        "no request is recorded yet"
    );

    handle
        .request_received(corr_id, meerkat_core::types::HandlingMode::Queue, requester)
        .unwrap();
    assert_eq!(
        resolve(&handle, corr_id, requester),
        PeerResponseAddresseeVerdict::Requester
    );
    assert_eq!(
        resolve(&handle, corr_id, bystander),
        PeerResponseAddresseeVerdict::NotRequester
    );
    assert_eq!(
        resolve(&handle, PeerCorrelationId::new(), requester),
        PeerResponseAddresseeVerdict::UnknownRequest,
        "a requester answers only for its own request id"
    );
    assert_eq!(
        handle.inbound_state(corr_id),
        Some(InboundPeerRequestState::Received),
        "judging an addressee is read-only: the request stays answerable"
    );
    assert_eq!(
        resolve(&handle, corr_id, requester),
        PeerResponseAddresseeVerdict::Requester,
        "a refused addressee does not consume the request"
    );

    handle.response_replied(corr_id).unwrap();
    assert_eq!(
        resolve(&handle, corr_id, requester),
        PeerResponseAddresseeVerdict::UnknownRequest,
        "the terminal reply removes the requester with the request"
    );
}

#[test]
fn duplicate_inbound_request_keeps_its_first_requester() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    let requester = PeerId::new();
    let reuser = PeerId::new();
    handle
        .request_received(corr_id, meerkat_core::types::HandlingMode::Queue, requester)
        .unwrap();
    handle
        .request_received(corr_id, meerkat_core::types::HandlingMode::Queue, reuser)
        .expect_err("a second receipt of the same id must reject");
    assert_eq!(
        resolve(&handle, corr_id, requester),
        PeerResponseAddresseeVerdict::Requester
    );
    assert_eq!(
        resolve(&handle, corr_id, reuser),
        PeerResponseAddresseeVerdict::NotRequester,
        "reusing a request id never rebinds its requester"
    );
}

fn assert_requesters_match_requests(dsl: &HandleDslAuthority, step: &str) {
    let state = dsl.snapshot_state();
    assert_eq!(
        state
            .inbound_peer_request_requesters
            .keys()
            .collect::<Vec<_>>(),
        state.inbound_peer_requests.keys().collect::<Vec<_>>(),
        "every recorded inbound request has exactly its requester ({step})"
    );
}

#[test]
fn inbound_requesters_share_their_keys_with_inbound_requests() {
    let dsl = new_registered_authority();
    let handle = RuntimePeerInteractionHandle::new(Arc::clone(&dsl));
    let first = PeerCorrelationId::new();
    let second = PeerCorrelationId::new();
    assert_requesters_match_requests(&dsl, "registered");
    handle
        .request_received(
            first,
            meerkat_core::types::HandlingMode::Queue,
            PeerId::new(),
        )
        .unwrap();
    handle
        .request_received(
            second,
            meerkat_core::types::HandlingMode::Steer,
            PeerId::new(),
        )
        .unwrap();
    assert_requesters_match_requests(&dsl, "two received");
    assert_eq!(
        dsl.snapshot_state().inbound_peer_request_requesters.len(),
        2
    );
    handle.response_replied(first).unwrap();
    assert_requesters_match_requests(&dsl, "one replied");
    handle.response_replied(second).unwrap();
    assert_requesters_match_requests(&dsl, "both replied");
    assert!(
        dsl.snapshot_state()
            .inbound_peer_request_requesters
            .is_empty()
    );
}

/// The keys-equal invariant is generated authority, not a test convention:
/// recovered state with a request but no requester, or a requester but no
/// request, is refused.
#[test]
fn recovered_state_refuses_an_inbound_request_without_its_requester() {
    let registered = || mm_dsl::MeerkatMachineState {
        lifecycle_phase: mm_dsl::MeerkatPhase::Idle,
        session_id: Some(mm_dsl::SessionId("peer-interaction-test".to_string())),
        ..Default::default()
    };
    let corr_id = mm_dsl::PeerCorrelationId::from(PeerCorrelationId::new());
    let requester = mm_dsl::PeerId(PeerId::new().to_string());

    let mut without_requester = registered();
    without_requester
        .inbound_peer_requests
        .insert(corr_id.clone(), mm_dsl::InboundPeerRequestState::Received);
    assert!(
        mm_dsl::MeerkatMachineAuthority::recover_from_state(without_requester).is_err(),
        "an inbound request without a requester is not a recoverable state"
    );

    let mut without_request = registered();
    without_request
        .inbound_peer_request_requesters
        .insert(corr_id.clone(), requester.clone());
    assert!(
        mm_dsl::MeerkatMachineAuthority::recover_from_state(without_request).is_err(),
        "a requester without an inbound request is not a recoverable state"
    );

    let mut matched = registered();
    matched
        .inbound_peer_requests
        .insert(corr_id.clone(), mm_dsl::InboundPeerRequestState::Received);
    matched
        .inbound_peer_request_requesters
        .insert(corr_id, requester);
    assert!(
        mm_dsl::MeerkatMachineAuthority::recover_from_state(matched).is_ok(),
        "a request with its requester is recoverable"
    );
}

/// A response from any peer but the addressee recorded at send moves nothing:
/// progress, terminal and rejection are all refused by generated authority,
/// and the request stays open for its addressee, whose responses apply.
#[test]
fn a_response_from_another_peer_is_refused_and_leaves_the_request_open() {
    let handle = new_handle();
    let corr_id = PeerCorrelationId::new();
    let other = PeerId::from_ed25519_pubkey(&[9u8; 32]);
    handle.request_sent(corr_id, addressee()).unwrap();

    let progress = handle
        .response_progress(corr_id, other)
        .expect_err("progress from another peer is refused");
    assert_eq!(progress.context, "PeerInteractionHandle::response_progress");
    for disposition in [
        PeerTerminalDisposition::Completed,
        PeerTerminalDisposition::Failed,
    ] {
        let terminal = handle
            .response_terminal(corr_id, disposition, other)
            .expect_err("a terminal response from another peer is refused");
        assert_eq!(terminal.context, "PeerInteractionHandle::response_terminal");
    }
    let rejected = handle
        .response_rejected(corr_id, other)
        .expect_err("another peer's response observation cannot cancel the request");
    assert_eq!(rejected.context, "PeerInteractionHandle::response_rejected");
    assert_eq!(
        handle.outbound_state(corr_id),
        Some(OutboundPeerRequestState::Sent),
        "refused frames leave the request open"
    );

    handle.response_progress(corr_id, addressee()).unwrap();
    assert_eq!(
        handle.outbound_state(corr_id),
        Some(OutboundPeerRequestState::AcceptedProgress)
    );
    handle
        .response_terminal(corr_id, PeerTerminalDisposition::Completed, addressee())
        .unwrap();
    assert!(handle.outbound_state(corr_id).is_none());
}

/// The ingress verdict names only the recorded addressee, and every way the
/// request ends drops it: a later frame for the id is `UnknownRequest`.
#[test]
fn the_response_sender_verdict_accepts_only_the_recorded_addressee() {
    use meerkat_core::PeerResponseSenderVerdict;
    use meerkat_core::handles::PeerCommsHandle;
    use meerkat_runtime::handles::RuntimePeerCommsHandle;

    let dsl = new_registered_authority();
    let handle = RuntimePeerInteractionHandle::new(Arc::clone(&dsl));
    let ingress = RuntimePeerCommsHandle::new(Arc::clone(&dsl));
    let other = PeerId::from_ed25519_pubkey(&[9u8; 32]);
    let verdict = |corr_id, responder| {
        ingress
            .resolve_peer_response_sender(corr_id, responder)
            .expect("the machine judges the response sender")
    };

    let completed = PeerCorrelationId::new();
    assert_eq!(
        verdict(completed, addressee()),
        PeerResponseSenderVerdict::UnknownRequest,
        "an id this session never sent is unknown"
    );
    handle.request_sent(completed, addressee()).unwrap();
    assert_eq!(
        verdict(completed, addressee()),
        PeerResponseSenderVerdict::Addressee
    );
    assert_eq!(
        verdict(completed, other),
        PeerResponseSenderVerdict::NotAddressee
    );
    handle
        .response_terminal(completed, PeerTerminalDisposition::Completed, addressee())
        .unwrap();
    assert_eq!(
        verdict(completed, addressee()),
        PeerResponseSenderVerdict::UnknownRequest,
        "an answered request is unknown"
    );

    let timed_out = PeerCorrelationId::new();
    handle.request_sent(timed_out, addressee()).unwrap();
    handle.request_timed_out(timed_out).unwrap();
    assert_eq!(
        verdict(timed_out, addressee()),
        PeerResponseSenderVerdict::UnknownRequest,
        "a timed-out request is unknown"
    );

    let send_failed = PeerCorrelationId::new();
    handle.request_sent(send_failed, addressee()).unwrap();
    handle.request_send_failed(send_failed).unwrap();
    assert_eq!(
        verdict(send_failed, addressee()),
        PeerResponseSenderVerdict::UnknownRequest,
        "a request whose send failed is unknown"
    );

    let rejected = PeerCorrelationId::new();
    handle.request_sent(rejected, addressee()).unwrap();
    handle.response_rejected(rejected, addressee()).unwrap();
    assert_eq!(
        verdict(rejected, addressee()),
        PeerResponseSenderVerdict::UnknownRequest,
        "a rejected response observation ends the request"
    );
    assert!(
        dsl.snapshot_state()
            .pending_peer_request_addressees
            .is_empty(),
        "every ended request drops its addressee"
    );
}

/// The keys-equal invariant is generated authority: recovered state with an
/// outbound request but no addressee, or an addressee but no request, is
/// refused.
#[test]
fn recovered_state_refuses_an_outbound_request_without_its_addressee() {
    let registered = || mm_dsl::MeerkatMachineState {
        lifecycle_phase: mm_dsl::MeerkatPhase::Idle,
        session_id: Some(mm_dsl::SessionId("peer-interaction-test".to_string())),
        ..Default::default()
    };
    let corr_id = mm_dsl::PeerCorrelationId::from(PeerCorrelationId::new());
    let peer = mm_dsl::PeerId(addressee().to_string());

    let mut without_addressee = registered();
    without_addressee
        .pending_peer_requests
        .insert(corr_id.clone(), mm_dsl::OutboundPeerRequestState::Sent);
    assert!(
        mm_dsl::MeerkatMachineAuthority::recover_from_state(without_addressee).is_err(),
        "an outbound request without an addressee is not a recoverable state"
    );

    let mut without_request = registered();
    without_request
        .pending_peer_request_addressees
        .insert(corr_id.clone(), peer.clone());
    assert!(
        mm_dsl::MeerkatMachineAuthority::recover_from_state(without_request).is_err(),
        "an addressee without an outbound request is not a recoverable state"
    );

    let mut matched = registered();
    matched
        .pending_peer_requests
        .insert(corr_id.clone(), mm_dsl::OutboundPeerRequestState::Sent);
    matched
        .pending_peer_request_addressees
        .insert(corr_id, peer);
    assert!(
        mm_dsl::MeerkatMachineAuthority::recover_from_state(matched).is_ok(),
        "a request with its addressee is recoverable"
    );
}
