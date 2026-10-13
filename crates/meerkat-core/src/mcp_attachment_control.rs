//! One exact, authenticated MCP attachment control, prepared by the installed
//! native owners (see [`crate::mcp_attachment`]).
//!
//! The control carries the exact effect-bearing server descriptor, the facts a
//! trusted control ingress resolved, its process evidence and the bound
//! session. It is not a permission: the installed work-mandate and
//! operation-policy owners resolve the evidence for each effect under one
//! current grant publication. There is no serde or wire admission.

use std::any::Any;
use std::sync::Arc;

use crate::authorization::{
    AuthorizationOperation, OperationAuthorizationError, OperationAuthorizationFacts,
    OwnerQualifiedTarget, PreparedAuthorizationBinding, PublicationAuthorizationFacts,
    PublicationMode, PublicationRecipient, SourceAuthorizationFacts, SourceAuthorizationTarget,
    SourceAuthorizationUse,
};
use crate::exact_operation::OperationExecutionScope;
use crate::mcp_config::{McpScope, McpServerConfig};
use crate::{OperationId, PrincipalRef, SessionId};

/// Facts resolved by a trusted control ingress, never inferred from a server
/// name, a session id or a saved configuration row.
#[derive(Clone)]
pub struct McpAttachmentFacts {
    pub requester: PrincipalRef,
    pub actor: PrincipalRef,
    pub realm: crate::connection::RealmId,
    /// The bound session destination the server's tools would serve.
    pub destination: OwnerQualifiedTarget,
}

/// The one effect a prepared check covers. A live attachment never authorizes
/// a configuration write, and a write in one scope never covers another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpAttachmentEffect {
    /// Attach the server to the bound session: its tools are hydrated from
    /// the server and published to the session destination.
    Apply,
    /// Write the descriptor to configuration in this scope.
    Persist(McpScope),
}

/// Exact immutable attachment control submitted to the installed native
/// owners. The process evidence is a real host/mandate receipt whose concrete
/// type and current validity those owners must verify.
pub struct McpAttachmentControlRequest {
    operation_id: OperationId,
    session_id: SessionId,
    server: McpServerConfig,
    facts: McpAttachmentFacts,
    evidence: Arc<dyn Any + Send + Sync>,
}

impl McpAttachmentControlRequest {
    pub fn from_trusted_ingress<T: Any + Send + Sync>(
        session_id: SessionId,
        server: McpServerConfig,
        facts: McpAttachmentFacts,
        evidence: Arc<T>,
    ) -> Result<Arc<Self>, OperationAuthorizationError> {
        facts
            .requester
            .validate_qualified()
            .map_err(|_| malformed())?;
        facts.actor.validate_qualified().map_err(|_| malformed())?;
        facts
            .destination
            .authority
            .validate_qualified()
            .map_err(|_| malformed())?;
        if server.name.is_empty() {
            return Err(malformed());
        }
        Ok(Arc::new(Self {
            operation_id: OperationId::new(),
            session_id,
            server,
            facts,
            evidence,
        }))
    }

    pub fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }
    pub fn server(&self) -> &McpServerConfig {
        &self.server
    }
    pub fn facts(&self) -> &McpAttachmentFacts {
        &self.facts
    }
    pub fn evidence<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.evidence.downcast_ref()
    }

    /// The server the descriptor names, under the destination's authority.
    pub fn server_target(&self) -> OwnerQualifiedTarget {
        OwnerQualifiedTarget {
            authority: self.facts.destination.authority.clone(),
            namespace: Arc::from("mcp-server"),
            id: Arc::from(self.server.name.as_str()),
        }
    }

    /// The configuration entry a persisted descriptor occupies in `scope`.
    pub fn configuration_target(&self, scope: McpScope) -> OwnerQualifiedTarget {
        OwnerQualifiedTarget {
            authority: self.facts.destination.authority.clone(),
            namespace: Arc::from(match scope {
                McpScope::User => "mcp-config:user",
                McpScope::Project => "mcp-config:project",
            }),
            id: Arc::from(self.server.name.as_str()),
        }
    }

    /// The exact operations `effect` prepares: every one must be allowed
    /// under the same observation. Apply hydrates the server and publishes
    /// its tools to the destination; Persist retains the descriptor in that
    /// scope's configuration. The last operation bears the effect.
    pub fn effect_operations(&self, effect: McpAttachmentEffect) -> Vec<AuthorizationOperation> {
        match effect {
            McpAttachmentEffect::Apply => vec![
                AuthorizationOperation::Source(SourceAuthorizationFacts {
                    target: SourceAuthorizationTarget::External(self.server_target()),
                    usage: SourceAuthorizationUse::Hydrate,
                }),
                self.effect_operation(effect),
            ],
            McpAttachmentEffect::Persist(_) => vec![self.effect_operation(effect)],
        }
    }

    fn effect_operation(&self, effect: McpAttachmentEffect) -> AuthorizationOperation {
        match effect {
            McpAttachmentEffect::Apply => {
                AuthorizationOperation::Publication(PublicationAuthorizationFacts {
                    recipient: PublicationRecipient::Destination(self.facts.destination.clone()),
                    scope: None,
                    live_channel: None,
                    mode: PublicationMode::Buffered,
                })
            }
            McpAttachmentEffect::Persist(scope) => {
                AuthorizationOperation::Source(SourceAuthorizationFacts {
                    target: SourceAuthorizationTarget::External(self.configuration_target(scope)),
                    usage: SourceAuthorizationUse::Retain,
                })
            }
        }
    }

    /// The binding a prepared check for `effect` is entered with.
    pub fn effect_binding(&self, effect: McpAttachmentEffect) -> PreparedAuthorizationBinding {
        self.binding_for(self.effect_operation(effect))
    }

    pub fn binding_for(&self, operation: AuthorizationOperation) -> PreparedAuthorizationBinding {
        PreparedAuthorizationBinding::new(OperationAuthorizationFacts {
            operation_id: self.operation_id.clone(),
            execution_scope: OperationExecutionScope::Domain,
            run_id: None,
            context_revision: None,
            operation,
        })
    }

    /// Which effect `binding` is, if it is exactly one of this control's
    /// effect bindings; anything else is not covered by this control.
    pub fn effect_of(&self, binding: &PreparedAuthorizationBinding) -> Option<McpAttachmentEffect> {
        let facts = binding.facts();
        if facts.operation_id != self.operation_id
            || facts.execution_scope != OperationExecutionScope::Domain
            || facts.run_id.is_some()
            || facts.context_revision.is_some()
        {
            return None;
        }
        match &facts.operation {
            AuthorizationOperation::Publication(publication)
                if publication.scope.is_none()
                    && publication.live_channel.is_none()
                    && publication.mode == PublicationMode::Buffered
                    && matches!(&publication.recipient,
                        PublicationRecipient::Destination(destination)
                            if same_target(destination, &self.facts.destination)) =>
            {
                Some(McpAttachmentEffect::Apply)
            }
            AuthorizationOperation::Source(SourceAuthorizationFacts {
                target: SourceAuthorizationTarget::External(target),
                usage: SourceAuthorizationUse::Retain,
            }) => [McpScope::User, McpScope::Project]
                .into_iter()
                .find(|scope| same_target(target, &self.configuration_target(*scope)))
                .map(McpAttachmentEffect::Persist),
            _ => None,
        }
    }
}

fn same_target(left: &OwnerQualifiedTarget, right: &OwnerQualifiedTarget) -> bool {
    left.authority == right.authority && left.namespace == right.namespace && left.id == right.id
}

fn malformed() -> OperationAuthorizationError {
    crate::OperationRefused::new(crate::OperationRefusalKind::MalformedFacts).into()
}
