//! The agent a registered session acts as, retained by its registration.
//!
//! A trusted host that selected and may invoke a qualified agent principal
//! and the exact grant lineage it selected per MCP server (with the
//! represented subject that mandate applies to, when there is one) binds
//! that selection to the session's actual registration before the session's agent is built. The binding is
//! a fact of that registration: a replaced, detached or reopened
//! registration starts without one, and the host binds again from its own
//! trusted selection and the restored current lineage. Nothing is persisted,
//! inferred from inputs or labels, or minted here; an absent binding is
//! typed unavailable, never implicit permission.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use meerkat_authorization_contracts::work_association::GrantLineageRef;
use meerkat_core::auth::PrincipalRef;

use super::{MeerkatMachine, RuntimeSessionRegistrationWitness};
use crate::traits::RuntimeDriverError;

/// A trusted selection of the agent a session acts as: the qualified
/// executor principal, the exact worker grant lineage selected for each MCP
/// server it may use, and the represented subject its mandate applies to
/// (explicitly none when it acts for itself). The agent's tool catalog is
/// base work: the work's authority basis selects its lineage, not this
/// mandate.
///
/// The per-server lineages are selection data, not permission: each names
/// which root-to-leaf lineage the native grant owner resolves, at use, for
/// that one server. They are never united or intersected. A server with no
/// selected lineage, or whose lineage no longer resolves, refuses only that
/// server's operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionAgentMandate {
    executor: PrincipalRef,
    servers: BTreeMap<String, Vec<GrantLineageRef>>,
    represented_subject: Option<PrincipalRef>,
}

/// Why a session agent mandate is not well formed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentMandateError {
    #[error("the session agent's executor principal is not qualified")]
    UnqualifiedExecutor,
    #[error("the session agent's represented subject is not qualified")]
    UnqualifiedSubject,
    #[error("the session agent's mandate selects a lineage for an unnamed server")]
    UnnamedServer,
    #[error("the session agent's mandate selects an empty grant lineage")]
    EmptyLineage,
    #[error("the session agent's mandate selects no MCP server")]
    NoSelection,
}

impl SessionAgentMandate {
    /// # Errors
    /// Refuses unqualified principals, an unnamed server, an empty lineage
    /// and a mandate that selects no server (leave such a registration
    /// unbound).
    pub fn new(
        executor: PrincipalRef,
        servers: BTreeMap<String, Vec<GrantLineageRef>>,
        represented_subject: Option<PrincipalRef>,
    ) -> Result<Self, AgentMandateError> {
        executor
            .validate_qualified()
            .map_err(|_| AgentMandateError::UnqualifiedExecutor)?;
        if let Some(subject) = &represented_subject {
            subject
                .validate_qualified()
                .map_err(|_| AgentMandateError::UnqualifiedSubject)?;
        }
        if servers.keys().any(String::is_empty) {
            return Err(AgentMandateError::UnnamedServer);
        }
        if servers.values().any(Vec::is_empty) {
            return Err(AgentMandateError::EmptyLineage);
        }
        if servers.is_empty() {
            return Err(AgentMandateError::NoSelection);
        }
        Ok(Self {
            executor,
            servers,
            represented_subject,
        })
    }

    pub fn executor(&self) -> &PrincipalRef {
        &self.executor
    }

    /// The lineage selected for `server`, if any. `None` means nothing was
    /// selected for that server; it says nothing about any other server.
    pub fn server_lineage(&self, server: &str) -> Option<&[GrantLineageRef]> {
        self.servers.get(server).map(Vec::as_slice)
    }

    /// Every server's selected lineage, in server-name order.
    pub fn server_lineages(&self) -> impl Iterator<Item = (&str, &[GrantLineageRef])> {
        self.servers
            .iter()
            .map(|(server, lineage)| (server.as_str(), lineage.as_slice()))
    }

    pub fn represented_subject(&self) -> Option<&PrincipalRef> {
        self.represented_subject.as_ref()
    }
}

/// Why a session registration reports no agent mandate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AgentBindingUnavailable {
    /// The registration is current but its trusted host bound no mandate.
    #[error("the session registration has no bound agent mandate")]
    Unbound,
    /// The registration ended (unregistered, replaced, discarded, detached
    /// to a recovery anchor, or its owner disposed): it is no longer current.
    #[error("the session registration's agent binding has ended")]
    Retired,
}

/// One registration's agent binding slot. Every registration owns a fresh
/// slot; a clone observes the same registration's binding and never another
/// registration's. When the registration ends (unregistered or replaced) its
/// slot is retired, terminally: a retired slot reports no mandate to any
/// holder, including an authority exported while it was current, and can
/// never be bound again.
#[derive(Clone, Default)]
pub struct SessionAgentBinding(Arc<AgentSlot>);

#[derive(Default)]
struct AgentSlot {
    mandate: OnceLock<SessionAgentMandate>,
    retired: AtomicBool,
}

impl std::fmt::Debug for SessionAgentBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionAgentBinding")
            .field("bound", &self.0.mandate.get().is_some())
            .field("retired", &self.0.retired.load(Ordering::Acquire))
            .finish()
    }
}

impl SessionAgentBinding {
    /// The mandate the registration's trusted host bound, while that
    /// registration is current.
    ///
    /// # Errors
    /// [`AgentBindingUnavailable`] until the host binds one, and for good
    /// once the registration ended.
    pub fn current(&self) -> Result<&SessionAgentMandate, AgentBindingUnavailable> {
        if self.0.retired.load(Ordering::Acquire) {
            return Err(AgentBindingUnavailable::Retired);
        }
        self.0.mandate.get().ok_or(AgentBindingUnavailable::Unbound)
    }

    /// End this registration's binding for good. Called by the registration
    /// owner where the registration leaves the machine.
    pub(crate) fn retire(&self) {
        self.0.retired.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn same_slot(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    fn bind(&self, mandate: SessionAgentMandate) -> Result<(), RuntimeDriverError> {
        if self.0.retired.load(Ordering::Acquire) {
            return Err(RuntimeDriverError::StaleAuthority {
                reason: "the session registration's agent binding has ended".to_string(),
            });
        }
        let bound = self.0.mandate.get_or_init(|| mandate.clone());
        if bound == &mandate {
            Ok(())
        } else {
            Err(RuntimeDriverError::ValidationFailed {
                reason: "the session registration is already bound to another agent mandate"
                    .to_string(),
            })
        }
    }
}

impl MeerkatMachine {
    /// Bind the agent the session of `registration` acts as.
    ///
    /// Only the installed governed host's composition may do this, for the
    /// exact current registration, before the session's agent is built. A
    /// registration's binding is immutable: binding the same mandate again is
    /// accepted, another mandate is refused.
    pub async fn bind_session_agent_mandate(
        &self,
        registration: &RuntimeSessionRegistrationWitness,
        mandate: SessionAgentMandate,
    ) -> Result<(), RuntimeDriverError> {
        if self.native_work_authorization_host.get().is_none() {
            return Err(RuntimeDriverError::ValidationFailed {
                reason: "a session agent mandate requires the governed work host".to_string(),
            });
        }
        let stale = || RuntimeDriverError::StaleAuthority {
            reason: "agent mandate registration witness is not current".to_string(),
        };
        if !registration.belongs_to(self) {
            return Err(stale());
        }
        let _gate = self
            .lock_current_durability_ready_session_mutation_gate(registration.session_id())
            .await?;
        let sessions = self.sessions.read().await;
        let entry = sessions.get(registration.session_id()).ok_or_else(stale)?;
        if !registration.matches_entry(entry) || &entry.epoch_id != registration.epoch_id() {
            return Err(stale());
        }
        entry.agent_binding.bind(mandate)
    }

    /// Install `gate` as the tool-surface gate of the exact current
    /// registration, before the session's agent is built: the agent's
    /// prepared surface is then decided by it before the agent is finished.
    /// A registration has one gate; a replaced registration starts without
    /// one.
    pub async fn bind_session_tool_surface_gate(
        &self,
        registration: &RuntimeSessionRegistrationWitness,
        gate: Arc<dyn meerkat_core::ToolSurfaceGate>,
    ) -> Result<(), RuntimeDriverError> {
        let stale = || RuntimeDriverError::StaleAuthority {
            reason: "tool-surface gate registration witness is not current".to_string(),
        };
        if !registration.belongs_to(self) {
            return Err(stale());
        }
        let _gate = self
            .lock_current_durability_ready_session_mutation_gate(registration.session_id())
            .await?;
        let sessions = self.sessions.read().await;
        let entry = sessions.get(registration.session_id()).ok_or_else(stale)?;
        if !registration.matches_entry(entry) || &entry.epoch_id != registration.epoch_id() {
            return Err(stale());
        }
        entry.tool_surface_gate.install(gate).map_err(|refusal| {
            RuntimeDriverError::ValidationFailed {
                reason: refusal.reason,
            }
        })
    }

    /// The tool-surface gate installed on `session_id`'s current
    /// registration, if any.
    pub async fn session_tool_surface_gate(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Option<Arc<dyn meerkat_core::ToolSurfaceGate>> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .and_then(|entry| entry.tool_surface_gate.current())
    }

    /// The current registration's agent binding slot for `session_id`.
    #[cfg(test)]
    pub(crate) async fn session_agent_binding(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Option<SessionAgentBinding> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .map(|entry| entry.agent_binding.clone())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use meerkat_authorization_contracts::evidence::EvidenceId;
    use meerkat_core::auth::PrincipalKind;
    use meerkat_core::types::SessionId;

    struct AdmitsNothing;
    impl crate::input_authority::NativeWorkAuthorizationHost for AdmitsNothing {
        fn authenticate_association(
            &self,
            _: &crate::identifiers::LogicalRuntimeId,
            _: &crate::input::Input,
            _: &crate::input_authority::NativeIngressContext,
            _: &meerkat_authorization_contracts::work_association::InputAuthorityAssociation,
        ) -> Result<(), crate::input_authority::NativeAdmissionError> {
            Err(
                meerkat_core::OperationRefused::new(meerkat_core::OperationRefusalKind::Denied)
                    .into(),
            )
        }
        fn work_context(
            &self,
            _: &crate::input_authority::NativeWorkBatch,
        ) -> Result<
            meerkat_core::authorization::WorkAuthorizationContext,
            crate::input_authority::NativeWorkContextError,
        > {
            Err(crate::input_authority::NativeWorkContextError::MalformedAcceptedWork)
        }
    }

    fn agent(name: &str) -> PrincipalRef {
        PrincipalRef::in_domain(
            PrincipalKind::SharedAgent,
            name,
            meerkat_core::TrustDomainId::new("agents").unwrap(),
        )
        .unwrap()
    }

    fn lineage(grant: &str) -> GrantLineageRef {
        GrantLineageRef {
            root_authority: agent("operator"),
            authority_namespace: EvidenceId::new("grants").unwrap(),
            authority_generation: 1,
            authority_incarnation:
                meerkat_authorization_contracts::grant::GrantAuthorityIncarnation::from_uuid(
                    uuid::Uuid::parse_str("6f1c2b9e-3a4d-4f8e-9c1b-2d3e4f5a6b7c").unwrap(),
                )
                .unwrap(),
            grant_id: EvidenceId::new(grant).unwrap(),
            issued_revision: 1,
        }
    }

    fn servers(grant: &str) -> BTreeMap<String, Vec<GrantLineageRef>> {
        BTreeMap::from([("server".to_owned(), vec![lineage(grant)])])
    }

    fn mandate(name: &str, grant: &str) -> SessionAgentMandate {
        SessionAgentMandate::new(agent(name), servers(grant), None).unwrap()
    }

    fn governed() -> MeerkatMachine {
        MeerkatMachine::ephemeral()
            .with_native_work_authorization_host(Arc::new(AdmitsNothing))
            .unwrap()
    }

    async fn registered(
        machine: &MeerkatMachine,
    ) -> (SessionId, RuntimeSessionRegistrationWitness) {
        let session = SessionId::new();
        machine.register_session(session.clone()).await.unwrap();
        let registration = machine
            .current_session_registration_witness(&session)
            .await
            .unwrap();
        (session, registration)
    }

    #[test]
    fn a_mandate_needs_qualified_principals_and_a_selection() {
        let unqualified = PrincipalRef::new(PrincipalKind::SharedAgent, "loose").unwrap();
        assert_eq!(
            SessionAgentMandate::new(unqualified.clone(), servers("g"), None),
            Err(AgentMandateError::UnqualifiedExecutor)
        );
        assert_eq!(
            SessionAgentMandate::new(agent("a"), servers("g"), Some(unqualified)),
            Err(AgentMandateError::UnqualifiedSubject)
        );
        assert_eq!(
            SessionAgentMandate::new(
                agent("a"),
                BTreeMap::from([("server".to_owned(), Vec::new())]),
                None
            ),
            Err(AgentMandateError::EmptyLineage)
        );
        assert_eq!(
            SessionAgentMandate::new(
                agent("a"),
                BTreeMap::from([(String::new(), vec![lineage("g")])]),
                None
            ),
            Err(AgentMandateError::UnnamedServer)
        );
        // A mandate carries MCP server lineages only: one that selects no
        // server is no mandate (the registration stays unbound).
        assert_eq!(
            SessionAgentMandate::new(agent("a"), BTreeMap::new(), None),
            Err(AgentMandateError::NoSelection)
        );
    }

    #[test]
    fn each_server_reads_only_its_own_selected_lineage() {
        let selected = SessionAgentMandate::new(
            agent("a"),
            BTreeMap::from([
                ("first".to_owned(), vec![lineage("g1")]),
                ("second".to_owned(), vec![lineage("g2")]),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(selected.server_lineage("first"), Some(&[lineage("g1")][..]));
        assert_eq!(
            selected.server_lineage("second"),
            Some(&[lineage("g2")][..])
        );
        assert_eq!(selected.server_lineage("third"), None);
        assert_eq!(
            selected
                .server_lineages()
                .map(|(server, _)| server)
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    #[tokio::test]
    async fn an_unbound_registration_is_unavailable_and_a_bound_one_is_immutable() {
        let machine = governed();
        let (session, registration) = registered(&machine).await;
        let slot = machine.session_agent_binding(&session).await.unwrap();
        assert_eq!(slot.current(), Err(AgentBindingUnavailable::Unbound));
        machine
            .bind_session_agent_mandate(&registration, mandate("agent-a", "grant-a"))
            .await
            .expect("bind the trusted selection");
        assert_eq!(slot.current(), Ok(&mandate("agent-a", "grant-a")));
        machine
            .bind_session_agent_mandate(&registration, mandate("agent-a", "grant-a"))
            .await
            .expect("the same mandate again is accepted");
        assert!(
            machine
                .bind_session_agent_mandate(&registration, mandate("agent-b", "grant-b"))
                .await
                .is_err(),
            "another agent cannot replace the registration's mandate"
        );
        assert_eq!(slot.current(), Ok(&mandate("agent-a", "grant-a")));
    }

    #[tokio::test]
    async fn a_mandate_needs_the_governed_host_and_the_current_registration() {
        let ungoverned = MeerkatMachine::ephemeral();
        let (_, registration) = registered(&ungoverned).await;
        assert!(
            ungoverned
                .bind_session_agent_mandate(&registration, mandate("agent-a", "grant-a"))
                .await
                .is_err(),
            "no governed host, no agent mandate"
        );

        let machine = governed();
        let (session, stale) = registered(&machine).await;
        machine.unregister_session(&session).await.unwrap();
        machine.register_session(session.clone()).await.unwrap();
        assert!(
            matches!(
                machine
                    .bind_session_agent_mandate(&stale, mandate("agent-a", "grant-a"))
                    .await,
                Err(RuntimeDriverError::StaleAuthority { .. })
            ),
            "a replaced registration's witness binds nothing"
        );
        let other = registered(&governed()).await.1;
        assert!(
            matches!(
                machine
                    .bind_session_agent_mandate(&other, mandate("agent-a", "grant-a"))
                    .await,
                Err(RuntimeDriverError::StaleAuthority { .. })
            ),
            "another machine's registration binds nothing here"
        );
    }

    #[tokio::test]
    async fn a_replaced_registration_starts_without_a_mandate() {
        let machine = governed();
        let (session, registration) = registered(&machine).await;
        machine
            .bind_session_agent_mandate(&registration, mandate("agent-a", "grant-a"))
            .await
            .unwrap();
        let old = machine.session_agent_binding(&session).await.unwrap();
        machine.unregister_session(&session).await.unwrap();
        machine.register_session(session.clone()).await.unwrap();
        let new = machine.session_agent_binding(&session).await.unwrap();
        assert!(!new.same_slot(&old), "the new registration owns a new slot");
        assert_eq!(
            new.current(),
            Err(AgentBindingUnavailable::Unbound),
            "a reopened registration borrows no earlier mandate"
        );
        assert_eq!(
            old.current(),
            Err(AgentBindingUnavailable::Retired),
            "the replaced registration's binding ended with it"
        );
    }

    /// Unregistering ends the registration's binding for good: a holder of
    /// the slot (as an authority exported while it was current would be)
    /// sees no mandate afterwards, and the ended slot can never be bound
    /// again, even with the mandate it had.
    #[tokio::test]
    async fn unregistering_ends_the_binding_for_every_holder() {
        let machine = governed();
        let (session, registration) = registered(&machine).await;
        machine
            .bind_session_agent_mandate(&registration, mandate("agent-a", "grant-a"))
            .await
            .unwrap();
        let exported = machine.session_agent_binding(&session).await.unwrap();
        assert!(exported.current().is_ok(), "current while registered");
        machine.unregister_session(&session).await.unwrap();
        assert_eq!(
            exported.current(),
            Err(AgentBindingUnavailable::Retired),
            "an exported holder is refused once the registration ended"
        );
        assert!(
            matches!(
                exported.bind(mandate("agent-a", "grant-a")),
                Err(RuntimeDriverError::StaleAuthority { .. })
            ),
            "an ended binding is terminal"
        );
        assert_eq!(exported.current(), Err(AgentBindingUnavailable::Retired));
    }

    struct SelectsNothing;

    #[async_trait::async_trait]
    impl meerkat_core::ToolSurfaceGate for SelectsNothing {
        async fn decide(
            &self,
            _surface: &meerkat_core::PreparedToolSurface,
        ) -> Result<meerkat_core::ToolSurfaceSelection, meerkat_core::ToolSurfaceRefusal> {
            Err(meerkat_core::ToolSurfaceRefusal::new("selects nothing"))
        }
    }

    /// A registration's tool-surface gate is carried by its runtime bindings
    /// and ends with the registration for every holder, a bindings clone
    /// included; a registration has one gate.
    #[tokio::test]
    async fn the_tool_surface_gate_ends_with_its_registration() {
        let machine = governed();
        let (session, registration) = registered(&machine).await;
        let bindings = machine
            .prepare_bindings(session.clone())
            .await
            .expect("bindings");
        assert!(bindings.tool_surface_gate().is_none(), "none installed yet");
        machine
            .bind_session_tool_surface_gate(&registration, Arc::new(SelectsNothing))
            .await
            .expect("install the host's gate");
        assert!(
            bindings.tool_surface_gate().is_some(),
            "bindings prepared earlier carry the registration's gate"
        );
        assert!(
            machine
                .bind_session_tool_surface_gate(&registration, Arc::new(SelectsNothing))
                .await
                .is_err(),
            "a registration has one gate"
        );
        machine.unregister_session(&session).await.unwrap();
        assert!(
            bindings.tool_surface_gate().is_none(),
            "an ended registration's gate is gone for every holder"
        );
        assert!(machine.session_tool_surface_gate(&session).await.is_none());
    }

    /// A durability discard (or any final transition) closes the
    /// registration's handle teardown gate: its agent binding ends with it.
    #[tokio::test]
    async fn closing_the_registration_teardown_gate_ends_the_binding() {
        let machine = governed();
        let (session, registration) = registered(&machine).await;
        machine
            .bind_session_agent_mandate(&registration, mandate("agent-a", "grant-a"))
            .await
            .unwrap();
        let exported = machine.session_agent_binding(&session).await.unwrap();
        machine
            .sessions
            .read()
            .await
            .get(&session)
            .unwrap()
            .close_handle_teardown_gate();
        assert_eq!(exported.current(), Err(AgentBindingUnavailable::Retired));
    }

    /// Disposing of the machine that owns the registration ends its binding
    /// for any holder that outlives the owner.
    #[tokio::test]
    async fn disposing_of_the_owner_ends_the_binding() {
        let machine = governed();
        let (session, registration) = registered(&machine).await;
        machine
            .bind_session_agent_mandate(&registration, mandate("agent-a", "grant-a"))
            .await
            .unwrap();
        let exported = machine.session_agent_binding(&session).await.unwrap();
        drop(registration);
        drop(machine);
        assert_eq!(exported.current(), Err(AgentBindingUnavailable::Retired));
    }
}
