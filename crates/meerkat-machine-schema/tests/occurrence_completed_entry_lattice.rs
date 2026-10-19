//! Occurrence Completed entry lattice pin: an occurrence's terminal comes
//! only from its input's own terminal outcome.
//!
//! A deduplicated admission once took an occurrence straight from
//! `Dispatching` to `Completed` (`DispatchDeduplicatedFromDispatching`). A
//! dedup is the target's proof that the stable delivery identity was
//! admitted once, not proof of what that input then did: after a reclaim the
//! occurrence recorded `Completed` for an input that was still running, or
//! that later failed. The dedup arm now stays `Dispatching` like an accepted
//! admission, and the occurrence waits for the existing input's outcome.
//!
//! This test pins every arc into `Completed` structurally. It derives each
//! transition's admitted source phases from its `from` list and its phase
//! guards by walking the guard expression tree (never its Debug text, so a
//! guard on `RuntimeCompletionOutcome::Completed` is never read as the
//! phase). A guard shape it does not recognize leaves the phase open, which
//! fails closed: the arm then counts as an entry into `Completed`.
//! Restoring `to Completed` in `DispatchDeduplicatedFromDispatching` adds an
//! entry on `DispatchAccepted` and fails the first test; deleting or
//! widening the `runtime_outcome_completed` guard fails the second.

#![allow(clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use meerkat_machine_schema::catalog::dsl::dsl_occurrence_lifecycle_machine;
use meerkat_machine_schema::{Expr, TransitionSchema, TriggerMatch};

const COMPLETED: &str = "Completed";
const PHASE_FIELD: &str = "lifecycle_phase";

/// The phase named by `phase` when `phase_read` reads the machine phase
/// (`self.lifecycle_phase`, lowered to `CurrentPhase` for a stored-phase
/// machine) and `phase` is a phase literal.
fn phase_equality(phase_read: &Expr, phase: &Expr) -> Option<String> {
    let reads_phase = match phase_read {
        Expr::CurrentPhase => true,
        Expr::Field(field) => field.as_str() == PHASE_FIELD,
        _ => false,
    };
    match phase {
        Expr::Phase(id) if reads_phase => Some(id.as_str().to_string()),
        _ => None,
    }
}

/// The phases `expr` restricts the machine to, or `None` when it leaves the
/// phase open: an Eq between the phase read and a phase literal, an Or whose
/// every disjunct is such a constraint, and the conjuncts of an And that
/// constrain the phase (intersected). Anything else leaves the phase open.
fn phase_constraint(expr: &Expr) -> Option<BTreeSet<String>> {
    match expr {
        Expr::Eq(left, right) => phase_equality(left, right)
            .or_else(|| phase_equality(right, left))
            .map(|phase| BTreeSet::from([phase])),
        Expr::Or(disjuncts) => {
            let mut admitted = BTreeSet::new();
            for disjunct in disjuncts {
                admitted.extend(phase_constraint(disjunct)?);
            }
            Some(admitted)
        }
        Expr::And(conjuncts) => {
            let mut admitted: Option<BTreeSet<String>> = None;
            for phases in conjuncts.iter().filter_map(phase_constraint) {
                admitted = Some(match admitted {
                    Some(earlier) => earlier.intersection(&phases).cloned().collect(),
                    None => phases,
                });
            }
            admitted
        }
        _ => None,
    }
}

/// The phases `transition` can fire from: its `from` list (every phase when
/// empty), narrowed by any phase constraint left in its guards.
fn admitted_phases(
    transition: &TransitionSchema,
    every_phase: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut admitted: BTreeSet<String> = if transition.from.is_empty() {
        every_phase.clone()
    } else {
        transition
            .from
            .iter()
            .map(|phase| phase.as_str().to_string())
            .collect()
    };
    for guard in &transition.guards {
        if let Some(phases) = phase_constraint(&guard.expr) {
            admitted = admitted.intersection(&phases).cloned().collect();
        }
    }
    admitted
}

fn trigger(transition: &TransitionSchema) -> String {
    match &transition.on {
        TriggerMatch::Input { variant, .. } => format!("input {}", variant.as_str()),
        TriggerMatch::Signal { variant, .. } => format!("signal {}", variant.as_str()),
    }
}

fn pinned(rows: &[(&str, &str)]) -> BTreeMap<String, String> {
    rows.iter()
        .map(|(name, input)| ((*name).to_string(), (*input).to_string()))
        .collect()
}

/// Whether `expr` reads the runtime completion outcome the trigger binds.
fn reads_outcome(expr: &Expr) -> bool {
    match expr {
        Expr::Binding(name) => name.as_str() == "outcome",
        Expr::Field(field) => field.as_str() == "outcome",
        _ => false,
    }
}

/// Whether `expr` is the literal `RuntimeCompletionOutcome::Completed`.
fn is_completed_outcome(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::NamedVariant { enum_name, variant }
            if enum_name.as_str() == "RuntimeCompletionOutcome"
                && variant.as_str() == COMPLETED
    )
}

/// Every arc into `Completed`, pinned by name and trigger input. The only
/// entries are the delivery's own terminal: its `Complete`, and a runtime
/// completion whose outcome is `Completed`. The only other arcs are
/// self-loops that already sit in `Completed`.
#[test]
fn completed_is_entered_only_from_the_inputs_own_terminal_outcome() {
    let schema = dsl_occurrence_lifecycle_machine();
    let every_phase: BTreeSet<String> = schema
        .state
        .phase
        .variants
        .iter()
        .map(|variant| variant.name.as_str().to_string())
        .collect();
    assert!(
        every_phase.contains(COMPLETED),
        "the occurrence machine lost its {COMPLETED} phase: {every_phase:?}"
    );

    let mut entries = BTreeMap::new();
    let mut self_loops = BTreeMap::new();
    for transition in &schema.transitions {
        if transition.to.as_str() != COMPLETED {
            continue;
        }
        let admitted = admitted_phases(transition, &every_phase);
        let row = (transition.name.as_str().to_string(), trigger(transition));
        if admitted.iter().any(|phase| phase != COMPLETED) {
            entries.insert(row.0, row.1);
        } else {
            self_loops.insert(row.0, row.1);
        }
    }

    assert_eq!(
        entries,
        pinned(&[
            ("CompleteFromDispatchingOrAwaiting", "input Complete"),
            (
                "RuntimeCompletionCompleted",
                "input ResolveRuntimeCompletion"
            ),
        ]),
        "an occurrence's terminal comes only from its input's own terminal \
         outcome: Completed is entered only by the delivery's Complete or by a \
         runtime completion whose outcome is Completed. An admission, deduplicated \
         or not, proves the input was admitted, not what it did. A new entry into \
         Completed is a reviewed machine-authority change, never a silent one"
    );
    assert_eq!(
        self_loops,
        pinned(&[
            ("ClassifyDueCompletedNoAction", "input ClassifyDue"),
            ("RecordReceiptCompleted", "input RecordReceipt"),
        ]),
        "the arcs that stay in Completed changed; each one must admit only \
         Completed, or it is an entry and belongs in the reviewed entry set above"
    );
}

/// `RuntimeCompletionCompleted` keeps its name and trigger whatever its
/// guards say, so the entry pin alone would still pass if the outcome guard
/// were dropped or widened and a failed outcome entered Completed. The
/// labelled guard must stay exactly `outcome == RuntimeCompletionOutcome::Completed`.
#[test]
fn runtime_completion_enters_completed_only_on_a_completed_outcome() {
    let schema = dsl_occurrence_lifecycle_machine();
    let Some(transition) = schema
        .transitions
        .iter()
        .find(|transition| transition.name.as_str() == "RuntimeCompletionCompleted")
    else {
        panic!("the occurrence machine lost RuntimeCompletionCompleted");
    };
    assert_eq!(transition.to.as_str(), COMPLETED);
    let TriggerMatch::Input { variant, bindings } = &transition.on else {
        panic!("RuntimeCompletionCompleted is no longer an input transition");
    };
    assert_eq!(variant.as_str(), "ResolveRuntimeCompletion");
    assert!(
        bindings.iter().any(|binding| binding.as_str() == "outcome"),
        "RuntimeCompletionCompleted no longer binds the runtime completion outcome: {bindings:?}"
    );

    let Some(guard) = transition
        .guards
        .iter()
        .find(|guard| guard.name == "runtime_outcome_completed")
    else {
        panic!(
            "RuntimeCompletionCompleted lost its runtime_outcome_completed guard, so any \
             runtime completion outcome would enter Completed"
        );
    };
    let guards_completed_outcome = match &guard.expr {
        Expr::Eq(left, right) => {
            (reads_outcome(left) && is_completed_outcome(right))
                || (reads_outcome(right) && is_completed_outcome(left))
        }
        _ => false,
    };
    assert!(
        guards_completed_outcome,
        "runtime_outcome_completed must stay exactly outcome == \
         RuntimeCompletionOutcome::Completed; a dropped or widened guard lets an \
         outcome other than Completed enter Completed: {:?}",
        guard.expr
    );
}
