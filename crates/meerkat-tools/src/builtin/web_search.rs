//! Builtin Meerkat-owned web-search fallback tool.

use crate::builtin::{BuiltinTool, BuiltinToolError, ToolOutput};
use async_trait::async_trait;
use meerkat_core::types::{ToolDef, ToolProvenance, ToolSourceKind};
use meerkat_core::web_search::{
    WEB_SEARCH_TOOL_NAME, WebSearchRequest, WebSearchResult, WebSearchStatus,
};
use meerkat_core::{Provider, WebSearchEvidence, WebSearchNativeEvent};
use meerkat_llm_core::WebSearchExecutor;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

const WEB_SEARCH_TOOL_DOCUMENTATION: &str = r#"Search the web through Meerkat when the active model does not have provider-native web search.

Use this tool when the user asks for current, recent, online, or cited information and no native provider web-search tool is available in this session.

Request shape:
{"query":"latest Meerkat release notes"}

Fields:
- query: required natural-language search/research query.
- provider: optional provider assertion: "openai", "gemini", or "anthropic". If omitted, Meerkat uses the session's configured fallback provider. If present, it must match that provider.
- context: optional brief context from the conversation to help disambiguate the query.

Result shape follows the common provider-native search idea: status, provider, model, answer, evidence, and native_events. Treat native_events as provider-observed evidence, not Meerkat-owned truth."#;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WebSearchToolArgs {
    #[schemars(description = "Natural-language query to search for.")]
    query: String,
    #[serde(default)]
    #[schemars(description = "Optional provider override: openai, gemini, or anthropic.")]
    provider: Option<String>,
    #[serde(default)]
    #[schemars(description = "Optional brief conversation context to disambiguate the search.")]
    context: Option<String>,
}

/// The Meerkat-owned `web_search` builtin: one public web search through its
/// own hosted-search helper, governed as two operations (the call by its
/// typed builtin target, the helper as the call's typed delegate).
///
/// It keeps the default [`BuiltinTool::review_entry_support`] (unsupported):
/// operation review of `web_search` is an unsupported slice, not review
/// coverage. A required R2 or R3 review settles as local feedback and the
/// search never runs; serving R2 or R3 needs the consuming review entry
/// first. Its read-only mutation class describes local state only: the
/// outbound query can carry conversation content, so neither that class nor
/// the typed target means private information cannot leave.
#[derive(Clone)]
pub struct WebSearchTool {
    executor: Arc<dyn WebSearchExecutor>,
}

impl WebSearchTool {
    pub fn new(executor: Arc<dyn WebSearchExecutor>) -> Self {
        Self { executor }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl BuiltinTool for WebSearchTool {
    fn name(&self) -> &'static str {
        WEB_SEARCH_TOOL_NAME
    }

    fn def(&self) -> ToolDef {
        ToolDef {
            audience: Default::default(),
            name: self.name().into(),
            description: WEB_SEARCH_TOOL_DOCUMENTATION.to_string(),
            input_schema: crate::schema::schema_for::<WebSearchToolArgs>(),
            provenance: Some(ToolProvenance {
                kind: ToolSourceKind::Builtin,
                source_id: "builtin".into(),
                artifact: None,
            }),
        }
    }

    fn default_enabled(&self) -> bool {
        false
    }

    /// Retrieval-only outbound query; it publishes nothing and writes no local state.
    fn mutation_class(&self) -> meerkat_core::ToolMutationClass {
        meerkat_core::ToolMutationClass::ReadOnly
    }

    /// Its registration owner states this as the call's typed operation
    /// target, so operation policy never classifies a search by its name.
    fn builtin_operation(&self) -> Option<meerkat_core::BuiltinToolOperation> {
        Some(meerkat_core::BuiltinToolOperation::WebSearch)
    }

    async fn call(&self, args: Value) -> Result<ToolOutput, BuiltinToolError> {
        self.search(args, None).await
    }

    /// A governed call prepares its helper's own model request as the typed
    /// hosted-search delegate of exactly this call: under the call's work
    /// and coordinates, decided by the helper's own owners for its own model,
    /// account, endpoint and hosted search. Neither the call's admission nor
    /// its tool name authorizes the helper. A governed context that cannot
    /// name this call's own prepared builtin operation refuses before any
    /// search. Without admitted work the call is unchanged.
    async fn call_with_context(
        &self,
        call: meerkat_core::ToolCallView<'_>,
        args: Value,
        context: &meerkat_core::ToolDispatchContext,
    ) -> Result<ToolOutput, BuiltinToolError> {
        let authorization = context
            .tool_delegate_attribution(
                call,
                meerkat_core::authorization::ToolDelegateRole::HostedSearchInference,
            )
            .map_err(|error| BuiltinToolError::EntryRefused(Box::new(error)))?
            .map(meerkat_core::LlmRequestAuthorization::for_tool_delegate);
        self.search(args, authorization).await
    }
}

impl WebSearchTool {
    async fn search(
        &self,
        args: Value,
        authorization: Option<meerkat_core::LlmRequestAuthorization>,
    ) -> Result<ToolOutput, BuiltinToolError> {
        let args: WebSearchToolArgs = serde_json::from_value(args)
            .map_err(|err| BuiltinToolError::invalid_args(err.to_string()))?;
        let query = args.query.trim().to_string();
        if query.is_empty() {
            return Err(BuiltinToolError::invalid_args("query must not be empty"));
        }
        let provider = args
            .provider
            .as_deref()
            .map(parse_search_provider)
            .transpose()?;
        let result = self
            .executor
            .execute_web_search_authorized(
                WebSearchRequest {
                    query,
                    provider,
                    provider_params: None,
                    context: args.context.filter(|value| !value.trim().is_empty()),
                },
                authorization,
            )
            .await
            .map_err(|err| match err {
                meerkat_llm_core::LlmError::OperationRefused { refusal } => {
                    BuiltinToolError::OperationRefused { refusal }
                }
                meerkat_llm_core::LlmError::OperationObservationUnavailable => {
                    BuiltinToolError::OperationObservationUnavailable
                }
                meerkat_llm_core::LlmError::OperationAuthorizationUnavailable => {
                    BuiltinToolError::OperationAuthorizationUnavailable
                }
                meerkat_llm_core::LlmError::OperationReviewRefused { refusal } => {
                    BuiltinToolError::EntryRefused(Box::new(refusal.into()))
                }
                other => BuiltinToolError::execution_failed(other.to_string()),
            })?;
        serde_json::to_value(result)
            .map(ToolOutput::Json)
            .map_err(|err| BuiltinToolError::execution_failed(err.to_string()))
    }
}

fn parse_search_provider(value: &str) -> Result<Provider, BuiltinToolError> {
    match Provider::parse_strict(value) {
        Some(provider @ (Provider::OpenAI | Provider::Gemini | Provider::Anthropic)) => {
            Ok(provider)
        }
        Some(other) => Err(BuiltinToolError::invalid_args(format!(
            "provider '{}' does not support Meerkat web_search fallback",
            other.as_str()
        ))),
        None => Err(BuiltinToolError::invalid_args(format!(
            "unknown provider '{value}' (expected openai, gemini, or anthropic)"
        ))),
    }
}

#[derive(Default)]
pub struct EmptyWebSearchExecutor;

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl WebSearchExecutor for EmptyWebSearchExecutor {
    async fn execute_web_search(
        &self,
        request: WebSearchRequest,
    ) -> Result<WebSearchResult, meerkat_llm_core::LlmError> {
        Ok(WebSearchResult {
            status: WebSearchStatus::Unavailable,
            query: request.query,
            provider: request.provider,
            model: None,
            answer: None,
            evidence: Vec::<WebSearchEvidence>::new(),
            native_events: Vec::<WebSearchNativeEvent>::new(),
            error: Some("no configured provider supports Meerkat web_search fallback".to_string()),
            checked_at: chrono::Utc::now(),
        })
    }

    /// Reports unavailability without any model request, so there is no
    /// helper operation for the companion to govern.
    async fn execute_web_search_authorized(
        &self,
        request: WebSearchRequest,
        _authorization: Option<meerkat_core::LlmRequestAuthorization>,
    ) -> Result<WebSearchResult, meerkat_llm_core::LlmError> {
        self.execute_web_search(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meerkat_core::lifecycle::run_primitive::ModelId;
    use meerkat_core::{WebSearchStatus, web_search::WebSearchResult};
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct RecordingExecutor {
        seen: Mutex<Vec<WebSearchRequest>>,
    }

    #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
    impl WebSearchExecutor for RecordingExecutor {
        async fn execute_web_search(
            &self,
            request: WebSearchRequest,
        ) -> Result<WebSearchResult, meerkat_llm_core::LlmError> {
            self.seen.lock().await.push(request.clone());
            Ok(WebSearchResult {
                status: WebSearchStatus::Completed,
                query: request.query,
                provider: Some(request.provider.unwrap_or(Provider::OpenAI)),
                model: Some(ModelId::new("gpt-5.5")),
                answer: Some("answer".to_string()),
                evidence: Vec::new(),
                native_events: Vec::new(),
                error: None,
                checked_at: chrono::Utc::now(),
            })
        }
    }

    #[tokio::test]
    async fn web_search_tool_accepts_provider_override() -> Result<(), String> {
        let executor = Arc::new(RecordingExecutor::default());
        let tool = WebSearchTool::new(executor.clone());
        let output = tool
            .call(serde_json::json!({
                "query": "today's news",
                "provider": "gemini",
                "context": "smoke test"
            }))
            .await
            .expect("tool call should succeed");
        let value = match output {
            ToolOutput::Json(value) => value,
            other => return Err(format!("expected JSON output, got {other:?}")),
        };
        assert_eq!(value["status"], "completed");
        assert_eq!(value["provider"], "gemini");
        let seen = executor.seen.lock().await;
        assert_eq!(seen[0].provider, Some(Provider::Gemini));
        assert_eq!(seen[0].provider_params, None);
        Ok(())
    }

    #[tokio::test]
    async fn web_search_tool_rejects_provider_native_params() {
        let tool = WebSearchTool::new(Arc::new(RecordingExecutor::default()));
        let err = tool
            .call(serde_json::json!({
                "query": "today's news",
                "provider_params": {"allowed_domains": ["example.com"]}
            }))
            .await
            .expect_err("model-callable fallback search must not accept provider-native params");
        assert!(err.to_string().contains("unknown field"));
    }

    #[tokio::test]
    async fn web_search_tool_rejects_non_search_provider() {
        let tool = WebSearchTool::new(Arc::new(RecordingExecutor::default()));
        let err = tool
            .call(serde_json::json!({"query": "x", "provider": "self_hosted"}))
            .await
            .expect_err("self-hosted cannot own fallback search");
        assert!(err.to_string().contains("does not support"));
    }

    struct UnusedPolicy;
    impl meerkat_core::authorization::WorkAuthorization for UnusedPolicy {
        fn prepare(
            &self,
            _: &meerkat_core::authorization::PreparedAuthorizationBinding,
        ) -> Result<
            Arc<dyn meerkat_core::authorization::PreparedOperationAuthorization>,
            meerkat_core::OperationAuthorizationError,
        > {
            Err(meerkat_core::authorization::OperationRefused::new(
                meerkat_core::authorization::OperationRefusalKind::Denied,
            )
            .into())
        }
    }

    fn governed_context() -> meerkat_core::ToolDispatchContext {
        meerkat_core::ToolDispatchContext::default().with_work_authorization(Some(
            meerkat_core::WorkAuthorizationContext::new(
                Arc::new(UnusedPolicy),
                meerkat_core::exact_operation::OperationExecutionScope::Domain,
            ),
        ))
    }

    async fn call_in(
        tool: &WebSearchTool,
        context: &meerkat_core::ToolDispatchContext,
    ) -> Result<ToolOutput, BuiltinToolError> {
        let args = serde_json::json!({"query": "fixed query"});
        let raw = serde_json::value::RawValue::from_string(args.to_string()).unwrap();
        tool.call_with_context(
            meerkat_core::ToolCallView {
                id: "search-call",
                name: WEB_SEARCH_TOOL_NAME,
                args: &raw,
            },
            args,
            context,
        )
        .await
    }

    /// Stands in for a helper whose own model operation is refused.
    #[derive(Default)]
    struct RefusedHelper {
        authorized: Mutex<Vec<bool>>,
    }

    #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
    impl WebSearchExecutor for RefusedHelper {
        async fn execute_web_search(
            &self,
            _request: WebSearchRequest,
        ) -> Result<WebSearchResult, meerkat_llm_core::LlmError> {
            Err(meerkat_llm_core::LlmError::InvalidRequest {
                message: "the tool always uses the authorized seam".to_string(),
            })
        }

        async fn execute_web_search_authorized(
            &self,
            _request: WebSearchRequest,
            authorization: Option<meerkat_core::LlmRequestAuthorization>,
        ) -> Result<WebSearchResult, meerkat_llm_core::LlmError> {
            self.authorized.lock().await.push(authorization.is_some());
            Err(meerkat_llm_core::LlmError::operation_refused(
                meerkat_core::authorization::OperationRefusalKind::Denied,
            ))
        }
    }

    struct Allowed;
    impl meerkat_core::authorization::PreparedOperationAuthorization for Allowed {
        fn review_tier(&self) -> meerkat_core::authorization::OperationReviewTier {
            meerkat_core::authorization::OperationReviewTier::R1
        }

        fn check_current(
            &self,
            _binding: &meerkat_core::authorization::PreparedAuthorizationBinding,
        ) -> Result<(), meerkat_core::OperationAuthorizationError> {
            Ok(())
        }
    }

    /// Allows every operation and records the bindings it prepares, by kind.
    #[derive(Default)]
    struct RecordingWork {
        tools: std::sync::Mutex<Vec<meerkat_core::authorization::PreparedAuthorizationBinding>>,
        models: std::sync::Mutex<Vec<meerkat_core::authorization::PreparedAuthorizationBinding>>,
    }

    impl meerkat_core::authorization::WorkAuthorization for RecordingWork {
        fn prepare(
            &self,
            binding: &meerkat_core::authorization::PreparedAuthorizationBinding,
        ) -> Result<
            Arc<dyn meerkat_core::authorization::PreparedOperationAuthorization>,
            meerkat_core::OperationAuthorizationError,
        > {
            let recorded = match &binding.facts().operation {
                meerkat_core::authorization::AuthorizationOperation::Model(_) => &self.models,
                _ => &self.tools,
            };
            recorded.lock().unwrap().push(binding.clone());
            Ok(Arc::new(Allowed))
        }
    }

    /// One real governed fenced dispatch of a web_search call through a
    /// composite whose registered web_search uses `executor`.
    async fn governed_dispatch(
        executor: Arc<dyn WebSearchExecutor>,
        work: meerkat_core::WorkAuthorizationContext,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        let mut composite = crate::builtin::CompositeDispatcher::new(
            Arc::new(crate::builtin::MemoryTaskStore::new()),
            &crate::builtin::BuiltinToolConfig::default(),
            Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
            None,
            None,
            None,
        )
        .unwrap();
        composite.register_web_search_tool(executor, meerkat_core::ToolCategoryOverride::Enable);
        let dispatcher = Arc::new(composite);
        let context =
            meerkat_core::ToolDispatchContext::default().with_work_authorization(Some(work));
        let args = serde_json::value::RawValue::from_string(
            serde_json::json!({"query": "fixed query"}).to_string(),
        )
        .unwrap();
        let call = meerkat_core::ToolCallView {
            id: "search-call",
            name: WEB_SEARCH_TOOL_NAME,
            args: &args,
        };
        let resolution = meerkat_core::ToolExecutionResolutionContext::new(
            meerkat_core::ToolDeadlineChain::new(vec![
                meerkat_core::ToolDeadlineContributor::finite(
                    meerkat_core::ToolDeadlineOwner::CoreToolDispatch,
                    std::time::Duration::from_secs(60),
                ),
            ])
            .unwrap(),
        );
        let plan = meerkat_core::agent::resolve_tool_execution_plan_fenced(
            &dispatcher,
            call,
            &context,
            &resolution,
        )
        .unwrap();
        meerkat_core::agent::dispatch_tool_execution_plan_fenced(&dispatcher, call, &context, &plan)
            .await
    }

    fn recording_work() -> (Arc<RecordingWork>, meerkat_core::WorkAuthorizationContext) {
        let recorder = Arc::new(RecordingWork::default());
        let work = meerkat_core::WorkAuthorizationContext::new(
            recorder.clone(),
            meerkat_core::exact_operation::OperationExecutionScope::Domain,
        );
        (recorder, work)
    }

    /// The helper's own route, as a provider adapter reports it.
    fn helper_facts() -> meerkat_core::authorization::ModelAuthorizationFacts {
        meerkat_core::authorization::ModelAuthorizationFacts {
            identity: Arc::new(meerkat_core::SessionLlmIdentity {
                model: "search-model".into(),
                provider: Provider::OpenAI,
                self_hosted_server_id: None,
                provider_params: None,
                auth_binding: None,
            }),
            wire_model: Arc::from("search-wire-model"),
            hosted_capabilities: Arc::from([meerkat_core::ServerToolKind::WebSearch]),
            backend_profile_id: Some(Arc::from("search-profile")),
            backend_kind: Arc::from("openai"),
            endpoint: Arc::from("https://search.invalid/responses"),
            credential: None,
            usage: meerkat_core::authorization::ModelAuthorizationUse::Inference,
            live_channel: None,
        }
    }

    /// Prepares its own helper request with the authorization it is handed,
    /// as a provider adapter does for its actual target, and keeps the check.
    #[derive(Default)]
    struct PreparingHelper {
        prepared: Mutex<Vec<meerkat_core::authorization::PreparedOperationCheck>>,
    }

    #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
    impl WebSearchExecutor for PreparingHelper {
        async fn execute_web_search(
            &self,
            _request: WebSearchRequest,
        ) -> Result<WebSearchResult, meerkat_llm_core::LlmError> {
            Err(meerkat_llm_core::LlmError::InvalidRequest {
                message: "a governed search binds its helper".to_string(),
            })
        }

        async fn execute_web_search_authorized(
            &self,
            request: WebSearchRequest,
            authorization: Option<meerkat_core::LlmRequestAuthorization>,
        ) -> Result<WebSearchResult, meerkat_llm_core::LlmError> {
            let Some(authorization) = authorization else {
                return self.execute_web_search(request).await;
            };
            let check = authorization
                .prepare(helper_facts())
                .map_err(meerkat_llm_core::LlmError::from_operation_authorization)?;
            self.prepared.lock().await.push(check);
            Ok(WebSearchResult {
                status: WebSearchStatus::Completed,
                query: request.query,
                provider: Some(Provider::OpenAI),
                model: Some(ModelId::new("search-model")),
                answer: Some("answer".to_string()),
                evidence: Vec::new(),
                native_events: Vec::new(),
                error: None,
                checked_at: chrono::Utc::now(),
            })
        }
    }

    /// A governed web_search prepares its helper request as the typed
    /// hosted-search delegate of exactly the call its dispatch prepared:
    /// under the same work, as an inference operation with a fresh id and
    /// the parent's coordinates, carrying the parent's binding.
    #[tokio::test]
    async fn governed_search_prepares_its_helper_as_a_typed_delegate_of_its_own_call() {
        let (recorder, work) = recording_work();
        let helper = Arc::new(PreparingHelper::default());
        governed_dispatch(helper.clone(), work.clone())
            .await
            .expect("the governed search completes");
        let prepared = helper.prepared.lock().await;
        assert_eq!(prepared.len(), 1, "one helper request prepared");
        let binding = prepared[0].binding();
        let tools = recorder.tools.lock().unwrap();
        assert_eq!(tools.len(), 1, "the dispatch prepared its own call once");
        let parent = &tools[0];
        let link = binding
            .delegate_attribution()
            .expect("the helper is a typed delegate of its call");
        assert!(link.parent_binding().same_operation(parent));
        assert!(link.work_authorization().same_context(&work));
        assert_eq!(
            link.role(),
            meerkat_core::authorization::ToolDelegateRole::HostedSearchInference
        );
        assert!(matches!(
            &binding.facts().operation,
            meerkat_core::authorization::AuthorizationOperation::Model(model)
                if model.usage == meerkat_core::authorization::ModelAuthorizationUse::Inference
                    && model.hosted_capabilities.as_ref()
                        == [meerkat_core::ServerToolKind::WebSearch]
        ));
        assert_ne!(binding.facts().operation_id, parent.facts().operation_id);
        assert_eq!(binding.facts().run_id, parent.facts().run_id);
        let models = recorder.models.lock().unwrap();
        assert_eq!(models.len(), 1);
        assert!(models[0].same_operation(binding));
    }

    #[tokio::test]
    async fn governed_turn_forwards_work_authorization_and_keeps_refusal_typed() {
        let executor = Arc::new(RefusedHelper::default());
        let (_recorder, work) = recording_work();
        let error = governed_dispatch(executor.clone(), work)
            .await
            .expect_err("a refused helper is not a search result");
        assert_eq!(
            error,
            meerkat_core::ToolError::AuthorizationRefused {
                refusal: meerkat_core::authorization::OperationRefused::new(
                    meerkat_core::authorization::OperationRefusalKind::Denied,
                ),
            },
            "refusal must not flatten into ExecutionFailed"
        );
        assert_eq!(*executor.authorized.lock().await, vec![true]);
    }

    #[tokio::test]
    async fn executor_without_an_authorized_seam_refuses_governed_work_without_searching() {
        let executor = Arc::new(RecordingExecutor::default());
        let (_recorder, work) = recording_work();
        let error = governed_dispatch(executor.clone(), work)
            .await
            .expect_err("an executor that cannot bind the helper must refuse");
        assert!(
            matches!(error, meerkat_core::ToolError::AuthorizationRefused { .. }),
            "{error:?}"
        );
        assert!(executor.seen.lock().await.is_empty(), "zero searches");

        // Without admitted work the legacy contract is unchanged.
        call_in(
            &WebSearchTool::new(executor.clone()),
            &meerkat_core::ToolDispatchContext::default(),
        )
        .await
        .expect("ungoverned search keeps its existing behavior");
        assert_eq!(executor.seen.lock().await.len(), 1);
    }

    /// A governed context whose dispatch never prepared this call (a work
    /// association without its prepared check) refuses before any search,
    /// whatever executor is installed: no helper request is bound or sent.
    /// Ungoverned search keeps its existing contract.
    #[tokio::test]
    async fn a_governed_context_without_its_prepared_call_refuses_before_searching() {
        let recording = Arc::new(RecordingExecutor::default());
        let refusing = Arc::new(RefusedHelper::default());
        for tool in [
            WebSearchTool::new(recording.clone()),
            WebSearchTool::new(refusing.clone()),
        ] {
            let error = call_in(&tool, &governed_context())
                .await
                .expect_err("an unprepared governed call refuses");
            assert!(
                matches!(
                    &error,
                    BuiltinToolError::EntryRefused(entry)
                        if matches!(
                            entry.as_ref(),
                            meerkat_core::ToolError::AuthorizationRefused { refusal }
                                if refusal.kind()
                                    == meerkat_core::authorization::OperationRefusalKind::MalformedFacts
                        )
                ),
                "refused before the executor as malformed facts: {error:?}"
            );
        }
        assert!(recording.seen.lock().await.is_empty(), "zero searches");
        assert!(
            refusing.authorized.lock().await.is_empty(),
            "the helper was never asked to bind a request"
        );
        call_in(
            &WebSearchTool::new(recording.clone()),
            &meerkat_core::ToolDispatchContext::default(),
        )
        .await
        .expect("ungoverned search keeps its existing behavior");
        assert_eq!(recording.seen.lock().await.len(), 1);
    }
}
