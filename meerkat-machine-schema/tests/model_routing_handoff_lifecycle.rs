//! Generated model-routing handoff lifecycle: convergence and conflict.
//!
//! The handoff lifecycle exists because a permanent routing change survives the
//! run that asked for it, which means it must survive crashes, retries, and
//! competing claims. That safety rests on exactly two properties, and neither
//! is expressible as "the shell checks first":
//!
//! * a retry of the SAME request for the SAME target from the SAME originating
//!   run converges — it does not rotate identity a second time;
//! * the same request id naming a DIFFERENT target or a DIFFERENT originating
//!   run is refused by the machine, not reinterpreted.
//!
//! These are asserted against the generated kernel directly, so they hold
//! regardless of which shell drives it.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use meerkat_machine_schema::catalog::dsl::meerkat_machine::{
    MeerkatMachineAuthority, MeerkatMachineInput, MeerkatMachineMutator, MeerkatMachineSignal,
    RoutingHandoffPhase, SessionId,
};

const REQUEST: &str = "req-1";
const RUN: &str = "run-1";
const TARGET: &str = "model-b";

/// A registered, idle machine — the state the pre-dequeue seam actually
/// observes, because it runs between turns.
fn registered_authority() -> MeerkatMachineAuthority {
    let mut authority = MeerkatMachineAuthority::new();
    authority
        .apply_signal(MeerkatMachineSignal::Initialize)
        .expect("machine initializes");
    MeerkatMachineMutator::apply(
        &mut authority,
        MeerkatMachineInput::RegisterSession {
            session_id: SessionId("session-1".to_string()),
            runtime_epoch_id: None,
        },
    )
    .expect("session registers");
    authority
}

fn import(
    authority: &mut MeerkatMachineAuthority,
    request_id: &str,
    run: &str,
    target: &str,
) -> Result<(), String> {
    MeerkatMachineMutator::apply(
        authority,
        MeerkatMachineInput::ImportCommittedModelRoutingHandoff {
            request_id: request_id.to_string(),
            originating_run_id: run.to_string(),
            target_model: target.to_string(),
        },
    )
    .map(|_| ())
    .map_err(|error| format!("{error:?}"))
}

fn claim(
    authority: &mut MeerkatMachineAuthority,
    request_id: &str,
    run: &str,
    target: &str,
) -> Result<(), String> {
    MeerkatMachineMutator::apply(
        authority,
        MeerkatMachineInput::ClaimModelRoutingHandoff {
            request_id: request_id.to_string(),
            originating_run_id: run.to_string(),
            target_model: target.to_string(),
        },
    )
    .map(|_| ())
    .map_err(|error| format!("{error:?}"))
}

fn realize(
    authority: &mut MeerkatMachineAuthority,
    request_id: &str,
    run: &str,
    target: &str,
    applied: &str,
) -> Result<(), String> {
    MeerkatMachineMutator::apply(
        authority,
        MeerkatMachineInput::RealizeModelRoutingHandoff {
            request_id: request_id.to_string(),
            originating_run_id: run.to_string(),
            target_model: target.to_string(),
            applied_model: applied.to_string(),
        },
    )
    .map(|_| ())
    .map_err(|error| format!("{error:?}"))
}

fn phase(authority: &MeerkatMachineAuthority, request_id: &str) -> Option<RoutingHandoffPhase> {
    authority
        .state()
        .model_routing_handoff_phase
        .get(request_id)
        .copied()
}

#[test]
fn import_then_claim_then_realize_walks_the_lifecycle() {
    let mut authority = registered_authority();
    import(&mut authority, REQUEST, RUN, TARGET).expect("import");
    assert_eq!(
        phase(&authority, REQUEST),
        Some(RoutingHandoffPhase::Imported)
    );
    claim(&mut authority, REQUEST, RUN, TARGET).expect("claim");
    assert_eq!(
        phase(&authority, REQUEST),
        Some(RoutingHandoffPhase::Claimed)
    );
    realize(&mut authority, REQUEST, RUN, TARGET, TARGET).expect("realize");
    assert_eq!(
        phase(&authority, REQUEST),
        Some(RoutingHandoffPhase::Realized)
    );
    assert_eq!(
        authority
            .state()
            .model_routing_handoff_applied_model
            .get(REQUEST)
            .map(String::as_str),
        Some(TARGET),
        "the resolved identity that was actually installed must be recorded"
    );
}

/// The steady state: the durable log is append-only, so every later lap
/// re-observes the same committed record until it terminalizes. Re-importing
/// must be a no-op, not a second request.
#[test]
fn re_importing_the_identical_committed_fact_converges() {
    let mut authority = registered_authority();
    import(&mut authority, REQUEST, RUN, TARGET).expect("first import");
    import(&mut authority, REQUEST, RUN, TARGET).expect("identical re-import converges");
    assert_eq!(
        phase(&authority, REQUEST),
        Some(RoutingHandoffPhase::Imported),
        "a repeat observation must not move the lifecycle"
    );
}

/// A claim interrupted before realization is retried on the next lap. The
/// retry must converge rather than attempt a second rotation.
#[test]
fn re_claiming_the_same_request_converges_without_rotating_twice() {
    let mut authority = registered_authority();
    import(&mut authority, REQUEST, RUN, TARGET).expect("import");
    claim(&mut authority, REQUEST, RUN, TARGET).expect("claim");
    claim(&mut authority, REQUEST, RUN, TARGET).expect("re-claim converges");
    assert_eq!(
        phase(&authority, REQUEST),
        Some(RoutingHandoffPhase::Claimed)
    );
}

/// After realization, a replayed claim must report already-exact instead of
/// re-entering the applying state.
#[test]
fn claiming_an_already_realized_request_converges_already_exact() {
    let mut authority = registered_authority();
    import(&mut authority, REQUEST, RUN, TARGET).expect("import");
    claim(&mut authority, REQUEST, RUN, TARGET).expect("claim");
    realize(&mut authority, REQUEST, RUN, TARGET, TARGET).expect("realize");
    claim(&mut authority, REQUEST, RUN, TARGET).expect("claim after realize converges");
    assert_eq!(
        phase(&authority, REQUEST),
        Some(RoutingHandoffPhase::Realized),
        "convergence must not walk a realized request back to claimed"
    );
}

/// Replaying the exact realization is idempotent — the crash window between
/// the durable record and the generated terminal is expected.
#[test]
fn re_realizing_the_same_identity_is_idempotent() {
    let mut authority = registered_authority();
    import(&mut authority, REQUEST, RUN, TARGET).expect("import");
    claim(&mut authority, REQUEST, RUN, TARGET).expect("claim");
    realize(&mut authority, REQUEST, RUN, TARGET, TARGET).expect("realize");
    realize(&mut authority, REQUEST, RUN, TARGET, TARGET).expect("replayed realize converges");
    assert_eq!(
        phase(&authority, REQUEST),
        Some(RoutingHandoffPhase::Realized)
    );
}

/// The conflict case. A matching request id alone must never be enough to move
/// a handoff that belongs to a different target.
#[test]
fn the_same_request_id_with_a_different_target_is_refused() {
    let mut authority = registered_authority();
    import(&mut authority, REQUEST, RUN, TARGET).expect("import");
    let error = import(&mut authority, REQUEST, RUN, "model-c")
        .expect_err("a contradictory target must be refused");
    assert!(
        !error.is_empty(),
        "the kernel must reject rather than absorb the contradiction"
    );
    assert_eq!(
        authority
            .state()
            .model_routing_handoff_target
            .get(REQUEST)
            .map(String::as_str),
        Some(TARGET),
        "the refused import must not have mutated the committed target"
    );
}

/// Same identity, different originating run: also a conflict. A request is
/// only actionable because a specific run committed it.
#[test]
fn the_same_request_id_from_a_different_run_is_refused() {
    let mut authority = registered_authority();
    import(&mut authority, REQUEST, RUN, TARGET).expect("import");
    import(&mut authority, REQUEST, "run-2", TARGET)
        .expect_err("a different originating run must be refused");
    assert_eq!(
        authority
            .state()
            .model_routing_handoff_run
            .get(REQUEST)
            .map(String::as_str),
        Some(RUN)
    );
}

/// Claiming without importing is refused: a claim is a statement about a
/// committed record, and there is none.
#[test]
fn claiming_an_unimported_request_is_refused() {
    let mut authority = registered_authority();
    claim(&mut authority, REQUEST, RUN, TARGET).expect_err("nothing committed to claim");
    assert_eq!(phase(&authority, REQUEST), None);
}

/// Realizing without claiming is refused: realization marks a fact the claim
/// path is responsible for producing.
#[test]
fn realizing_an_unclaimed_request_is_refused() {
    let mut authority = registered_authority();
    import(&mut authority, REQUEST, RUN, TARGET).expect("import");
    realize(&mut authority, REQUEST, RUN, TARGET, TARGET)
        .expect_err("an unclaimed request cannot be realized");
    assert_eq!(
        phase(&authority, REQUEST),
        Some(RoutingHandoffPhase::Imported)
    );
}

/// Archive terminality is generated status whose durable mechanical mirror is
/// written by the archive chokepoint. It is admitted only after the session
/// itself reached generated Retired and is idempotent there.
#[test]
fn archiving_a_pending_handoff_requires_retired_and_is_idempotent() {
    let mut authority = registered_authority();
    import(&mut authority, REQUEST, RUN, TARGET).expect("import");
    MeerkatMachineMutator::apply(
        &mut authority,
        MeerkatMachineInput::ArchiveUnresolvedModelRoutingHandoff {
            request_id: REQUEST.to_string(),
        },
    )
    .expect_err("an active session cannot mint an archived handoff");
    assert_eq!(
        phase(&authority, REQUEST),
        Some(RoutingHandoffPhase::Imported)
    );
    MeerkatMachineMutator::apply(
        &mut authority,
        MeerkatMachineInput::Retire {
            session_id: SessionId("session-1".to_string()),
        },
    )
    .expect("session retires before handoff archive");
    for _ in 0..2 {
        MeerkatMachineMutator::apply(
            &mut authority,
            MeerkatMachineInput::ArchiveUnresolvedModelRoutingHandoff {
                request_id: REQUEST.to_string(),
            },
        )
        .expect("archive converges");
    }
    assert_eq!(
        phase(&authority, REQUEST),
        Some(RoutingHandoffPhase::Archived)
    );
}

/// A realized handoff is terminal for archive too: archiving must not walk it
/// back, because the routing change already happened.
#[test]
fn archiving_a_realized_handoff_is_refused() {
    let mut authority = registered_authority();
    import(&mut authority, REQUEST, RUN, TARGET).expect("import");
    claim(&mut authority, REQUEST, RUN, TARGET).expect("claim");
    realize(&mut authority, REQUEST, RUN, TARGET, TARGET).expect("realize");
    MeerkatMachineMutator::apply(
        &mut authority,
        MeerkatMachineInput::ArchiveUnresolvedModelRoutingHandoff {
            request_id: REQUEST.to_string(),
        },
    )
    .expect_err("a realized handoff is not unresolved");
    assert_eq!(
        phase(&authority, REQUEST),
        Some(RoutingHandoffPhase::Realized)
    );
}

/// An unregistered machine holds no session, so it can carry no handoff.
#[test]
fn an_unregistered_machine_refuses_to_import() {
    let mut authority = MeerkatMachineAuthority::new();
    authority
        .apply_signal(MeerkatMachineSignal::Initialize)
        .expect("machine initializes");
    import(&mut authority, REQUEST, RUN, TARGET)
        .expect_err("an unregistered session cannot own a handoff");
}
