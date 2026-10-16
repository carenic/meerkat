//! Governed MCP attachment: the machine-side control behind
//! [`McpAttachmentAuthority`].
//!
//! A surface prepares one authenticated [`McpAttachmentControlRequest`] for a
//! registered session before any discovery, spawn or configuration write. The
//! returned authority rechecks, at each effect entry and after any wait, that
//! the exact registration, executor attachment and scoped destination it was
//! prepared under are still current, and that the installed owners still
//! allow the effect (mandate, grant/account decision, review tier) under one
//! current observation. Each effect is entered at most once per prepared
//! control: the attempt is reserved only at the actual entry, after every
//! check passed, so a refusal before entry (Denied, Unavailable, NotCurrent,
//! ReviewRequired) consumes nothing and the same prepared control may be
//! asked again after the condition changes. A live attachment never
//! authorizes a configuration write.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use meerkat_authorization_contracts::work_association::NativeWorkTarget;
use meerkat_core::authorization::{
    OperationAuthorizationError, OperationRefusalKind, OwnerQualifiedTarget,
    PreparedOperationCheck, WorkAuthorizationContext,
};
use meerkat_core::mcp_attachment::{
    ConfiguredMcpAttachmentAuthority, McpAttachmentAuthority, McpAttachmentRefusal,
};
use meerkat_core::mcp_attachment_control::{McpAttachmentControlRequest, McpAttachmentEffect};
use meerkat_core::mcp_config::{McpScope, McpServerConfig};

use super::{
    MeerkatMachine, RuntimeExecutorAttachmentWitness, RuntimeLoopAttachmentSlot,
    RuntimeSessionEntry, RuntimeSessionRegistrationWitness,
};
use crate::driver::ephemeral::SharedIngressDslAuthority;

/// The one canonical owner-qualified destination of a bound session
/// document: its stable owner, the `session` namespace and its context (the
/// canonical session id). An attachment control's destination fact must be
/// exactly this for the target the session is bound to.
pub fn scoped_destination_target(bound: &NativeWorkTarget) -> OwnerQualifiedTarget {
    OwnerQualifiedTarget {
        authority: bound.logical_owner.clone(),
        namespace: Arc::from("session"),
        id: Arc::from(bound.context.as_str()),
    }
}

impl MeerkatMachine {
    /// Prepare one governed MCP attachment for `registration`'s session.
    ///
    /// The session must be this machine's exact current registration with a
    /// live executor attachment, `bound` must be the target its document is
    /// currently bound to, and the control's destination fact must be that
    /// target's canonical destination ([`scoped_destination_target`]): a
    /// claimed destination is joined with the actual binding, never taken on
    /// its word. The installed owners must currently allow attaching the
    /// control's server. Nothing is attached or written here.
    pub async fn prepare_mcp_attachment(
        self: &Arc<Self>,
        registration: &RuntimeSessionRegistrationWitness,
        bound: &NativeWorkTarget,
        control: Arc<McpAttachmentControlRequest>,
    ) -> Result<Arc<dyn McpAttachmentAuthority>, McpAttachmentRefusal> {
        if control.session_id() != registration.session_id() {
            return Err(McpAttachmentRefusal::Mismatch);
        }
        let claimed = &control.facts().destination;
        let canonical = scoped_destination_target(bound);
        if bound.logical_runtime.as_str()
            != Self::logical_runtime_id(registration.session_id()).to_string()
            || claimed.authority != canonical.authority
            || claimed.namespace != canonical.namespace
            || claimed.id != canonical.id
        {
            return Err(McpAttachmentRefusal::Mismatch);
        }
        let bound_binding = crate::input_authority::context_target_binding(bound)
            .map_err(|_| McpAttachmentRefusal::Mismatch)?;
        if !registration.belongs_to(self) {
            return Err(McpAttachmentRefusal::NotCurrent);
        }
        let host = self
            .native_work_authorization_host
            .get()
            .ok_or(McpAttachmentRefusal::Unavailable)?
            .host()
            .clone();
        let (attachment, driver, agent) = {
            let sessions = self.sessions.read().await;
            let entry = sessions
                .get(registration.session_id())
                .filter(|entry| registration.matches_entry(entry))
                .ok_or(McpAttachmentRefusal::NotCurrent)?;
            let attachment = live_attachment(self, registration.session_id(), entry)
                .ok_or(McpAttachmentRefusal::NotCurrent)?;
            (
                attachment,
                Arc::clone(&entry.driver),
                entry.agent_binding.clone(),
            )
        };
        // This registration's own agent selection for the control's server,
        // when the host selects per agent; decided before any preparation.
        let agent_selection = host.mcp_attachment_agent_selection(agent);
        if let Some(selection) = &agent_selection {
            selection.authorize_configured(control.server())?;
        }
        let destinations = driver.lock().await.shared_dsl_authority();
        let destination = current_destination(&destinations)?;
        if destination != bound_binding {
            return Err(McpAttachmentRefusal::NotCurrent);
        }
        let context = host
            .mcp_attachment_authorization(Arc::clone(&control))
            .map_err(refusal)?;
        let apply = PreparedOperationCheck::prepare(
            context.clone(),
            control.effect_binding(McpAttachmentEffect::Apply),
        )
        .map_err(refusal)?;
        Ok(Arc::new(NativeMcpAttachment {
            machine: Arc::downgrade(self),
            registration: registration.clone(),
            attachment,
            destinations,
            destination,
            control,
            agent_selection,
            context,
            apply,
            applied: AtomicBool::new(false),
            persisted: Mutex::new(Vec::new()),
        }))
    }
}

/// The exact live executor attachment of `entry`, if it has one.
fn live_attachment(
    machine: &MeerkatMachine,
    session_id: &meerkat_core::types::SessionId,
    entry: &RuntimeSessionEntry,
) -> Option<RuntimeExecutorAttachmentWitness> {
    if !entry.generated_executor_registration_active() || !entry.attachment_is_live() {
        return None;
    }
    let RuntimeLoopAttachmentSlot::Attached(attachment) = &entry.attachment_slot else {
        return None;
    };
    Some(RuntimeExecutorAttachmentWitness::new(
        Arc::downgrade(&machine.shared),
        session_id.clone(),
        entry.epoch_id.clone(),
        attachment.id,
    ))
}

/// The session document's current scoped destination. A governed attachment
/// serves one bound destination; without one there is nothing to attach to.
fn current_destination(
    destinations: &SharedIngressDslAuthority,
) -> Result<String, McpAttachmentRefusal> {
    destinations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .state()
        .scoped_context_binding
        .clone()
        .ok_or(McpAttachmentRefusal::NotCurrent)
}

fn refusal(error: OperationAuthorizationError) -> McpAttachmentRefusal {
    match error {
        OperationAuthorizationError::Refused(refused) => match refused.kind() {
            OperationRefusalKind::Denied => McpAttachmentRefusal::Denied,
            OperationRefusalKind::MalformedFacts => McpAttachmentRefusal::Mismatch,
            OperationRefusalKind::ReprepareRequired => McpAttachmentRefusal::Unavailable,
        },
        OperationAuthorizationError::Unavailable
        | OperationAuthorizationError::ObservationUnavailable(_) => {
            McpAttachmentRefusal::Unavailable
        }
    }
}

struct NativeMcpAttachment {
    machine: Weak<MeerkatMachine>,
    registration: RuntimeSessionRegistrationWitness,
    attachment: RuntimeExecutorAttachmentWitness,
    destinations: SharedIngressDslAuthority,
    destination: String,
    control: Arc<McpAttachmentControlRequest>,
    /// The registration's per-agent selection conjunct, if the host selects
    /// per agent.
    agent_selection: Option<Arc<dyn ConfiguredMcpAttachmentAuthority>>,
    context: WorkAuthorizationContext,
    apply: PreparedOperationCheck,
    applied: AtomicBool,
    persisted: Mutex<Vec<McpScope>>,
}

impl NativeMcpAttachment {
    /// The registration, executor attachment and destination this control
    /// was prepared under are all still current.
    fn native_current(&self) -> Result<(), McpAttachmentRefusal> {
        let machine = self
            .machine
            .upgrade()
            .ok_or(McpAttachmentRefusal::NotCurrent)?;
        {
            let sessions = machine
                .sessions
                .try_read()
                .map_err(|_| McpAttachmentRefusal::Unavailable)?;
            let entry = sessions
                .get(self.registration.session_id())
                .filter(|entry| self.registration.matches_entry(entry))
                .ok_or(McpAttachmentRefusal::NotCurrent)?;
            if live_attachment(&machine, self.registration.session_id(), entry).as_ref()
                != Some(&self.attachment)
            {
                return Err(McpAttachmentRefusal::NotCurrent);
            }
        }
        if current_destination(&self.destinations)? != self.destination {
            return Err(McpAttachmentRefusal::NotCurrent);
        }
        Ok(())
    }

    /// The registration's bound agent still has a currently resolving lineage
    /// selected for the control's own server (when the host selects per
    /// agent). Checked for every effect, so the server's hydration and the
    /// destination's publication of a live attachment, and each
    /// configuration write, are all decided for that agent and server. The
    /// exact-account binding of the attachment joins this check.
    fn agent_selected(&self) -> Result<(), McpAttachmentRefusal> {
        match &self.agent_selection {
            Some(selection) => selection.authorize_configured(self.control.server()),
            None => Ok(()),
        }
    }

    /// The final entry of one prepared effect: native currentness, current
    /// owners and tier, then the effect's one reservation, the entry
    /// observation, and current owners and tier again (the observer is
    /// arbitrary owner code). A refusal before the reservation consumes
    /// nothing; once reserved, the effect is spent whatever follows.
    fn enter(
        &self,
        check: &PreparedOperationCheck,
        reserve: impl FnOnce() -> bool,
    ) -> Result<(), McpAttachmentRefusal> {
        self.native_current()?;
        self.agent_selected()?;
        let current = check.current().map_err(refusal)?;
        current
            .require_unreviewed_entry()
            .map_err(|_| McpAttachmentRefusal::ReviewRequired)?;
        if !reserve() {
            return Err(McpAttachmentRefusal::NotCurrent);
        }
        current
            .observe_entry(meerkat_core::OperationEntryBoundary::McpAttachment)
            .map_err(|_| McpAttachmentRefusal::Unavailable)?;
        let refreshed = current.current().map_err(refusal)?;
        refreshed
            .require_unreviewed_entry()
            .map_err(|_| McpAttachmentRefusal::ReviewRequired)?;
        // The agent selection runs arbitrary owner code (its clock), during
        // which native state can change without any grant publication: the
        // native-only read is last.
        self.agent_selected()?;
        self.native_current()
    }
}

impl McpAttachmentAuthority for NativeMcpAttachment {
    fn server(&self) -> &McpServerConfig {
        self.control.server()
    }

    fn authorize_apply(&self) -> Result<(), McpAttachmentRefusal> {
        // One live attachment per control, reserved at the entry itself.
        self.enter(&self.apply, || !self.applied.swap(true, Ordering::AcqRel))
    }

    fn authorize_persist(&self, scope: McpScope) -> Result<(), McpAttachmentRefusal> {
        // The configuration write is its own effect: prepared now under the
        // same owners, never covered by the live attachment's decision. One
        // write per scope, reserved at the entry itself.
        let check = PreparedOperationCheck::prepare(
            self.context.clone(),
            self.control
                .effect_binding(McpAttachmentEffect::Persist(scope)),
        )
        .map_err(refusal)?;
        self.enter(&check, || {
            let mut persisted = self
                .persisted
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if persisted.contains(&scope) {
                return false;
            }
            persisted.push(scope);
            true
        })
    }
}
