//! Anthropic family detection and legacy fallback for non-catalog model IDs.
//!
//! All capability data for catalog models lives in
//! [`crate::capabilities::anthropic`]. This module is retained only to
//! synthesize a [`ModelCapabilities`] for model IDs that aren't in the static
//! catalog (e.g., dated snapshots such as `claude-haiku-4-5-20251001` or
//! future prefixes that haven't been added yet).

use crate::capabilities::{BetaHeader, ModelCapabilities, ThinkingSupport};
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

const OPUS_46_BETAS: &[BetaHeader] = &[
    BETA_COMPACTION,
    BETA_STRUCTURED_OUTPUT,
    BETA_INTERLEAVED_THINKING,
];
const STANDARD_BETAS: &[BetaHeader] = &[BETA_STRUCTURED_OUTPUT, BETA_INTERLEAVED_THINKING];
const OPUS_46_EFFORT: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// Detect the model family. Returns `None` for non-Anthropic models.
fn detect_family(model: &str) -> Option<&'static str> {
    let m = model.to_ascii_lowercase();
    if m.starts_with("claude-opus-4") {
        Some("claude-opus-4")
    } else if m.starts_with("claude-sonnet-4") {
        Some("claude-sonnet-4")
    } else if m.starts_with("claude-haiku-4") {
        Some("claude-haiku-4")
    } else if m.starts_with("claude-") {
        Some("claude")
    } else {
        None
    }
}

/// Synthesize capabilities for an Anthropic model ID that isn't in the catalog.
///
/// The shape matches the pre-refactor heuristic profile: Opus 4.6+ prefixes
/// get the opus46 schema shape (adaptive thinking, effort, inference_geo,
/// compaction); everything else gets the standard shape (enabled-only
/// thinking, no effort).
pub fn fallback_caps(model: &str) -> Option<ModelCapabilities> {
    let family = detect_family(model)?;
    let m = model.to_ascii_lowercase();
    // Opus 4.6/4.7+ prefixes get the richer "opus46 bucket" shape.
    let opus46_shape = m.starts_with("claude-opus-4-6") || m.starts_with("claude-opus-4-7");
    let (thinking, effort, inference_geo, compaction, betas) = if opus46_shape {
        (
            ThinkingSupport::AnthropicAdaptiveAndEnabled,
            OPUS_46_EFFORT,
            true,
            true,
            OPUS_46_BETAS,
        )
    } else {
        (
            ThinkingSupport::AnthropicEnabledOnly,
            &[] as &[&str],
            false,
            false,
            STANDARD_BETAS,
        )
    };
    let call_timeout_secs = match family {
        "claude-opus-4" => Some(300),
        "claude-sonnet-4" => Some(120),
        "claude-haiku-4" => Some(60),
        _ => None,
    };
    Some(ModelCapabilities {
        id: "",
        provider: "anthropic",
        display_name: "",
        tier: ModelTier::Supported,
        model_family: family,
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
        thinking,
        supports_reasoning: false,
        effort_levels: effort,
        supports_web_search: true,
        supports_inference_geo: inference_geo,
        supports_compaction: compaction,
        supports_structured_output: true,
        supports_legacy_penalties: false,
        supports_thinking_budget_legacy: true,
        beta_headers: betas,
        call_timeout_secs,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn detect_family_known_prefixes() {
        assert_eq!(detect_family("claude-opus-4-7"), Some("claude-opus-4"));
        assert_eq!(detect_family("claude-sonnet-4-6"), Some("claude-sonnet-4"));
        assert_eq!(
            detect_family("claude-haiku-4-5-20251001"),
            Some("claude-haiku-4"),
        );
        assert_eq!(detect_family("claude-something-else"), Some("claude"));
        assert_eq!(detect_family("gpt-5.2"), None);
    }

    #[test]
    fn fallback_opus_46_shape_for_4_6_and_4_7_prefixes() {
        let caps = fallback_caps("claude-opus-4-6-future-snapshot").unwrap();
        assert_eq!(caps.thinking, ThinkingSupport::AnthropicAdaptiveAndEnabled);
        assert!(!caps.effort_levels.is_empty());
        assert!(caps.supports_inference_geo);
        assert!(caps.supports_compaction);

        let caps = fallback_caps("claude-opus-4-7-preview").unwrap();
        assert_eq!(caps.thinking, ThinkingSupport::AnthropicAdaptiveAndEnabled);
        assert!(!caps.effort_levels.is_empty());
    }

    #[test]
    fn fallback_standard_shape_for_older_prefixes() {
        let caps = fallback_caps("claude-haiku-4-5-20251001").unwrap();
        assert_eq!(caps.thinking, ThinkingSupport::AnthropicEnabledOnly);
        assert!(caps.effort_levels.is_empty());
        assert!(!caps.supports_inference_geo);
        assert!(!caps.supports_compaction);
    }
}
