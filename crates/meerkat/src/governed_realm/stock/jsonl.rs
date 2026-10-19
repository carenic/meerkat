//! The stock host's governed JSONL faces.
//!
//! A governed JSONL connection served by the stock host in the local-operator
//! posture authenticates the same local operator through the same ingress as
//! a CLI prompt, so it uses the same composition: the session is bound to its
//! canonical destination by the one generation authority
//! ([`super::destination::bind_stock_session`]), and every input is bound to
//! an authenticated original recorded in the realm's ledger
//! ([`StockGovernance::bind_prompt`]). A connection of a host-authenticated
//! composition carries the caller its host authenticated instead
//! ([`StockGovernance::jsonl_caller_facts`],
//! [`StockGovernance::bind_jsonl_caller_input`]); its sessions keep the same
//! canonical owner and destination. Nothing here reads a destination back
//! from the machine, the wire or a claim.

use meerkat_authorization_contracts::evidence::HistoricalEvidenceRef;
use meerkat_authorization_contracts::work_association::NativeWorkTarget;
use meerkat_core::connection::RealmId;
use meerkat_core::{ControllerModelClient, PrincipalRef, SessionId};
use meerkat_runtime::MeerkatMachine;
use meerkat_runtime::input::Input;

use super::caller::{StockAuthenticatedCaller, StockCallerPosture};
use super::originals::PendingOriginal;
use super::{StockGovernance, StockGovernanceError, destination, process_id};

/// The facts the stock host authenticated for one governed JSONL
/// connection: the requester (the local operator, or the host's
/// authenticated caller) through the stock ingress, in this realm. The
/// authentication reference identifies the observation of the requester's
/// authentication; it is not authentication by itself.
#[derive(Clone)]
pub struct StockJsonlConnectionFacts {
    pub requester: PrincipalRef,
    pub ingress_actor: PrincipalRef,
    pub realm: RealmId,
    pub authentication: HistoricalEvidenceRef,
}

/// One bound JSONL input: the input bound to its authenticated original, the
/// exact destination it names (now the session's current binding), and the
/// candidate original, admitted once the runtime accepted that input.
pub struct StockJsonlInput {
    pub input: Input,
    pub target: NativeWorkTarget,
    pub original: PendingOriginal,
}

impl std::fmt::Debug for StockJsonlConnectionFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StockJsonlConnectionFacts")
            .finish_non_exhaustive()
    }
}

impl StockGovernance {
    /// The operator facts of one governed JSONL connection of this host, in
    /// the local-operator posture only.
    ///
    /// # Errors
    /// [`StockGovernanceError::RequesterUnsupported`] for a
    /// host-authenticated composition, whose operator is no requester.
    pub fn jsonl_connection_facts(
        &self,
    ) -> Result<StockJsonlConnectionFacts, StockGovernanceError> {
        if !matches!(self.posture, StockCallerPosture::LocalOperator) {
            return Err(StockGovernanceError::RequesterUnsupported(
                "a host-authenticated connection carries its authenticated caller".to_string(),
            ));
        }
        Ok(StockJsonlConnectionFacts {
            requester: self.principals.operator.clone(),
            ingress_actor: self.principals.ingress_actor.clone(),
            realm: self.realm.clone(),
            authentication: self.evidence(
                "operator-authentication",
                process_id(),
                uuid::Uuid::now_v7(),
            )?,
        })
    }

    /// The facts of one governed JSONL connection of `caller`, a caller the
    /// trusted host authenticated, in the host-authenticated posture only.
    ///
    /// # Errors
    /// [`StockGovernanceError::RequesterUnsupported`] for the
    /// local-operator composition, which admits no caller.
    pub fn jsonl_caller_facts(
        &self,
        caller: &StockAuthenticatedCaller,
    ) -> Result<StockJsonlConnectionFacts, StockGovernanceError> {
        if !matches!(self.posture, StockCallerPosture::HostAuthenticated(_)) {
            return Err(StockGovernanceError::RequesterUnsupported(
                "the local-operator composition admits only its operator".to_string(),
            ));
        }
        Ok(StockJsonlConnectionFacts {
            requester: caller.requester().clone(),
            ingress_actor: self.principals.ingress_actor.clone(),
            realm: self.realm.clone(),
            authentication: caller.authentication().clone(),
        })
    }

    /// Bind a session this host just created to its canonical destination,
    /// at the next generation, before any governed input or MCP entry, and
    /// return the exact bound target.
    pub async fn bind_jsonl_session(
        &self,
        machine: &MeerkatMachine,
        session_id: &SessionId,
    ) -> Result<NativeWorkTarget, StockGovernanceError> {
        destination::bind_stock_session(machine, &self.principals.operator, session_id)
            .await
            .map(|destination| destination.target().clone())
            .map_err(|error| StockGovernanceError::Destination(error.to_string()))
    }

    /// Bind one JSONL input exactly as a CLI prompt is bound
    /// ([`Self::bind_prompt`]), and report the destination it names.
    pub async fn bind_jsonl_input(
        &self,
        machine: &MeerkatMachine,
        session_id: &SessionId,
        input: Input,
        controller: ControllerModelClient,
    ) -> Result<StockJsonlInput, StockGovernanceError> {
        let (input, original) = self
            .bind_prompt(machine, session_id, input, controller)
            .await?;
        jsonl_input(input, original)
    }

    /// Bind one JSONL input of `caller` exactly as
    /// [`Self::bind_caller_prompt`] binds it, and report the destination it
    /// names.
    pub async fn bind_jsonl_caller_input(
        &self,
        machine: &MeerkatMachine,
        session_id: &SessionId,
        input: Input,
        controller: ControllerModelClient,
        caller: &StockAuthenticatedCaller,
    ) -> Result<StockJsonlInput, StockGovernanceError> {
        let (input, original) = self
            .bind_caller_prompt(machine, session_id, input, controller, caller)
            .await?;
        jsonl_input(input, original)
    }
}

/// One bound input with the destination it names.
fn jsonl_input(
    input: Input,
    original: PendingOriginal,
) -> Result<StockJsonlInput, StockGovernanceError> {
    // Defence in depth: the governed JSONL connection binds only the prompt
    // of a turn it starts, and `bind_prompt` already refuses any other input
    // before anything is written, so no non-prompt reaches this check today.
    let Input::Prompt(prompt) = &input else {
        return Err(StockGovernanceError::Binding(
            "a governed JSONL input is a prompt".into(),
        ));
    };
    let target = prompt
        .header
        .authority_association
        .as_ref()
        .map(|association| association.candidate().target.clone())
        .ok_or_else(|| {
            StockGovernanceError::Binding("the bound input names no destination".into())
        })?;
    Ok(StockJsonlInput {
        input,
        target,
        original,
    })
}
