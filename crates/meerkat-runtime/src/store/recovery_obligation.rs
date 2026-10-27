//! Whether a runtime holds recovery work: the one predicate a boot asks.
//!
//! A boot learns which runtimes recovery must act on without loading their
//! bodies. Three small facts kept beside each runtime decide it: its boot
//! facts ([`BootFactsV1`]), its machine lifecycle record and its session
//! catalog entry. [`runtime_has_recovery_obligation`] reads them. The host
//! store adapter's open asks it to choose the bodies it loads, and every
//! store's obligation scan
//! ([`RuntimeStore::list_runtimes_with_recovery_work`](super::RuntimeStore::list_runtimes_with_recovery_work))
//! asks it to choose the runtimes it lists, so the two cannot disagree.

use serde::{Deserialize, Serialize};

use super::{MachineLifecycleObservation, RuntimeSessionCatalogEntry};

/// Facts a boot reads from a runtime's head to decide whether recovery must
/// act on it, refreshed by every commit of the runtime.
///
/// The host store adapter pins them in every runtime head under the entry
/// key `boot` (see `memory::journal_image`); the in-memory and SQLite stores
/// compute the same facts from their own rows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BootFactsV1 {
    pub(crate) v: u32,
    /// The runtime has nonterminal inputs.
    pub(crate) nonterminal: bool,
    /// The runtime has pending terminal owners.
    pub(crate) pending_owners: bool,
    /// The runtime has a provisional tail (an open run cursor).
    pub(crate) tail: bool,
    /// The runtime has input rows.
    pub(crate) inputs: bool,
}

impl BootFactsV1 {
    /// The format version every writer stamps and every reader requires.
    pub(crate) const VERSION: u32 = 1;
}

/// Whether a lifecycle record names a current run. A record that does not
/// decode counts as one: recovery must look at it.
pub(crate) fn lifecycle_has_current_run(bytes: Option<&[u8]>) -> bool {
    match bytes.map(super::classify_machine_lifecycle_record) {
        None => false,
        Some(MachineLifecycleObservation::Decoded { record, .. }) => {
            record.run().current_run_id().is_some()
        }
        Some(_) => true,
    }
}

/// Whether a runtime's session catalog entry marks it archived.
pub(crate) fn catalog_entry_is_archived(catalog: Option<&RuntimeSessionCatalogEntry>) -> bool {
    catalog
        .and_then(RuntimeSessionCatalogEntry::lifecycle_terminal)
        .is_some_and(meerkat_core::SessionLifecycleTerminal::is_archived)
}

/// Whether recovery must act on a runtime: it holds work recovery owns and
/// it is not archived.
///
/// The work: nonterminal inputs, pending terminal owners, a provisional tail
/// (an open run cursor), a `lifecycle` record that names a current run or
/// does not decode, or input rows with no lifecycle record. `facts` are the
/// runtime's boot facts ([`BootFactsV1::default`] for a runtime that holds
/// nothing), `lifecycle` its machine lifecycle record as stored, `catalog`
/// its session catalog entry. A runtime whose catalog entry is archived
/// holds no recovery work, whatever its other facts say.
pub(crate) fn runtime_has_recovery_obligation(
    facts: &BootFactsV1,
    lifecycle: Option<&[u8]>,
    catalog: Option<&RuntimeSessionCatalogEntry>,
) -> bool {
    !catalog_entry_is_archived(catalog)
        && (facts.nonterminal
            || facts.pending_owners
            || facts.tail
            || lifecycle_has_current_run(lifecycle)
            || (facts.inputs && lifecycle.is_none()))
}

/// A contract every [`super::RuntimeStore`] obligation scan meets, seeded
/// through the store's own trait methods: one runtime for each kind of
/// recovery work, and runtimes the scan must leave out.
#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
pub(crate) mod scan_contract {
    use std::collections::HashSet;
    use std::sync::Arc;

    use meerkat_core::lifecycle::{InputId, RunId};
    use meerkat_core::types::{Message, UserMessage};

    use crate::identifiers::LogicalRuntimeId;
    use crate::input_state::{
        InputLifecycleState, InputStatePersistenceRecord, InputTerminalOutcome, StoredInputState,
    };
    use crate::runtime_state::RuntimeState;
    use crate::store::{
        MachineLifecycleBindingFacts, MachineLifecycleCommit, MachineLifecyclePreRunPhase,
        MachineLifecycleRunFacts, PreparedWholeBlobProvisionalTail, RuntimeStore,
        SerializedSessionSnapshot, SupervisorAuthoritySnapshot,
    };

    /// The runtimes [`seed`] wrote, by whether a scan lists them.
    pub(crate) struct ScanFixture {
        /// One runtime for each kind of recovery work.
        pub(crate) listed: HashSet<LogicalRuntimeId>,
        /// An idle runtime and an archived one that holds a nonterminal
        /// input.
        pub(crate) unlisted: HashSet<LogicalRuntimeId>,
    }

    impl ScanFixture {
        /// Every runtime the fixture wrote.
        pub(crate) fn runtimes(&self) -> impl Iterator<Item = &LogicalRuntimeId> {
            self.listed.iter().chain(&self.unlisted)
        }
    }

    fn persistable(state: StoredInputState) -> InputStatePersistenceRecord {
        InputStatePersistenceRecord::from_machine_snapshot(state).expect("machine snapshot")
    }

    /// An accepted input: nonterminal.
    fn accepted() -> InputStatePersistenceRecord {
        persistable(StoredInputState::new_accepted(InputId::new()))
    }

    /// `state`, consumed: terminal.
    fn consumed(mut state: StoredInputState) -> InputStatePersistenceRecord {
        state.seed.phase = InputLifecycleState::Consumed;
        state.seed.terminal_outcome = Some(InputTerminalOutcome::Consumed);
        state.seed.recovery_lane = None;
        persistable(state)
    }

    /// A lifecycle bound to `runtime_id`, naming `current_run` or no run.
    fn lifecycle(
        runtime_id: &LogicalRuntimeId,
        current_run: Option<RunId>,
    ) -> MachineLifecycleCommit {
        let state = if current_run.is_some() {
            RuntimeState::Running
        } else {
            RuntimeState::Idle
        };
        MachineLifecycleCommit::new_with_binding_run_and_unregister_progress(
            state,
            MachineLifecycleBindingFacts::new(
                Some(runtime_id.0.clone()),
                Some(1),
                Some(1),
                Some("epoch-1".to_string()),
            ),
            MachineLifecycleRunFacts::new(current_run, Some(MachineLifecyclePreRunPhase::Idle)),
            SupervisorAuthoritySnapshot::UnboundNoReceipt,
            None,
        )
    }

    /// Commit `session` as its runtime's committed document.
    async fn commit_session(
        store: &dyn RuntimeStore,
        session: &meerkat_core::Session,
    ) -> LogicalRuntimeId {
        let runtime_id = LogicalRuntimeId::for_session(session.id());
        store
            .commit_session_snapshot(
                &runtime_id,
                SerializedSessionSnapshot {
                    session_snapshot: Arc::new(serde_json::to_vec(session).unwrap()),
                },
            )
            .await
            .expect("the committed document");
        runtime_id
    }

    /// Write the fixture into an empty `store`.
    pub(crate) async fn seed(store: &dyn RuntimeStore) -> ScanFixture {
        let mut listed = HashSet::new();
        let mut unlisted = HashSet::new();

        // Nonterminal inputs, beside a lifecycle with no run.
        let nonterminal = LogicalRuntimeId::new("scan-nonterminal-inputs");
        store
            .commit_machine_lifecycle(&nonterminal, lifecycle(&nonterminal, None), &[])
            .await
            .unwrap();
        store
            .persist_input_state(&nonterminal, &accepted())
            .await
            .unwrap();
        listed.insert(nonterminal);

        // A pending terminal owner whose own input is terminal, beside a
        // lifecycle with no run.
        let owner = LogicalRuntimeId::new("scan-pending-terminal-owner");
        store
            .commit_machine_lifecycle(&owner, lifecycle(&owner, None), &[])
            .await
            .unwrap();
        let (pending, _) = crate::store::pending_terminal_owner_fixture(InputId::new(), false);
        store
            .persist_input_state(&owner, &consumed(pending))
            .await
            .unwrap();
        listed.insert(owner);

        // A provisional tail: a run's candidate on a committed base, with
        // no input and no lifecycle.
        let mut session = meerkat_core::Session::new();
        session.push(Message::User(UserMessage::text("the committed base")));
        let tail = commit_session(store, &session).await;
        let base = store
            .load_whole_blob_store_authority(&tail)
            .await
            .unwrap()
            .expect("a committed base");
        session.push(Message::User(UserMessage::text("the run's first round")));
        store
            .write_prepared_whole_blob_provisional_tail(
                &tail,
                PreparedWholeBlobProvisionalTail::prepare_from_session(
                    base,
                    RunId::new(),
                    1,
                    &session,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        listed.insert(tail);

        // A lifecycle that names a current run, and nothing else.
        let running = LogicalRuntimeId::new("scan-current-run");
        store
            .commit_machine_lifecycle(&running, lifecycle(&running, Some(RunId::new())), &[])
            .await
            .unwrap();
        listed.insert(running);

        // Terminal input rows and no lifecycle record.
        let orphan = LogicalRuntimeId::new("scan-inputs-without-lifecycle");
        store
            .persist_input_state(
                &orphan,
                &consumed(StoredInputState::new_accepted(InputId::new())),
            )
            .await
            .unwrap();
        listed.insert(orphan);

        // Idle: terminal inputs, a lifecycle with no run, no tail.
        let idle = LogicalRuntimeId::new("scan-idle");
        store
            .commit_machine_lifecycle(&idle, lifecycle(&idle, None), &[])
            .await
            .unwrap();
        store
            .persist_input_state(
                &idle,
                &consumed(StoredInputState::new_accepted(InputId::new())),
            )
            .await
            .unwrap();
        unlisted.insert(idle);

        // Archived, though it still holds a nonterminal input.
        let mut archived_session = meerkat_core::Session::new();
        archived_session.push(Message::User(UserMessage::text("an archived session")));
        let archived = LogicalRuntimeId::for_session(archived_session.id());
        store
            .persist_input_state(&archived, &accepted())
            .await
            .unwrap();
        archived_session
            .set_lifecycle_terminal(meerkat_core::SessionLifecycleTerminal::Archived)
            .unwrap();
        commit_session(store, &archived_session).await;
        unlisted.insert(archived);

        ScanFixture { listed, unlisted }
    }

    /// The scan of a store that holds exactly `fixture` lists each runtime
    /// with recovery work once, and no other runtime.
    pub(crate) async fn assert_lists_exactly(store: &dyn RuntimeStore, fixture: &ScanFixture) {
        let listed = store
            .list_runtimes_with_recovery_work()
            .await
            .expect("the scan answers");
        let unique: HashSet<LogicalRuntimeId> = listed.iter().cloned().collect();
        assert_eq!(
            unique.len(),
            listed.len(),
            "each runtime is listed once: {listed:?}"
        );
        assert_eq!(
            unique, fixture.listed,
            "the scan lists exactly the runtimes with recovery work"
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use meerkat_core::SessionLifecycleTerminal;
    use meerkat_core::lifecycle::RunId;

    use super::*;
    use crate::runtime_state::RuntimeState;
    use crate::store::{
        MachineLifecycleBindingFacts, MachineLifecycleCommit, MachineLifecyclePreRunPhase,
        MachineLifecycleRunFacts, RuntimeSessionPersistenceProfile, SupervisorAuthoritySnapshot,
    };

    fn facts(nonterminal: bool, pending_owners: bool, tail: bool, inputs: bool) -> BootFactsV1 {
        BootFactsV1 {
            v: BootFactsV1::VERSION,
            nonterminal,
            pending_owners,
            tail,
            inputs,
        }
    }

    /// The stored bytes of a lifecycle naming `current_run` or no run.
    fn lifecycle(current_run: Option<RunId>) -> Vec<u8> {
        let state = if current_run.is_some() {
            RuntimeState::Running
        } else {
            RuntimeState::Idle
        };
        MachineLifecycleCommit::new_with_binding_run_and_unregister_progress(
            state,
            MachineLifecycleBindingFacts::new(
                Some("predicate-runtime".to_string()),
                Some(1),
                Some(1),
                Some("epoch-1".to_string()),
            ),
            MachineLifecycleRunFacts::new(current_run, Some(MachineLifecyclePreRunPhase::Idle)),
            SupervisorAuthoritySnapshot::UnboundNoReceipt,
            None,
        )
        .store_record()
        .encode()
        .unwrap()
    }

    fn catalog(terminal: Option<SessionLifecycleTerminal>) -> RuntimeSessionCatalogEntry {
        let mut session = meerkat_core::Session::new();
        if let Some(terminal) = terminal {
            session.set_lifecycle_terminal(terminal).unwrap();
        }
        RuntimeSessionCatalogEntry::from_session(
            &session,
            RuntimeSessionPersistenceProfile::WholeBlobV1,
            None,
        )
        .unwrap()
    }

    /// Each kind of work, alone, makes an obligation; no work makes none;
    /// and an archived catalog entry makes none whatever the facts say.
    #[test]
    fn boot_obligation_scan_predicate_names_each_kind_of_work_and_no_archived_runtime() {
        let idle = lifecycle(None);
        let running = lifecycle(Some(RunId::new()));
        let undecodable = b"not a lifecycle record".to_vec();
        let rows: [(&str, BootFactsV1, Option<&[u8]>, bool); 10] = [
            ("nothing", facts(false, false, false, false), None, false),
            (
                "a lifecycle with no run",
                facts(false, false, false, false),
                Some(idle.as_slice()),
                false,
            ),
            (
                "terminal inputs beside a lifecycle with no run",
                facts(false, false, false, true),
                Some(idle.as_slice()),
                false,
            ),
            (
                "nonterminal inputs",
                facts(true, false, false, true),
                Some(idle.as_slice()),
                true,
            ),
            (
                "pending terminal owners",
                facts(false, true, false, true),
                Some(idle.as_slice()),
                true,
            ),
            (
                "a provisional tail",
                facts(false, false, true, false),
                None,
                true,
            ),
            (
                "a lifecycle that names a current run",
                facts(false, false, false, false),
                Some(running.as_slice()),
                true,
            ),
            (
                "a lifecycle that does not decode",
                facts(false, false, false, false),
                Some(undecodable.as_slice()),
                true,
            ),
            (
                "input rows with no lifecycle",
                facts(false, false, false, true),
                None,
                true,
            ),
            (
                "every kind of work",
                facts(true, true, true, true),
                Some(running.as_slice()),
                true,
            ),
        ];
        let unmarked = catalog(None);
        let active = catalog(Some(SessionLifecycleTerminal::Active));
        let archived = catalog(Some(SessionLifecycleTerminal::Archived));
        for (name, facts, lifecycle, expected) in &rows {
            for (entry_name, entry) in [
                ("no catalog entry", None),
                ("a catalog entry with no terminal", Some(&unmarked)),
                ("an active catalog entry", Some(&active)),
            ] {
                assert_eq!(
                    runtime_has_recovery_obligation(facts, *lifecycle, entry),
                    *expected,
                    "{name} with {entry_name}"
                );
            }
            assert!(
                !runtime_has_recovery_obligation(facts, *lifecycle, Some(&archived)),
                "{name}: an archived runtime holds no recovery work"
            );
        }
    }
}
