#![allow(clippy::expect_used, clippy::panic)]

//! Live-context results follow the bootstrap summary only, and readiness and
//! authorization must never disagree.
//!
//! `ObserveLiveContextDeliveryReadiness` reports a channel Ready when its
//! context preparation is absent or provider-acknowledged, and the release
//! task asks for authorization once a channel is Ready. Every arm of
//! `AuthorizeLiveDelegationResultDelivery` and `AuthorizeLiveBridgeSubmission`
//! therefore guards on that same condition, `bootstrap_summary_is_acknowledged`.
//! When those guards also required the queued context outbox to drain,
//! readiness and authorization disagreed and the release task spun on refused
//! authorizations (#1597).
//!
//! This contract compares, as schema expressions, the guard of every arm
//! those two inputs trigger with the Ready condition of every readiness arm.
//! Its mutants show that a one-sided change on either side is refused.

use meerkat_machine_schema::catalog::dsl::dsl_meerkat_machine;
use meerkat_machine_schema::identity::FieldId;
use meerkat_machine_schema::{Expr, Guard, MachineSchema, TransitionSchema, TriggerMatch};

const READINESS_INPUT: &str = "ObserveLiveContextDeliveryReadiness";
const READINESS_EFFECT: &str = "LiveContextDeliveryReadinessObserved";
const READINESS_FIELD: &str = "readiness";
const READINESS_ENUM: &str = "LiveContextDeliveryReadiness";
const AUTHORIZATION_INPUTS: [&str; 2] = [
    "AuthorizeLiveDelegationResultDelivery",
    "AuthorizeLiveBridgeSubmission",
];
const READY_GUARD: &str = "bootstrap_summary_is_acknowledged";

fn triggered_by(transition: &TransitionSchema, input: &str) -> bool {
    matches!(&transition.on, TriggerMatch::Input { variant, .. } if variant.as_str() == input)
}

fn is_ready(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::NamedVariant { enum_name, variant }
            if enum_name.as_str() == READINESS_ENUM && variant.as_str() == "Ready"
    )
}

/// The condition of the `if` whose branch yields `Ready`.
fn ready_branch(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::IfElse {
            condition,
            then_expr,
            else_expr,
        } => {
            if is_ready(then_expr) {
                Some(condition.as_ref())
            } else {
                ready_branch(then_expr).or_else(|| ready_branch(else_expr))
            }
        }
        _ => None,
    }
}

fn ready_branch_mut(expr: &mut Expr) -> Option<&mut Expr> {
    match expr {
        Expr::IfElse {
            condition,
            then_expr,
            else_expr,
        } => {
            if is_ready(then_expr) {
                Some(condition.as_mut())
            } else {
                ready_branch_mut(then_expr).or_else(|| ready_branch_mut(else_expr))
            }
        }
        _ => None,
    }
}

fn readiness_field() -> FieldId {
    FieldId::parse(READINESS_FIELD).expect("readiness field id")
}

/// The readiness the arm reports.
fn reported_readiness(arm: &TransitionSchema) -> &Expr {
    arm.emit
        .iter()
        .find(|emit| emit.variant.as_str() == READINESS_EFFECT)
        .and_then(|emit| emit.fields.get(&readiness_field()))
        .unwrap_or_else(|| {
            panic!(
                "{} reports no {READINESS_EFFECT} readiness",
                arm.name.as_str()
            )
        })
}

/// The one Ready condition of every readiness arm.
fn ready_condition(machine: &MachineSchema) -> Expr {
    let conditions: Vec<(&str, &Expr)> = machine
        .transitions
        .iter()
        .filter(|arm| triggered_by(arm, READINESS_INPUT))
        .map(|arm| {
            let condition = ready_branch(reported_readiness(arm))
                .unwrap_or_else(|| panic!("{} reports no Ready", arm.name.as_str()));
            (arm.name.as_str(), condition)
        })
        .collect();
    let Some((_, first)) = conditions.first() else {
        panic!("MeerkatMachine has no {READINESS_INPUT} arm");
    };
    for (arm, condition) in &conditions {
        assert_eq!(
            condition, first,
            "{arm} reports Ready under another condition than the other readiness arms"
        );
    }
    (*first).clone()
}

fn authorization_arms(machine: &MachineSchema) -> impl Iterator<Item = &TransitionSchema> {
    machine.transitions.iter().filter(|arm| {
        AUTHORIZATION_INPUTS
            .iter()
            .any(|input| triggered_by(arm, input))
    })
}

/// Authorization arms whose readiness guard is missing or differs from the
/// Ready condition.
fn readiness_guard_drift(machine: &MachineSchema) -> Vec<String> {
    let ready = ready_condition(machine);
    authorization_arms(machine)
        .filter(|arm| {
            !arm.guards
                .iter()
                .any(|guard| guard.name == READY_GUARD && guard.expr == ready)
        })
        .map(|arm| arm.name.as_str().to_owned())
        .collect()
}

fn arm<'a>(machine: &'a mut MachineSchema, name: &str) -> &'a mut TransitionSchema {
    machine
        .transitions
        .iter_mut()
        .find(|transition| transition.name.as_str() == name)
        .unwrap_or_else(|| panic!("{name}"))
}

fn ready_guard(arm: &mut TransitionSchema) -> &mut Guard {
    arm.guards
        .iter_mut()
        .find(|guard| guard.name == READY_GUARD)
        .unwrap_or_else(|| panic!("{READY_GUARD}"))
}

/// The tail-drain conjunct #1597 removed: no queued context row may wait
/// before a result.
fn queued_outbox_drained() -> Expr {
    Expr::Eq(
        Box::new(Expr::Field(
            FieldId::parse("live_context_queued_append_by_cursor").expect("queued outbox field id"),
        )),
        Box::new(Expr::EmptyMap),
    )
}

fn with_queued_outbox_drained(expr: &mut Expr) {
    *expr = Expr::And(vec![expr.clone(), queued_outbox_drained()]);
}

#[test]
fn authorization_guards_follow_the_readiness_rule() {
    let machine = dsl_meerkat_machine();
    for input in AUTHORIZATION_INPUTS {
        assert!(
            machine
                .transitions
                .iter()
                .any(|arm| triggered_by(arm, input)),
            "MeerkatMachine has no {input} arm"
        );
    }
    assert_eq!(readiness_guard_drift(&machine), Vec::<String>::new());
}

/// Mutant: one authorization guard that also waits for the queued context
/// outbox is refused.
#[test]
fn an_authorization_guard_that_waits_for_queued_context_is_refused() {
    let mut machine = dsl_meerkat_machine();
    with_queued_outbox_drained(
        &mut ready_guard(arm(
            &mut machine,
            "AuthorizeLiveDelegationResultDeliveryRunning",
        ))
        .expr,
    );
    assert_eq!(
        readiness_guard_drift(&machine),
        ["AuthorizeLiveDelegationResultDeliveryRunning"]
    );
}

/// Mutant: an authorization arm without the readiness guard is refused.
#[test]
fn an_authorization_arm_without_the_readiness_guard_is_refused() {
    let mut machine = dsl_meerkat_machine();
    arm(&mut machine, "AuthorizeLiveBridgeSubmissionIdle")
        .guards
        .retain(|guard| guard.name != READY_GUARD);
    assert_eq!(
        readiness_guard_drift(&machine),
        ["AuthorizeLiveBridgeSubmissionIdle"]
    );
}

/// Mutant: readiness that waits for the queued context outbox, with the
/// authorization guards unchanged, refuses every authorization arm.
#[test]
fn readiness_that_waits_for_queued_context_refuses_every_authorization_arm() {
    let mut machine = dsl_meerkat_machine();
    let mut changed = 0;
    for arm in machine
        .transitions
        .iter_mut()
        .filter(|arm| triggered_by(arm, READINESS_INPUT))
    {
        let readiness = arm
            .emit
            .iter_mut()
            .find(|emit| emit.variant.as_str() == READINESS_EFFECT)
            .and_then(|emit| emit.fields.get_mut(&readiness_field()))
            .expect("readiness");
        with_queued_outbox_drained(ready_branch_mut(readiness).expect("Ready branch"));
        changed += 1;
    }
    assert!(changed > 0, "positive control: readiness arms were changed");
    let every_authorization_arm: Vec<String> = authorization_arms(&machine)
        .map(|arm| arm.name.as_str().to_owned())
        .collect();
    assert!(
        every_authorization_arm.len() >= AUTHORIZATION_INPUTS.len(),
        "positive control: authorization arms exist"
    );
    assert_eq!(readiness_guard_drift(&machine), every_authorization_arm);
}
