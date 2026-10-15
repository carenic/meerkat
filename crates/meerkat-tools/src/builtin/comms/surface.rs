//! Comms tool surface - provides comms tools as a ToolDispatcher

use async_trait::async_trait;
use meerkat_comms::{
    CommsExecutionAuthority, Router, RuntimeCommsCommandHandle, ToolContext, TrustedPeersView,
    comms_tool_defs, comms_tool_unavailable_reason, handle_tools_call_with_context,
};
use meerkat_core::AgentToolDispatcher;
use meerkat_core::ToolCallArguments;
use meerkat_core::ToolDispatchContext;
use meerkat_core::error::ToolError;
use meerkat_core::types::{ToolCallView, ToolDef, ToolResult};
use meerkat_core::{ToolCallability, ToolCatalogCapabilities, ToolCatalogEntry};
#[cfg(test)]
use serde_json::Value;
use std::sync::Arc;

/// Tool dispatcher that provides comms tools.
///
/// Its comms owner ([`CommsExecutionAuthority`]) attaches each call's typed
/// operation target to the plan it resolves (the one peer a delivery
/// reaches, or the trusted-peer directory) and runs only a plan it resolved
/// for exactly that target.
pub struct CommsToolSurface {
    tool_context: ToolContext,
    tool_defs: Arc<[Arc<ToolDef>]>,
    execution_authority: CommsExecutionAuthority,
}

impl CommsToolSurface {
    /// Create a new comms tool surface
    pub fn new(router: Arc<Router>, trusted_peers: TrustedPeersView) -> Self {
        let tool_context = ToolContext {
            router,
            trusted_peers,
            runtime: None,
        };
        let tool_defs: Arc<[Arc<ToolDef>]> = comms_tool_defs().into();
        Self {
            tool_context,
            tool_defs,
            execution_authority: CommsExecutionAuthority::new(),
        }
    }

    pub fn new_with_runtime(
        router: Arc<Router>,
        trusted_peers: TrustedPeersView,
        runtime: Arc<dyn meerkat_core::agent::CommsRuntime>,
    ) -> Self {
        let tool_context = ToolContext {
            router,
            trusted_peers,
            runtime: Some(RuntimeCommsCommandHandle::new(runtime)),
        };
        let tool_defs: Arc<[Arc<ToolDef>]> = comms_tool_defs().into();
        Self {
            tool_context,
            tool_defs,
            execution_authority: CommsExecutionAuthority::new(),
        }
    }

    pub fn new_with_runtime_and_post_commit_hooks(
        router: Arc<Router>,
        trusted_peers: TrustedPeersView,
        runtime: Arc<dyn meerkat_core::agent::CommsRuntime>,
        post_commit_hooks: Arc<meerkat_core::PostCommitHookDispatcher>,
    ) -> Self {
        let tool_context = ToolContext {
            router,
            trusted_peers,
            runtime: Some(
                RuntimeCommsCommandHandle::new(runtime).with_post_commit_hooks(post_commit_hooks),
            ),
        };
        let tool_defs: Arc<[Arc<ToolDef>]> = comms_tool_defs().into();
        Self {
            tool_context,
            tool_defs,
            execution_authority: CommsExecutionAuthority::new(),
        }
    }

    fn callability_for_tool(&self, name: &str) -> ToolCallability {
        comms_tool_unavailable_reason(&self.tool_context, name)
            .map_or_else(ToolCallability::callable, ToolCallability::unavailable)
    }

    /// Usage instructions for comms tools to be added to the system prompt
    pub fn usage_instructions() -> &'static str {
        r#"# Inter-agent Communication

You can communicate with other agents using these tools:

- **send_message**: Send a message to a peer. Always include `handling_mode` - use `"queue"` for normal collaboration and `"steer"` only for urgent preemption.
- **reply_to_peer**: Reply to the peer whose message triggered the current turn. Pre-addressed - no peer_id needed. Always include `handling_mode`. If several peer messages arrived this turn, set `reply_to` to one of the delivery ids listed by the ambiguity error.
- **send_request**: Send a structured request (intent + params) and expect a correlated response. Always include `handling_mode`.
- **send_response**: Reply to a previous request using its ID.
- **peers**: List all visible peers. Always check peers first to see who is available.

Use `reply_to_peer` to answer an incoming peer message, `send_message` for ordinary outreach. Use `send_request` only when you need structured intent/params with a correlated reply."#
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl AgentToolDispatcher for CommsToolSurface {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        self.tool_catalog()
            .iter()
            .filter(|entry| entry.currently_callable())
            .map(|entry| Arc::clone(&entry.tool))
            .collect::<Vec<_>>()
            .into()
    }

    /// `peers` is a roster read; every other comms tool delivers a message to
    /// another agent, which is a mutation of state outside this session.
    fn tool_mutation_class(&self, tool_name: &str) -> meerkat_core::ToolMutationClass {
        if !self.tool_defs.iter().any(|tool| tool.name == tool_name) {
            return meerkat_core::ToolMutationClass::Unknown;
        }
        match tool_name {
            "peers" => meerkat_core::ToolMutationClass::ReadOnly,
            _ => meerkat_core::ToolMutationClass::Mutating,
        }
    }

    fn live_bridge_effect_kind(&self, tool_name: &str) -> meerkat_core::LiveBridgeEffectKind {
        if self.tool_defs.iter().any(|tool| tool.name == tool_name) {
            meerkat_core::LiveBridgeEffectKind::Comms
        } else {
            meerkat_core::LiveBridgeEffectKind::ExternalIo
        }
    }

    fn tool_catalog_capabilities(&self) -> ToolCatalogCapabilities {
        ToolCatalogCapabilities {
            exact_catalog: true,
            may_require_catalog_control_plane: false,
        }
    }

    fn tool_catalog(&self) -> Arc<[ToolCatalogEntry]> {
        self.tool_defs
            .iter()
            .map(|tool| {
                ToolCatalogEntry::session_inline_with_callability(
                    Arc::clone(tool),
                    self.callability_for_tool(&tool.name),
                )
            })
            .collect::<Vec<_>>()
            .into()
    }

    /// Every comms plan carries the call's typed operation target from the
    /// comms owner: the one peer a delivery reaches, or the trusted-peer
    /// directory.
    fn resolve_execution_plan(
        &self,
        call: ToolCallView<'_>,
        dispatch_context: &ToolDispatchContext,
        resolution_context: &meerkat_core::ToolExecutionResolutionContext,
    ) -> Result<meerkat_core::ResolvedToolExecutionPlan, meerkat_core::ToolExecutionResolutionError>
    {
        if !self.tool_defs.iter().any(|tool| tool.name == call.name) {
            return Err(meerkat_core::ToolExecutionResolutionError::NotFound {
                tool_name: call.name.to_string(),
            });
        }
        self.execution_authority.resolve_plan(
            &self.tool_context,
            call,
            self.execution_binding_fingerprint(call.name)?,
            dispatch_context,
            resolution_context,
        )
    }

    /// A resolved plan runs only when the comms owner resolved it for
    /// exactly this call's current target and binding.
    async fn dispatch_resolved_with_context(
        &self,
        call: ToolCallView<'_>,
        context: &ToolDispatchContext,
        plan: &meerkat_core::ResolvedToolExecutionPlan,
    ) -> Result<meerkat_core::ops::ToolDispatchOutcome, ToolError> {
        if !self.tool_defs.iter().any(|tool| tool.name == call.name) {
            return Err(ToolError::NotFound {
                name: call.name.to_string(),
            });
        }
        if plan.mode() != meerkat_core::ToolExecutionMode::Fast {
            return Err(ToolError::unavailable(
                call.name,
                meerkat_core::ToolUnavailableReason::ExecutionModeOwnerUnavailable,
            ));
        }
        let binding = self
            .execution_binding_fingerprint(call.name)
            .map_err(ToolError::from)?;
        if !self
            .execution_authority
            .resolved(call, &binding, context, plan)
        {
            return Err(ToolError::unavailable(
                call.name,
                meerkat_core::ToolUnavailableReason::ExecutionOwnerChanged,
            ));
        }
        self.dispatch_with_context(call, context).await
    }

    async fn dispatch(
        &self,
        call: ToolCallView<'_>,
    ) -> Result<meerkat_core::ops::ToolDispatchOutcome, ToolError> {
        self.dispatch_with_context(call, &ToolDispatchContext::default())
            .await
    }

    async fn dispatch_with_context(
        &self,
        call: ToolCallView<'_>,
        context: &ToolDispatchContext,
    ) -> Result<meerkat_core::ops::ToolDispatchOutcome, ToolError> {
        let is_comms = self.tool_defs.iter().any(|t| t.name == call.name);
        if !is_comms {
            return Err(ToolError::NotFound {
                name: call.name.to_string(),
            });
        }
        if let Some(reason) = self.callability_for_tool(call.name).unavailable_reason() {
            return Err(ToolError::unavailable(call.name, reason));
        }

        let args = ToolCallArguments::from_raw_json(call.args)
            .map_err(|error| ToolError::invalid_arguments(call.name, error.to_string()))?;
        let result =
            handle_tools_call_with_context(&self.tool_context, call.name, args.as_value(), context)
                .await
                .map_err(|e| ToolError::ExecutionFailed { message: e })?;
        Ok(ToolResult::new(call.id.to_string(), result.to_string(), false).into())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use meerkat_comms::{CommsConfig, Keypair};
    use meerkat_core::ToolUnavailableReason;

    fn make_keypair() -> Keypair {
        Keypair::generate()
    }

    #[test]
    fn comms_tool_names_match_the_tool_definitions() {
        let defined: Vec<String> = comms_tool_defs()
            .iter()
            .map(|tool| tool.name.to_string())
            .collect();
        assert_eq!(defined, crate::builtin::COMMS_TOOL_NAMES);
    }

    fn make_tool_context() -> (Arc<Router>, TrustedPeersView) {
        let keypair = make_keypair();
        let (_, inbox_sender) = meerkat_comms::Inbox::new_transport_only();
        let router = Arc::new(Router::new(
            keypair,
            CommsConfig::default(),
            inbox_sender,
            true,
        ));
        let trusted_peers = router.trusted_peers_view();
        (router, trusted_peers)
    }

    #[test]
    fn comms_surface_reports_exact_catalog_support() {
        let (router, trusted_peers) = make_tool_context();
        let surface = CommsToolSurface::new(router, trusted_peers);
        assert!(surface.tool_catalog_capabilities().exact_catalog);
        let catalog = surface.tool_catalog();
        assert_eq!(catalog.len(), surface.tool_defs.len());
        assert!(catalog.iter().any(|entry| entry.tool.name == "send_request"
            && entry.callability.unavailable_reason()
                == Some(ToolUnavailableReason::RuntimeCommandAuthorityUnavailable)));
    }

    #[test]
    fn test_comms_tool_surface_trait() {
        let (router, trusted_peers) = make_tool_context();
        let surface = CommsToolSurface::new(router, trusted_peers);
        let tools = surface.tools();
        assert!(!tools.is_empty());
    }

    #[tokio::test]
    async fn test_comms_tool_surface_dispatch_unknown() {
        let (router, trusted_peers) = make_tool_context();
        let surface = CommsToolSurface::new(router, trusted_peers);
        let args_raw = serde_json::value::RawValue::from_string(Value::Null.to_string()).unwrap();
        let call = ToolCallView {
            id: "test-1",
            name: "unknown",
            args: &args_raw,
        };
        let result = surface.dispatch(call).await;
        assert!(matches!(result, Err(ToolError::NotFound { .. })));
    }

    #[test]
    fn test_comms_callability_true_with_trusted_peers() {
        let (_, inbox_sender) = meerkat_comms::Inbox::new_transport_only();
        let router = Arc::new(Router::new(
            make_keypair(),
            CommsConfig::default(),
            inbox_sender,
            true,
        ));
        let trusted_peers = router.trusted_peers_view();
        let surface = CommsToolSurface::new(router, trusted_peers);
        let tools = surface.tools();
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        assert!(tool_names.contains(&"send_message"));
        assert!(tool_names.contains(&"peers"));
        assert!(!tool_names.contains(&"send_request"));
        assert!(!tool_names.contains(&"send_response"));
    }

    #[tokio::test]
    async fn test_comms_tools_remain_visible_without_trusted_peers() {
        let (_, inbox_sender) = meerkat_comms::Inbox::new_transport_only();
        let router = Arc::new(Router::new(
            make_keypair(),
            CommsConfig::default(),
            inbox_sender,
            true,
        ));
        let trusted_peers = router.trusted_peers_view();
        let surface = CommsToolSurface::new(router, trusted_peers);
        let tools = surface.tools();
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        assert!(
            tool_names.contains(&"send_message"),
            "send_message must be advertised before peers appear so live providers can call it after later wiring: {tool_names:?}"
        );
        assert!(
            tool_names.contains(&"peers"),
            "peers must be advertised before peers appear so live providers can discover later wiring: {tool_names:?}"
        );
        assert!(!tool_names.contains(&"send_request"));
        assert!(!tool_names.contains(&"send_response"));

        let args_raw = serde_json::value::RawValue::from_string(Value::Null.to_string()).unwrap();
        let call = ToolCallView {
            id: "test-1",
            name: "send_request",
            args: &args_raw,
        };
        let result = surface.dispatch(call).await;
        assert!(matches!(result, Err(ToolError::Unavailable { .. })));
    }

    #[tokio::test]
    async fn send_request_without_runtime_returns_typed_unavailable_reason() {
        let (router, trusted_peers) = make_tool_context();
        let surface = CommsToolSurface::new(router, trusted_peers);
        let args_raw = serde_json::value::RawValue::from_string(
            serde_json::json!({
                "peer_id": meerkat_core::comms::PeerId::new(),
                "intent": "review",
                "params": {"file": "main.rs"},
                "handling_mode": "steer"
            })
            .to_string(),
        )
        .unwrap();
        let call = ToolCallView {
            id: "test-1",
            name: "send_request",
            args: &args_raw,
        };

        let result = surface.dispatch(call).await;
        assert!(matches!(
            result,
            Err(ToolError::Unavailable {
                reason: ToolUnavailableReason::RuntimeCommandAuthorityUnavailable,
                ..
            })
        ));
    }

    fn resolution() -> meerkat_core::ToolExecutionResolutionContext {
        meerkat_core::ToolExecutionResolutionContext::new(
            meerkat_core::ToolDeadlineChain::new(vec![
                meerkat_core::ToolDeadlineContributor::finite(
                    meerkat_core::ToolDeadlineOwner::CoreToolDispatch,
                    std::time::Duration::from_secs(600),
                ),
            ])
            .expect("deadline chain"),
        )
    }

    fn raw_args(value: Value) -> Box<serde_json::value::RawValue> {
        serde_json::value::RawValue::from_string(value.to_string()).expect("raw arguments")
    }

    fn send_args(peer: meerkat_core::comms::PeerId) -> Box<serde_json::value::RawValue> {
        raw_args(serde_json::json!({
            "peer_id": peer,
            "body": "hello",
            "handling_mode": "queue",
        }))
    }

    /// The operation targets a resolved plan carries, as their owners
    /// render them.
    fn plan_targets(plan: &meerkat_core::ResolvedToolExecutionPlan) -> Vec<String> {
        plan.owner_witnesses()
            .iter()
            .filter_map(|witness| witness.operation_target())
            .map(|target| format!("{target:?}"))
            .collect()
    }

    /// A delivery's plan names the peer the call is addressed to, and the
    /// roster read's plan names the trusted-peer directory: typed facts an
    /// operation policy owner can map, never the tool name.
    #[test]
    fn comms_plans_carry_their_typed_operation_target() {
        let (router, trusted_peers) = make_tool_context();
        let surface = CommsToolSurface::new(router, trusted_peers);
        let peer = meerkat_core::comms::PeerId::new();
        let args = send_args(peer);
        let plan = surface
            .resolve_execution_plan(
                ToolCallView {
                    id: "send-1",
                    name: "send_message",
                    args: &args,
                },
                &ToolDispatchContext::default(),
                &resolution(),
            )
            .expect("send_message plan");
        let targets = plan_targets(&plan);
        assert_eq!(
            targets.len(),
            1,
            "a delivery plan names exactly one operation target: {targets:?}"
        );
        assert!(
            targets[0].contains(&peer.to_string()),
            "the delivery target names the addressed peer: {targets:?}"
        );

        let empty = raw_args(serde_json::json!({}));
        let plan = surface
            .resolve_execution_plan(
                ToolCallView {
                    id: "peers-1",
                    name: "peers",
                    args: &empty,
                },
                &ToolDispatchContext::default(),
                &resolution(),
            )
            .expect("peers plan");
        assert_eq!(
            plan_targets(&plan).len(),
            1,
            "the roster read names its operation target"
        );
    }

    /// The comms owner revalidates its own target at dispatch: a plan
    /// resolved for one peer never delivers to another.
    #[tokio::test]
    async fn a_plan_resolved_for_one_peer_never_dispatches_to_another() {
        let (router, trusted_peers) = make_tool_context();
        let surface = CommsToolSurface::new(router, trusted_peers);
        let resolved_for = send_args(meerkat_core::comms::PeerId::new());
        let plan = surface
            .resolve_execution_plan(
                ToolCallView {
                    id: "send-1",
                    name: "send_message",
                    args: &resolved_for,
                },
                &ToolDispatchContext::default(),
                &resolution(),
            )
            .expect("send_message plan");
        let other = send_args(meerkat_core::comms::PeerId::new());
        let result = surface
            .dispatch_resolved_with_context(
                ToolCallView {
                    id: "send-1",
                    name: "send_message",
                    args: &other,
                },
                &ToolDispatchContext::default(),
                &plan,
            )
            .await;
        assert!(
            matches!(
                result,
                Err(ToolError::Unavailable {
                    reason: ToolUnavailableReason::ExecutionOwnerChanged,
                    ..
                })
            ),
            "a plan for another peer must not dispatch: {result:?}"
        );

        // The call the plan was resolved for passes the owner's check; the
        // handler's own trust check then refuses the untrusted peer.
        let result = surface
            .dispatch_resolved_with_context(
                ToolCallView {
                    id: "send-1",
                    name: "send_message",
                    args: &resolved_for,
                },
                &ToolDispatchContext::default(),
                &plan,
            )
            .await;
        assert!(
            matches!(
                &result,
                Err(ToolError::ExecutionFailed { message })
                    if message.starts_with("peer_not_found_or_not_trusted")
            ),
            "the resolved call reaches the comms handler: {result:?}"
        );
    }

    #[tokio::test]
    async fn non_object_args_are_rejected_as_invalid_arguments() {
        let (router, trusted_peers) = make_tool_context();
        let surface = CommsToolSurface::new(router, trusted_peers);
        // `peers` is callable without runtime authority; non-object args must
        // be rejected at the boundary instead of being coerced to a string Value
        // and forwarded into handle_tools_call_with_context.
        for body in ["\"a string\"", "[1, 2, 3]", "42"] {
            let args_raw = serde_json::value::RawValue::from_string(body.to_string()).unwrap();
            let call = ToolCallView {
                id: "test-1",
                name: "peers",
                args: &args_raw,
            };
            let result = surface.dispatch(call).await;
            assert!(
                matches!(result, Err(ToolError::InvalidArguments { .. })),
                "non-object arg {body:?} must be rejected as InvalidArguments, got {result:?}"
            );
        }
    }
}
