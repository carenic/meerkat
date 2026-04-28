// @generated — protocol helpers for `ops_barrier_satisfaction`
// Composition: meerkat_mob_seam, Producer: meerkat, Effect: WaitAllSatisfied
// Closure policy: AckRequired
// Liveness: eventual feedback under task-scheduling fairness

use crate::OperationId;
use crate::handles::{DslTransitionError, TurnStateHandle};

#[derive(Debug, Clone)]
pub struct OpsBarrierSatisfactionObligation {
    pub operation_ids: Vec<OperationId>,
}

pub fn submit_ops_barrier_satisfied(
    handle: &(impl TurnStateHandle + ?Sized),
    obligation: OpsBarrierSatisfactionObligation,
) -> Result<(), DslTransitionError> {
    handle.ops_barrier_satisfied(
        obligation
            .operation_ids
            .iter()
            .map(ToString::to_string)
            .collect(),
    )
}
