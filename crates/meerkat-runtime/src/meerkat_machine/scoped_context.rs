//! Scoped session document/context ownership.
//!
//! The registration owner binds one exact destination (owner, runtime,
//! context, generation, audience) to the session document it registered on
//! this machine. The generated machine records it; governed inputs that name
//! another destination are refused at admission. Accepted work whose
//! destination is no longer current (retired or rebound) still stages under
//! its own controller: its model operations keep their independent
//! authority, and its destination-scoped operations are refused as typed
//! local feedback.

use meerkat_authorization_contracts::work_association::NativeWorkTarget;

use super::{MeerkatMachine, RuntimeSessionRegistrationWitness, dsl};
use crate::input_state::ScopedDestinationWitness;
use crate::traits::RuntimeDriverError;

/// The session document never served (nor requires) a scoped destination:
/// governed work is unscoped, as before.
pub(crate) fn never_scoped(state: &dsl::MeerkatMachineState) -> bool {
    !state.scoped_context_required
        && state.scoped_context_generation == 0
        && state.scoped_context_binding.is_none()
}

/// The destination run `run_id` was staged under is still the session
/// document's current binding. A run staged without a destination is current
/// only on a never-scoped document: work accepted before a destination was
/// bound is not current once one was (bound or retired). Read from the
/// generated state at every use of the destination: a destination-scoped
/// operation's preparation and the disclosure of the run's result. The
/// run's controller operations and its checkpoint do not depend on it.
pub(crate) fn run_destination_current(
    state: &dsl::MeerkatMachineState,
    run_id: &dsl::RunId,
) -> bool {
    if state.authority_staged_run.as_ref() != Some(run_id) {
        return never_scoped(state);
    }
    match state.run_context_binding.as_ref() {
        None => never_scoped(state),
        Some(binding) => state.scoped_context_binding.as_ref() == Some(binding),
    }
}

/// How this machine's governed work host treats scoped destinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopedHostPosture {
    /// No governed host: no destination can be bound on this machine.
    Ungoverned,
    Governed,
    /// Every governed input must name a bound destination.
    RequiresDestination,
}

pub(crate) fn scoped_host_posture(
    host: &crate::input_authority::NativeWorkAuthorizationSlot,
) -> ScopedHostPosture {
    match host.get() {
        None => ScopedHostPosture::Ungoverned,
        Some(host) if host.host().requires_scoped_destination() => {
            ScopedHostPosture::RequiresDestination
        }
        Some(_) => ScopedHostPosture::Governed,
    }
}

/// The original destination witness an input admitted now keeps on its row:
/// the destination the machine recorded for it, else whether the session
/// document had ever served one.
pub(crate) fn admitted_destination_witness(
    state: &dsl::MeerkatMachineState,
    input_key: &str,
    posture: ScopedHostPosture,
) -> ScopedDestinationWitness {
    match state.input_context_targets.get(input_key) {
        Some(binding) => ScopedDestinationWitness::Bound {
            binding: binding.clone(),
        },
        None if never_scoped(state) && posture != ScopedHostPosture::RequiresDestination => {
            ScopedDestinationWitness::NeverScoped
        }
        None => ScopedDestinationWitness::Unbound,
    }
}

/// Whether the result payload of an input admitted under `witness` may be
/// disclosed to a reader now. `state` is the session document's live
/// generated state (`None` without a live registration: there is then no
/// current destination). Only the exact destination the input was admitted
/// under, while it is still the document's current binding, receives the
/// result of scoped work; a retired or replaced destination, or a
/// registration without one, receives nothing. Reader identity and audience
/// grants stay with the surface that authenticated the reader: this only
/// withholds, it never grants.
///
/// An `Unrecorded` (legacy or unknown) witness is never taken as unscoped
/// governed work: with a governed host it is disclosed only for an
/// ungoverned row on a document that never served a destination.
pub(crate) fn result_disclosure_permitted(
    state: Option<&dsl::MeerkatMachineState>,
    witness: &ScopedDestinationWitness,
    governed_row: bool,
    posture: ScopedHostPosture,
) -> bool {
    let document_unscoped = match posture {
        ScopedHostPosture::Ungoverned => true,
        ScopedHostPosture::Governed => state.is_none_or(never_scoped),
        ScopedHostPosture::RequiresDestination => false,
    };
    match witness {
        ScopedDestinationWitness::Bound { binding } => {
            state.is_some_and(|state| state.scoped_context_binding.as_ref() == Some(binding))
        }
        ScopedDestinationWitness::NeverScoped => document_unscoped,
        ScopedDestinationWitness::Unbound => false,
        ScopedDestinationWitness::Unrecorded => {
            posture == ScopedHostPosture::Ungoverned || (!governed_row && document_unscoped)
        }
    }
}

/// The completion registry's disclosure gate for one registration of a
/// session document. It captures the destination the machine recorded for
/// an admitted input, and the registration's runtime epoch; the registry
/// asks when the input's waiter registers, because the input's terminal
/// archives that record before its result is delivered. An input the
/// machine does not hold (not yet admitted, or already archived) yields no
/// decision: never an unscoped one. The decision itself is
/// taken when the payload is released, after any waits and cleanup relays.
/// Scoped work is released only against the actual current registration:
/// the machine's current session entry must still be this registration (its
/// driver owns this generated authority and it still holds this epoch, so
/// neither an unregistered nor a re-registered session passes), and the
/// destination must still be the document's current binding. Work of a
/// document that never served a destination is decided on that document.
pub(crate) fn completion_disclosure_gate(
    machine: std::sync::Weak<super::MeerkatMachineShared>,
    session_id: meerkat_core::types::SessionId,
    authority: crate::driver::ephemeral::SharedIngressDslAuthority,
) -> crate::completion::CompletionDisclosureGate {
    std::sync::Arc::new(move |input_id| {
        let (witness, epoch) = {
            let authority = authority
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let state = authority.state();
            let key = input_id.to_string();
            let witness = match state.input_context_targets.get(&key) {
                Some(binding) => ScopedDestinationWitness::Bound {
                    binding: binding.clone(),
                },
                // Unbound work on a document that served a destination is
                // refused by the same rule: such a document is not unscoped.
                None if state.input_phases.contains_key(&key) => {
                    ScopedDestinationWitness::NeverScoped
                }
                None => return None,
            };
            (witness, state.active_runtime_epoch_id.clone())
        };
        let machine = machine.clone();
        let session_id = session_id.clone();
        let delivered_by = std::sync::Arc::clone(&authority);
        Some(crate::completion::DeferredDisclosure::new(move || {
            let machine = machine.clone();
            let session_id = session_id.clone();
            let delivered_by = std::sync::Arc::clone(&delivered_by);
            let witness = witness.clone();
            let epoch = epoch.clone();
            async move {
                let machine = machine.upgrade();
                if witness == ScopedDestinationWitness::NeverScoped {
                    // Work of a registration whose document never served a
                    // destination is unscoped: decided on that document, as
                    // any never-scoped result (no destination to outlive).
                    let posture = machine
                        .as_ref()
                        .map_or(ScopedHostPosture::Governed, |machine| {
                            scoped_host_posture(&machine.native_work_authorization_host)
                        });
                    let delivered_by = delivered_by
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    return result_disclosure_permitted(
                        Some(delivered_by.state()),
                        &witness,
                        false,
                        posture,
                    );
                }
                let Some(machine) = machine else {
                    return false;
                };
                let driver = {
                    let sessions = machine.sessions.read().await;
                    match sessions.get(&session_id) {
                        Some(entry) => std::sync::Arc::clone(&entry.driver),
                        None => return false,
                    }
                };
                let driver = driver.lock().await;
                let current = driver.shared_dsl_authority();
                if !std::sync::Arc::ptr_eq(&current, &delivered_by) {
                    return false;
                }
                let current = current
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let state = current.state();
                state.session_id.is_some()
                    && epoch.is_some()
                    && state.active_runtime_epoch_id == epoch
                    && result_disclosure_permitted(
                        Some(state),
                        &witness,
                        false,
                        scoped_host_posture(&machine.native_work_authorization_host),
                    )
            }
        }))
    })
}

impl MeerkatMachine {
    /// A completion registry for one registration of `session_id`, whose
    /// result payloads are released through the session document's
    /// disclosure gate.
    pub(crate) fn scoped_completion_registry(
        &self,
        session_id: &meerkat_core::types::SessionId,
        authority: crate::driver::ephemeral::SharedIngressDslAuthority,
    ) -> crate::completion::CompletionRegistry {
        crate::completion::CompletionRegistry::with_disclosure_gate(completion_disclosure_gate(
            std::sync::Arc::downgrade(&self.shared),
            session_id.clone(),
            authority,
        ))
    }

    /// Whether a result read of `row` (live or durable) may disclose its
    /// payload now. Called after the read's last await, immediately before
    /// release. The decision is taken under the current session entry's
    /// driver lock, against its generated state; a session that is not
    /// registered (or was unregistered meanwhile) has no current
    /// destination.
    pub(crate) async fn stored_result_disclosure_permitted(
        &self,
        session_id: &meerkat_core::types::SessionId,
        row: &crate::input_state::InputState,
    ) -> bool {
        let posture = scoped_host_posture(&self.native_work_authorization_host);
        let driver = {
            let sessions = self.sessions.read().await;
            sessions
                .get(session_id)
                .map(|entry| std::sync::Arc::clone(&entry.driver))
        };
        match driver {
            Some(driver) => {
                let driver = driver.lock().await;
                disclosure_permitted_with(Some(&driver.shared_dsl_authority()), row, posture)
            }
            None => disclosure_permitted_with(None, row, posture),
        }
    }

    /// Withhold the result payload of a public row read this reader may not
    /// receive. The stored row is unchanged; the returned copy keeps every
    /// body-free lifecycle fact (phase, terminal, receipt identity, witness).
    pub(crate) async fn release_public_row(
        &self,
        session_id: &meerkat_core::types::SessionId,
        row: Option<crate::input_state::StoredInputState>,
    ) -> Option<crate::input_state::StoredInputState> {
        let mut row = row?;
        if row.carries_result_payload()
            && !self
                .stored_result_disclosure_permitted(session_id, &row.state)
                .await
        {
            row.withhold_result_payload();
        }
        Some(row)
    }
}

/// [`result_disclosure_permitted`] for a stored row against an optional live
/// generated authority. A destination the machine still records for the
/// input takes precedence over the row's witness (they agree by
/// construction).
pub(crate) fn disclosure_permitted_with(
    authority: Option<&crate::driver::ephemeral::SharedIngressDslAuthority>,
    row: &crate::input_state::InputState,
    posture: ScopedHostPosture,
) -> bool {
    let governed_row = !row.authority_contributors.is_empty();
    match authority {
        Some(authority) => {
            let authority = authority
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let state = authority.state();
            // An unregistered document has no current destination.
            if state.session_id.is_none() {
                return result_disclosure_permitted(
                    None,
                    &row.scoped_destination,
                    governed_row,
                    posture,
                );
            }
            let witness = match state.input_context_targets.get(&row.input_id.to_string()) {
                Some(binding) => ScopedDestinationWitness::Bound {
                    binding: binding.clone(),
                },
                None => row.scoped_destination.clone(),
            };
            result_disclosure_permitted(Some(state), &witness, governed_row, posture)
        }
        None => result_disclosure_permitted(None, &row.scoped_destination, governed_row, posture),
    }
}

/// Withhold a result-bearing outcome a reader may not receive. The input,
/// its terminal and its preserved result are unaffected; lifecycle-only
/// outcomes stay readable.
pub(crate) fn withhold_undisclosed(
    outcome: Option<crate::completion::CompletionOutcome>,
    permitted: bool,
    input_id: &meerkat_core::lifecycle::InputId,
) -> Result<Option<crate::completion::CompletionOutcome>, RuntimeDriverError> {
    match outcome {
        Some(outcome) if !permitted && outcome.carries_result_payload() => {
            Err(RuntimeDriverError::ScopedResultWithheld {
                input_id: input_id.clone(),
            })
        }
        outcome => Ok(outcome),
    }
}

impl MeerkatMachine {
    /// Bind the session document of `registration` to `target`.
    ///
    /// `registration` must be this machine's exact current registration and
    /// `target` must name its runtime. The generation is the target's
    /// `context_generation` and must advance; no run may be in flight. The
    /// owner, runtime, context and audience form the document's stable scope:
    /// once bound, a rebind may only advance the generation of that same
    /// scope (no relabeling of the same session history). A replacement
    /// registration (for example after a restart) binds again.
    pub async fn bind_scoped_context(
        &self,
        registration: &RuntimeSessionRegistrationWitness,
        target: &NativeWorkTarget,
    ) -> Result<(), RuntimeDriverError> {
        self.bind_scoped_context_within(registration, target, |commit| {
            commit();
            Ok(())
        })
        .await
    }

    /// [`Self::bind_scoped_context`], committed only within the binder's
    /// final decision. `within` runs synchronously at the binding's commit,
    /// under this session's mutation gate and with the session document's
    /// machine authority held, and receives `commit`, the binding's one
    /// transition. It runs `commit` at most once, and only while its decision
    /// to bind holds through it (for example inside its own non-waiting
    /// exclusion of the facts that decision read,
    /// `LocalGrantAuthority::try_with_owner_changes_excluded`). Once `commit`
    /// ran, the transition's result is the outcome. A `within` that does not
    /// run it changes nothing: its refusal is returned, and a success without
    /// a commit is refused. It must not perform I/O, await or wait for a lock.
    pub async fn bind_scoped_context_within(
        &self,
        registration: &RuntimeSessionRegistrationWitness,
        target: &NativeWorkTarget,
        within: impl FnOnce(&mut dyn FnMut()) -> Result<(), RuntimeDriverError> + Send,
    ) -> Result<(), RuntimeDriverError> {
        let session_id = registration.session_id();
        // A scoped destination is governed custody: without the installed
        // governed host, ungoverned input would bypass it entirely.
        if self.native_work_authorization_host.get().is_none() {
            return Err(RuntimeDriverError::ValidationFailed {
                reason: "a scoped destination requires the governed work host".to_string(),
            });
        }
        if target.logical_runtime.as_str() != Self::logical_runtime_id(session_id).to_string() {
            return Err(RuntimeDriverError::ValidationFailed {
                reason: "scoped context target names another runtime".to_string(),
            });
        }
        let binding = crate::input_authority::context_target_binding(target)?;
        let scope = crate::input_authority::context_scope_binding(target)?;
        self.with_current_registration_driver(registration, |driver| {
            // Immediately before the driver takes the session document's
            // machine authority: a test holds a binding here.
            #[cfg(any(test, feature = "test-support"))]
            self.run_scoped_binding_before_machine_authority_test_hook(session_id);
            driver.bind_scoped_context_within(binding, scope, target.context_generation, within)
        })
        .await
    }

    /// Retire the session document's current scoped destination of
    /// `generation`. Work staged under it keeps its controller; its
    /// destination-scoped operations are refused.
    pub async fn retire_scoped_context(
        &self,
        registration: &RuntimeSessionRegistrationWitness,
        generation: u64,
    ) -> Result<(), RuntimeDriverError> {
        self.with_current_registration_driver(registration, |driver| {
            driver.retire_scoped_context(generation)
        })
        .await
    }

    /// Authorize disclosing the reply of scoped run `run_id` to `target`'s
    /// audience. The output owner (the scoped reply sink) calls this before
    /// delivering: it succeeds only while `target` is exactly the destination
    /// the run was staged under and that destination is still the session
    /// document's current binding. A retired or replaced destination is
    /// refused; the run's checkpointed result is unaffected.
    pub async fn authorize_scoped_reply(
        &self,
        registration: &RuntimeSessionRegistrationWitness,
        run_id: &meerkat_core::RunId,
        target: &NativeWorkTarget,
    ) -> Result<(), RuntimeDriverError> {
        let binding = crate::input_authority::context_target_binding(target)?;
        let run = dsl::RunId::from_domain(run_id);
        self.with_current_registration_driver(registration, |driver| {
            let authority = driver.shared_dsl_authority();
            let authority = authority
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let state = authority.state();
            if state.authority_staged_run.as_ref() == Some(&run)
                && state.run_context_binding.as_ref() == Some(&binding)
                && run_destination_current(state, &run)
            {
                Ok(())
            } else {
                Err(RuntimeDriverError::ValidationFailed {
                    reason: "the scoped reply's destination is not the current binding of its run"
                        .to_string(),
                })
            }
        })
        .await
    }

    async fn with_current_registration_driver<R>(
        &self,
        registration: &RuntimeSessionRegistrationWitness,
        apply: impl FnOnce(&mut super::driver::DriverEntry) -> Result<R, RuntimeDriverError>,
    ) -> Result<R, RuntimeDriverError> {
        let stale = || RuntimeDriverError::StaleAuthority {
            reason: "scoped context registration witness is not current".to_string(),
        };
        if !registration.belongs_to(self) {
            return Err(stale());
        }
        let _gate = self
            .lock_current_durability_ready_session_mutation_gate(registration.session_id())
            .await?;
        let driver = {
            let sessions = self.sessions.read().await;
            let entry = sessions.get(registration.session_id()).ok_or_else(stale)?;
            if !registration.matches_entry(entry) || &entry.epoch_id != registration.epoch_id() {
                return Err(stale());
            }
            entry.driver.clone()
        };
        let mut driver = driver.lock().await;
        apply(&mut driver)
    }
}
