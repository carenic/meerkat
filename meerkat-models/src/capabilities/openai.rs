//! Per-model capability rows for OpenAI models.
//!
//! Seed values match the current `profile()` output so the parity test passes
//! before the switchover. Authoritative context-window and max-output
//! corrections (gpt-5.4 → 1,050,000 / 128,000; gpt-5.3-codex → 400,000 /
//! 128,000) land in a later commit.

use super::{ModelCapabilities, ThinkingSupport};
use crate::catalog::ModelTier;

/// OpenAI reasoning effort levels advertised by the GPT-5 bucket today.
///
/// `xhigh` / `minimal` are authoritative additions (per `developers.openai.com`)
/// that will land with the correctness-fix commit.
const GPT5_REASONING_EFFORT: &[&str] = &["low", "medium", "high"];

/// Capability rows for OpenAI catalog models.
pub const CAPABILITIES: &[ModelCapabilities] = &[
    ModelCapabilities {
        id: "gpt-5.4",
        provider: "openai",
        display_name: "GPT-5.4",
        tier: ModelTier::Recommended,
        model_family: "gpt-5",
        context_window: 128_000,
        max_output_tokens: 16_384,
        context_window_beta: None,
        max_output_tokens_beta: None,
        vision: true,
        image_tool_results: false,
        inline_video: false,
        // GPT-5 family rejects non-default temperature/top_p/top_k when
        // reasoning is active. Current code models temperature as unsupported
        // across the family; keep that seed shape for parity.
        supports_temperature: false,
        supports_top_p: false,
        supports_top_k: false,
        thinking: ThinkingSupport::None,
        supports_reasoning: true,
        effort_levels: GPT5_REASONING_EFFORT,
        supports_web_search: true,
        supports_inference_geo: false,
        supports_compaction: false,
        supports_structured_output: true,
        supports_legacy_penalties: true,
        supports_thinking_budget_legacy: false,
        beta_headers: &[],
        call_timeout_secs: Some(600),
    },
    ModelCapabilities {
        id: "gpt-5.3-codex",
        provider: "openai",
        display_name: "GPT-5.3 Codex",
        tier: ModelTier::Supported,
        model_family: "codex",
        context_window: 128_000,
        max_output_tokens: 16_384,
        context_window_beta: None,
        max_output_tokens_beta: None,
        vision: true,
        image_tool_results: false,
        inline_video: false,
        supports_temperature: false,
        supports_top_p: false,
        supports_top_k: false,
        thinking: ThinkingSupport::None,
        supports_reasoning: true,
        effort_levels: GPT5_REASONING_EFFORT,
        // Codex is a Responses-API-primary model; web search is not listed as
        // a supported tool on its model card. Correctness commit will confirm.
        supports_web_search: true,
        supports_inference_geo: false,
        supports_compaction: false,
        supports_structured_output: true,
        supports_legacy_penalties: true,
        supports_thinking_budget_legacy: false,
        beta_headers: &[],
        call_timeout_secs: Some(600),
    },
];
