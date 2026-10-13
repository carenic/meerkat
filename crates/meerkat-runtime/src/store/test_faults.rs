//! Test-support fault seams of the runtime store. Compiled only for tests and
//! the `test-support` feature; the normal build has none of this.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use meerkat_core::lifecycle::InputId;

use crate::identifiers::LogicalRuntimeId;

/// One store instance's scripted loss of an input-persistence acknowledgement.
///
/// Armed for exactly one runtime, it fires once: the next
/// `persist_input_states_atomically` batch for that runtime on the store that
/// issued this handle commits for real, and the call then reports
/// `RuntimeStoreError::WriteFailed` as if the acknowledgement was lost after
/// the commit. Other runtimes on the same store and every other store instance
/// are unaffected, and a batch that fails to commit does not consume it.
///
/// Only `SqliteRuntimeStore` issues one. `RuntimeStore` returns `None` for
/// every other store, including a store that wraps a `SqliteRuntimeStore`.
///
/// A control that arms it must assert afterwards that it fired:
/// `is_armed() == false` and `consumed_commit() == Some((runtime_id,
/// vec![input_id]))`. A wrong runtime id or a path that never commits through
/// this store then fails closed instead of passing vacuously. Stock sessions
/// persist their admissions under `LogicalRuntimeId::for_session(&session_id)`.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct InputPersistAckLossFault(Arc<Mutex<AckLossState>>);

#[derive(Default)]
struct AckLossState {
    armed: Option<LogicalRuntimeId>,
    consumed: Option<(LogicalRuntimeId, Vec<InputId>)>,
}

impl InputPersistAckLossFault {
    /// Lose the acknowledgement of the next committed input-state batch of
    /// exactly `runtime_id` on this store. Re-arming clears the previous
    /// consumed record.
    pub fn lose_next_commit_acknowledgement(&self, runtime_id: LogicalRuntimeId) {
        let mut state = self.state();
        state.armed = Some(runtime_id);
        state.consumed = None;
    }

    /// Whether an armed loss has not fired yet.
    pub fn is_armed(&self) -> bool {
        self.state().armed.is_some()
    }

    /// The runtime and the exact input ids of the committed batch whose
    /// acknowledgement was lost, once the armed loss has fired.
    pub fn consumed_commit(&self) -> Option<(LogicalRuntimeId, Vec<InputId>)> {
        self.state().consumed.clone()
    }

    /// Called by `SqliteRuntimeStore` after `inputs` committed for
    /// `runtime_id`: whether this commit's acknowledgement is lost. Consumes
    /// the loss only when it is armed for exactly that runtime.
    #[cfg(feature = "sqlite-store")]
    pub(crate) fn lose_after_commit(
        &self,
        runtime_id: &LogicalRuntimeId,
        inputs: Vec<InputId>,
    ) -> bool {
        let mut state = self.state();
        if state.armed.as_ref() != Some(runtime_id) {
            return false;
        }
        state.armed = None;
        state.consumed = Some((runtime_id.clone(), inputs));
        true
    }

    fn state(&self) -> MutexGuard<'_, AckLossState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
