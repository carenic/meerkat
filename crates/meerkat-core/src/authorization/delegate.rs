//! Process-local attribution of a builtin tool's own governed helper
//! operation (its delegate) to the dispatch that entered the tool.
//!
//! A delegate is minted only from a governed fenced dispatch's own prepared
//! call whose plan carries exactly the builtin operation its role requires,
//! and binds only its role's own facts under that call's exact work and
//! coordinates. The attribution is a binding, never a permit: it grants,
//! inherits and exempts nothing. The delegate's own owners decide it from
//! its own facts; nothing of the parent decision (allowance, validity,
//! review tier) carries over, and the parent's later currentness does not
//! stop a delegate of an invocation that already entered.

use std::fmt;
use std::sync::Arc;

use super::{
    AuthorizationOperation, ModelAuthorizationUse, OperationAuthorizationError,
    OperationAuthorizationFacts, OperationRefusalKind, OperationRefused,
    PreparedAuthorizationBinding, PreparedOperationCheck, WorkAuthorizationContext,
};
use crate::ServerToolKind;
use crate::tool_execution::{BuiltinToolOperation, ToolOperationTarget};

/// The role of a builtin tool's delegated operation. Exhaustive: a new role
/// makes every matcher map it. No role is an exemption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolDelegateRole {
    /// The model request of the builtin web_search's hosted-search helper:
    /// one inference with exactly one hosted search capability (provider web
    /// search or Google Search grounding) and no live channel.
    HostedSearchInference,
}

impl ToolDelegateRole {
    /// The builtin operation whose dispatch may perform this role.
    fn parent_operation(self) -> BuiltinToolOperation {
        match self {
            Self::HostedSearchInference => BuiltinToolOperation::WebSearch,
        }
    }

    /// Whether `operation` is this role's own kind of operation.
    fn admits(self, operation: &AuthorizationOperation) -> bool {
        match self {
            Self::HostedSearchInference => matches!(
                operation,
                AuthorizationOperation::Model(model)
                    if model.usage == ModelAuthorizationUse::Inference
                        && model.live_channel.is_none()
                        && matches!(
                            model.hosted_capabilities.as_ref(),
                            [ServerToolKind::WebSearch | ServerToolKind::GoogleSearch]
                        )
            ),
        }
    }
}

struct DelegateOrigin {
    parent: PreparedAuthorizationBinding,
    work: WorkAuthorizationContext,
    role: ToolDelegateRole,
}

/// The process-local origin of one builtin tool's delegated operation: the
/// exact binding of the governed call that entered the tool, that call's
/// work, and the delegate's role. Minted only by the core dispatch owner
/// from that call ([`crate::ToolDispatchContext::tool_delegate_attribution`]);
/// a historical audit id cannot construct it. Retaining it keeps no call
/// live and grants no operation, and it has no serialized form. One
/// attribution can prepare delegate requests until its run ends; each is its
/// own operation with a fresh id and needs its own current lineage.
///
/// # Behaviour contract for a call that already entered
///
/// Revoking the call's own lineage refuses later calls but does not cancel
/// an invocation that already entered: that invocation may still make its
/// authorized delegate request. The delegate passes its own current work,
/// contributor, route, capability and final entry checks, inherits no grant,
/// review result or controller authority of the call, and runs no further
/// authorization or review cycle of the call. Cancelling an entered
/// invocation on the call's revocation would be a different contract that
/// needs a live dependency on the call's currentness; this attribution does
/// not provide one. Neither contract undoes a provider effect that already
/// began or turns a local refusal into the end of the run.
///
/// ```compile_fail
/// fn requires_serialization<T: serde::Serialize>() {}
/// requires_serialization::<meerkat_core::authorization::ToolDelegateAttribution>();
/// ```
#[derive(Clone)]
pub struct ToolDelegateAttribution(Arc<DelegateOrigin>);

impl ToolDelegateAttribution {
    /// The binding of the governed call whose tool performs this delegate.
    pub fn parent_binding(&self) -> &PreparedAuthorizationBinding {
        &self.0.parent
    }

    /// The work the parent call was prepared in; the delegate is prepared
    /// only in it.
    pub fn work_authorization(&self) -> &WorkAuthorizationContext {
        &self.0.work
    }

    pub fn role(&self) -> ToolDelegateRole {
        self.0.role
    }

    /// The delegate of `parent`, a dispatch's own prepared call, for `role`.
    /// The call's plan must carry exactly the builtin operation the role
    /// requires (the target the builtin's registration owner states and
    /// revalidates, or that an in-process owner states for its own call),
    /// and the check must be prepared in `work`. Anything else is malformed
    /// facts.
    pub(crate) fn for_dispatch(
        parent: &PreparedOperationCheck,
        work: &WorkAuthorizationContext,
        role: ToolDelegateRole,
    ) -> Result<Self, OperationAuthorizationError> {
        if !parent.work_authorization().same_context(work) {
            return Err(malformed());
        }
        let (_, plan) = parent.tool_dispatch_parts().ok_or_else(malformed)?;
        let required = ToolOperationTarget::BuiltinOperation {
            operation: role.parent_operation(),
        };
        if !matches!(plan.single_operation_target(), Ok(Some(target)) if *target == required) {
            return Err(malformed());
        }
        Ok(Self(Arc::new(DelegateOrigin {
            parent: parent.binding().clone(),
            work: work.clone(),
            role,
        })))
    }

    /// Bind the delegate's own operation. `facts` must be its role's own
    /// kind of operation under the parent's work, execution scope, run and
    /// context revision, with an operation id of its own; anything else is
    /// malformed facts, before any owner prepares it.
    pub(crate) fn bind_child(
        &self,
        facts: OperationAuthorizationFacts,
        work: &WorkAuthorizationContext,
    ) -> Result<PreparedAuthorizationBinding, OperationAuthorizationError> {
        let parent = self.0.parent.facts();
        if !self.0.work.same_context(work)
            || facts.operation_id == parent.operation_id
            || facts.execution_scope != parent.execution_scope
            || &facts.execution_scope != work.execution_scope()
            || facts.run_id != parent.run_id
            || facts.context_revision != parent.context_revision
            || !self.0.role.admits(&facts.operation)
        {
            return Err(malformed());
        }
        Ok(PreparedAuthorizationBinding::new_delegate_child(
            facts,
            self.clone(),
        ))
    }
}

impl fmt::Debug for ToolDelegateAttribution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ToolDelegateAttribution([REDACTED])")
    }
}

fn malformed() -> OperationAuthorizationError {
    OperationRefused::new(OperationRefusalKind::MalformedFacts).into()
}

#[cfg(any(test, feature = "test-support"))]
mod test_support;
#[cfg(any(test, feature = "test-support"))]
pub use test_support::__test_mint_tool_delegate;

#[cfg(test)]
mod tests;
