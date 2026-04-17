//! Per-model capability rows for Anthropic models.
//!
//! Seed values are intentionally matched to the current `profile()` output so
//! that `schema_builder::build_params_schema(caps) == profile_for(...).params_schema`
//! for every catalog model before the switchover. Correctness fixes land in a
//! later commit with updated rows + golden schema snapshots.

use super::{BetaHeader, BetaValue, ModelCapabilities, ThinkingSupport};
use crate::catalog::ModelTier;

const BETA_COMPACTION: BetaHeader = BetaHeader {
    feature: "compaction",
    header_name: "anthropic-beta",
    header_value: "compact-2026-01-12",
};

const BETA_STRUCTURED_OUTPUT: BetaHeader = BetaHeader {
    feature: "structured_output",
    header_name: "anthropic-beta",
    header_value: "structured-outputs-2025-11-13",
};

const BETA_INTERLEAVED_THINKING: BetaHeader = BetaHeader {
    feature: "interleaved_thinking",
    header_name: "anthropic-beta",
    header_value: "interleaved-thinking-2025-05-14",
};

const BETA_OUTPUT_300K: BetaValue<u32> = BetaValue {
    header: "anthropic-beta: output-300k-2026-03-24",
    value: 300_000,
};

const OPUS_46_BETAS: &[BetaHeader] = &[
    BETA_COMPACTION,
    BETA_STRUCTURED_OUTPUT,
    BETA_INTERLEAVED_THINKING,
];

const STANDARD_BETAS: &[BetaHeader] = &[BETA_STRUCTURED_OUTPUT, BETA_INTERLEAVED_THINKING];

const OPUS_46_EFFORT: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// Capability rows for Anthropic catalog models.
///
/// **Note**: these rows reflect the *pre-correctness-fix* schema — they match
/// what the heuristic-based `profile()` function currently emits so the parity
/// test passes. Authoritative per-model corrections (Opus 4.7's adaptive-only
/// thinking, Opus 4.6 dropping xhigh, Opus 4.6 1M context, etc.) land in a
/// later commit.
pub const CAPABILITIES: &[ModelCapabilities] = &[
    // Opus 4.7 — currently shares the opus46 schema bucket with Opus 4.6.
    ModelCapabilities {
        id: "claude-opus-4-7",
        provider: "anthropic",
        display_name: "Claude Opus 4.7",
        tier: ModelTier::Recommended,
        model_family: "claude-opus-4",
        context_window: 1_000_000,
        max_output_tokens: 32_768,
        context_window_beta: None,
        max_output_tokens_beta: Some(BETA_OUTPUT_300K),
        vision: true,
        image_tool_results: true,
        inline_video: false,
        supports_temperature: true,
        supports_top_p: false,
        supports_top_k: true,
        thinking: ThinkingSupport::AnthropicAdaptiveAndEnabled,
        supports_reasoning: false,
        effort_levels: OPUS_46_EFFORT,
        supports_web_search: true,
        supports_inference_geo: true,
        supports_compaction: true,
        supports_structured_output: true,
        supports_legacy_penalties: false,
        supports_thinking_budget_legacy: true,
        beta_headers: OPUS_46_BETAS,
        call_timeout_secs: 300,
    },
    // Opus 4.6 — opus46 schema bucket.
    ModelCapabilities {
        id: "claude-opus-4-6",
        provider: "anthropic",
        display_name: "Claude Opus 4.6",
        tier: ModelTier::Supported,
        model_family: "claude-opus-4",
        context_window: 200_000,
        max_output_tokens: 32_768,
        context_window_beta: None,
        max_output_tokens_beta: Some(BETA_OUTPUT_300K),
        vision: true,
        image_tool_results: true,
        inline_video: false,
        supports_temperature: true,
        supports_top_p: false,
        supports_top_k: true,
        thinking: ThinkingSupport::AnthropicAdaptiveAndEnabled,
        supports_reasoning: false,
        effort_levels: OPUS_46_EFFORT,
        supports_web_search: true,
        supports_inference_geo: true,
        supports_compaction: true,
        supports_structured_output: true,
        supports_legacy_penalties: false,
        supports_thinking_budget_legacy: true,
        beta_headers: OPUS_46_BETAS,
        call_timeout_secs: 300,
    },
    // Sonnet 4.6 — standard schema bucket today.
    ModelCapabilities {
        id: "claude-sonnet-4-6",
        provider: "anthropic",
        display_name: "Claude Sonnet 4.6",
        tier: ModelTier::Recommended,
        model_family: "claude-sonnet-4",
        context_window: 200_000,
        max_output_tokens: 16_384,
        context_window_beta: None,
        max_output_tokens_beta: None,
        vision: true,
        image_tool_results: true,
        inline_video: false,
        supports_temperature: true,
        supports_top_p: false,
        supports_top_k: true,
        thinking: ThinkingSupport::AnthropicEnabledOnly,
        supports_reasoning: false,
        effort_levels: &[],
        supports_web_search: true,
        supports_inference_geo: false,
        supports_compaction: false,
        supports_structured_output: true,
        supports_legacy_penalties: false,
        supports_thinking_budget_legacy: true,
        beta_headers: STANDARD_BETAS,
        call_timeout_secs: 120,
    },
    // Sonnet 4.5 — standard schema bucket.
    ModelCapabilities {
        id: "claude-sonnet-4-5",
        provider: "anthropic",
        display_name: "Claude Sonnet 4.5",
        tier: ModelTier::Supported,
        model_family: "claude-sonnet-4",
        context_window: 200_000,
        max_output_tokens: 16_384,
        context_window_beta: None,
        max_output_tokens_beta: None,
        vision: true,
        image_tool_results: true,
        inline_video: false,
        supports_temperature: true,
        supports_top_p: false,
        supports_top_k: true,
        thinking: ThinkingSupport::AnthropicEnabledOnly,
        supports_reasoning: false,
        effort_levels: &[],
        supports_web_search: true,
        supports_inference_geo: false,
        supports_compaction: false,
        supports_structured_output: true,
        supports_legacy_penalties: false,
        supports_thinking_budget_legacy: true,
        beta_headers: STANDARD_BETAS,
        call_timeout_secs: 120,
    },
    // Opus 4.5 — standard schema bucket (but real docs say effort IS supported;
    // correctness fix lands later).
    ModelCapabilities {
        id: "claude-opus-4-5",
        provider: "anthropic",
        display_name: "Claude Opus 4.5",
        tier: ModelTier::Supported,
        model_family: "claude-opus-4",
        context_window: 200_000,
        max_output_tokens: 32_768,
        context_window_beta: None,
        max_output_tokens_beta: None,
        vision: true,
        image_tool_results: true,
        inline_video: false,
        supports_temperature: true,
        supports_top_p: false,
        supports_top_k: true,
        thinking: ThinkingSupport::AnthropicEnabledOnly,
        supports_reasoning: false,
        effort_levels: &[],
        supports_web_search: true,
        supports_inference_geo: false,
        supports_compaction: false,
        supports_structured_output: true,
        supports_legacy_penalties: false,
        supports_thinking_budget_legacy: true,
        beta_headers: STANDARD_BETAS,
        call_timeout_secs: 300,
    },
];
