//! Provider-authored request and token-accounting evidence.
//!
//! Provider adapters mint these values while lowering an exact request. Shared
//! consumers may inspect them, but must never reconstruct them from raw cache
//! counters or rendered prompt text.

use crate::{Message, Provider};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Exact provider request encoding measured after all lowering.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoweredRequestEncoding {
    AnthropicMessagesJson,
    OpenAiResponsesJson,
    OpenAiChatCompletionsJson,
    GeminiGenerateContentJson,
}

/// Identity of the fully lowered provider request body used for pressure and
/// context evidence.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LoweredRequestProvenance {
    pub provider: Provider,
    pub encoding: LoweredRequestEncoding,
    pub body_sha256: [u8; 32],
}

impl LoweredRequestProvenance {
    pub fn from_body(
        provider: Provider,
        encoding: LoweredRequestEncoding,
        encoded_body: &[u8],
    ) -> Self {
        Self {
            provider,
            encoding,
            body_sha256: Sha256::digest(encoded_body).into(),
        }
    }
}

/// Provider-native convention used to normalize tokens presented to a model.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentedTokenConvention {
    /// Anthropic reports uncached, cache-write, and cache-read input as
    /// disjoint components. The normalized total is their saturating sum.
    AnthropicDisjointInputComponents,
    /// OpenAI prompt/input tokens already include the cached-token subset.
    OpenAiInputIncludesCachedSubset,
    /// Gemini prompt tokens already include the cached-content subset.
    GeminiPromptIncludesCachedSubset,
    /// OpenAI-compatible prompt tokens are treated as the provider's inclusive
    /// prompt total; cache detail fields are observational subsets only.
    OpenAiCompatiblePromptIncludesCacheDetails,
    /// A custom/embedded client explicitly declares its `input_tokens` field
    /// as an inclusive presented-input total. This is never inferred from
    /// cache detail fields.
    HostDeclaredInclusiveInputTotal,
}

/// How the provider adapter obtained the normalized presented-token total.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenAggregationProvenance {
    /// Sum of provider-documented disjoint input components.
    SumDisjointProviderComponents,
    /// Provider-issued inclusive prompt/input total copied without adding
    /// cache detail fields.
    ProviderInclusiveInputTotal,
}

/// One provider adapter's normalized accounting for a single model turn.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProviderTokenAccounting {
    pub provider: Provider,
    pub model: String,
    /// Tokens presented to the model for this request. Output tokens are not
    /// included.
    pub presented_tokens: u64,
    pub convention: PresentedTokenConvention,
    pub aggregation: TokenAggregationProvenance,
}

impl ProviderTokenAccounting {
    pub fn anthropic(
        model: impl Into<String>,
        uncached_input: u64,
        cache_creation_input: u64,
        cache_read_input: u64,
    ) -> Self {
        Self {
            provider: Provider::Anthropic,
            model: model.into(),
            presented_tokens: uncached_input
                .saturating_add(cache_creation_input)
                .saturating_add(cache_read_input),
            convention: PresentedTokenConvention::AnthropicDisjointInputComponents,
            aggregation: TokenAggregationProvenance::SumDisjointProviderComponents,
        }
    }

    pub fn openai(model: impl Into<String>, input_tokens: u64) -> Self {
        Self {
            provider: Provider::OpenAI,
            model: model.into(),
            presented_tokens: input_tokens,
            convention: PresentedTokenConvention::OpenAiInputIncludesCachedSubset,
            aggregation: TokenAggregationProvenance::ProviderInclusiveInputTotal,
        }
    }

    pub fn gemini(model: impl Into<String>, prompt_tokens: u64) -> Self {
        Self {
            provider: Provider::Gemini,
            model: model.into(),
            presented_tokens: prompt_tokens,
            convention: PresentedTokenConvention::GeminiPromptIncludesCachedSubset,
            aggregation: TokenAggregationProvenance::ProviderInclusiveInputTotal,
        }
    }

    pub fn openai_compatible(model: impl Into<String>, prompt_tokens: u64) -> Self {
        Self {
            provider: Provider::SelfHosted,
            model: model.into(),
            presented_tokens: prompt_tokens,
            convention: PresentedTokenConvention::OpenAiCompatiblePromptIncludesCacheDetails,
            aggregation: TokenAggregationProvenance::ProviderInclusiveInputTotal,
        }
    }

    pub fn host_declared(provider: Provider, model: impl Into<String>, input_tokens: u64) -> Self {
        Self {
            provider,
            model: model.into(),
            presented_tokens: input_tokens,
            convention: PresentedTokenConvention::HostDeclaredInclusiveInputTotal,
            aggregation: TokenAggregationProvenance::ProviderInclusiveInputTotal,
        }
    }
}

/// Stable provider-independent identity of an authored cache breakpoint.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CacheBreakpointBoundary {
    /// Stable leading system/profile prefix. `message_count` is the exclusive
    /// canonical transcript boundary.
    SystemProfilePrefix { message_count: u64 },
    /// Explicit breakpoint after one canonical transcript message.
    TranscriptAfter { message_count: u64 },
}

impl CacheBreakpointBoundary {
    pub const fn message_count(self) -> u64 {
        match self {
            Self::SystemProfilePrefix { message_count }
            | Self::TranscriptAfter { message_count } => message_count,
        }
    }
}

/// Provider cache lifetime explicitly selected for this authored breakpoint.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCacheTtl {
    FiveMinutes,
    OneHour,
    ThirtyMinutes,
    TwentyFourHours,
    ProviderDefault,
}

/// Proof that an exact provider lowering authored a cache breakpoint at one
/// canonical transcript boundary.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AuthoredCacheBreakpoint {
    provider: Provider,
    model: String,
    boundary: CacheBreakpointBoundary,
    /// Canonical lowercase `sha256:<hex>` over the canonical transcript prefix.
    canonical_prefix_sha256: String,
    /// Canonical serialized prefix byte count, retained to make accidental
    /// digest-domain mismatches fail closed.
    canonical_prefix_bytes: u64,
    /// Exact provider-native cache-prefix projection authored by the lowering
    /// that inserted this breakpoint. Canonical transcript identity maps the
    /// boundary; only this rendered identity can prove cache byte reuse.
    rendered_prefix_sha256: String,
    rendered_prefix_bytes: u64,
    /// Identity of the complete lowered request body from which the rendered
    /// prefix was projected. This prevents a prefix witness from floating
    /// free of its actual provider request.
    lowered_request_provenance: LoweredRequestProvenance,
    ttl: ProviderCacheTtl,
}

/// Failure to bind provider authoring to a canonical transcript prefix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CacheBreakpointEvidenceError {
    #[error("cache breakpoint boundary {message_count} exceeds transcript length {message_len}")]
    BoundaryOutOfRange {
        message_count: u64,
        message_len: usize,
    },
    #[error("cache breakpoint prefix could not be canonically encoded: {detail}")]
    CanonicalEncodingFailed { detail: String },
    #[error("persisted cache-breakpoint evidence is malformed: {detail}")]
    PersistedEvidenceMalformed { detail: String },
    #[error("cache-breakpoint evidence does not match the canonical transcript prefix")]
    CanonicalPrefixMismatch,
    #[error("cache-breakpoint rendered-prefix evidence is malformed")]
    RenderedPrefixMalformed,
    #[error("cache-breakpoint provider and lowered-request encoding are incoherent")]
    ProviderEncodingMismatch,
}

impl AuthoredCacheBreakpoint {
    pub(crate) fn from_provider_claim(claim: ProviderCacheBreakpointClaim) -> Self {
        claim.evidence
    }

    pub const fn provider(&self) -> Provider {
        self.provider
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub const fn boundary(&self) -> CacheBreakpointBoundary {
        self.boundary
    }

    pub fn canonical_prefix_sha256(&self) -> &str {
        &self.canonical_prefix_sha256
    }

    pub const fn canonical_prefix_bytes(&self) -> u64 {
        self.canonical_prefix_bytes
    }

    pub fn rendered_prefix_sha256(&self) -> &str {
        &self.rendered_prefix_sha256
    }

    pub const fn rendered_prefix_bytes(&self) -> u64 {
        self.rendered_prefix_bytes
    }

    pub const fn lowered_request_provenance(&self) -> LoweredRequestProvenance {
        self.lowered_request_provenance
    }

    pub const fn ttl(&self) -> ProviderCacheTtl {
        self.ttl
    }

    /// Validate the provider-rendered identity independently of canonical
    /// boundary validation. This does not prove target compatibility; a fork
    /// still needs a fresh target lowering with the same rendered identity.
    pub fn validate_rendered_identity(&self) -> Result<(), CacheBreakpointEvidenceError> {
        let hash = self.rendered_prefix_sha256.as_bytes();
        let valid_hash = hash.len() == 71
            && hash.starts_with(b"sha256:")
            && hash[7..]
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte));
        if !valid_hash || self.rendered_prefix_bytes == 0 {
            return Err(CacheBreakpointEvidenceError::RenderedPrefixMalformed);
        }
        let coherent_encoding = matches!(
            (self.provider, self.lowered_request_provenance.encoding),
            (
                Provider::Anthropic,
                LoweredRequestEncoding::AnthropicMessagesJson
            ) | (
                Provider::Gemini,
                LoweredRequestEncoding::GeminiGenerateContentJson
            ) | (
                Provider::OpenAI | Provider::SelfHosted,
                LoweredRequestEncoding::OpenAiResponsesJson
                    | LoweredRequestEncoding::OpenAiChatCompletionsJson
            )
        ) && self.lowered_request_provenance.provider == self.provider;
        if !coherent_encoding {
            return Err(CacheBreakpointEvidenceError::ProviderEncodingMismatch);
        }
        Ok(())
    }
}

/// Non-authoritative output of one provider adapter lowering.
///
/// A claim is freely cloneable for transport from the provider crate to core,
/// but it is not durable evidence and cannot be installed in a session. Core
/// promotes it only at the successful provider-turn commit boundary, or while
/// lending a one-shot target-lowering issuer.
#[derive(Debug, Clone)]
pub struct ProviderCacheBreakpointClaim {
    evidence: AuthoredCacheBreakpoint,
}

impl ProviderCacheBreakpointClaim {
    pub const fn provider(&self) -> Provider {
        self.evidence.provider()
    }

    pub fn model(&self) -> &str {
        self.evidence.model()
    }
}

/// Revalidated source-session cache evidence authorized for one fork proof.
///
/// This capability is neither cloneable nor serializable. Raw deserialization
/// of [`AuthoredCacheBreakpoint`] remains a data-loading operation and cannot
/// satisfy the source side of [`crate::ForkPoint::prove`].
#[derive(Debug)]
pub struct ValidatedSourceCacheBreakpoint {
    evidence: AuthoredCacheBreakpoint,
}

impl ValidatedSourceCacheBreakpoint {
    pub(crate) fn new(evidence: AuthoredCacheBreakpoint) -> Self {
        Self { evidence }
    }

    pub const fn provider(&self) -> Provider {
        self.evidence.provider()
    }

    pub fn model(&self) -> &str {
        self.evidence.model()
    }

    pub const fn boundary(&self) -> CacheBreakpointBoundary {
        self.evidence.boundary()
    }

    pub(crate) fn into_authored_evidence(self) -> AuthoredCacheBreakpoint {
        self.evidence
    }
}

/// Ephemeral proof that an active provider adapter freshly lowered the target
/// request and authored this exact cache prefix.
///
/// Unlike [`AuthoredCacheBreakpoint`], this capability is deliberately not
/// cloneable, serializable, or deserializable. Persisted source evidence
/// therefore cannot be replayed as target proof. It can only be minted while
/// core lends an unconstructable [`TargetCacheLoweringIssuer`] to the active
/// adapter lowering path.
#[derive(Debug)]
pub struct TargetCacheLoweringCapability {
    evidence: AuthoredCacheBreakpoint,
}

impl TargetCacheLoweringCapability {
    pub const fn provider(&self) -> Provider {
        self.evidence.provider()
    }

    pub fn model(&self) -> &str {
        self.evidence.model()
    }

    pub const fn boundary(&self) -> CacheBreakpointBoundary {
        self.evidence.boundary()
    }

    pub fn rendered_prefix_sha256(&self) -> &str {
        self.evidence.rendered_prefix_sha256()
    }

    pub const fn rendered_prefix_bytes(&self) -> u64 {
        self.evidence.rendered_prefix_bytes()
    }

    pub const fn lowered_request_provenance(&self) -> LoweredRequestProvenance {
        self.evidence.lowered_request_provenance()
    }

    pub const fn ttl(&self) -> ProviderCacheTtl {
        self.evidence.ttl()
    }

    pub(crate) fn into_authored_evidence(self) -> AuthoredCacheBreakpoint {
        self.evidence
    }
}

/// Core-issued authority lent only for one active adapter target lowering.
///
/// There is no public constructor. Custom [`crate::agent::AgentLlmClient`]
/// implementations receive this value only when core explicitly requests a
/// fresh target lowering, making their call to [`Self::mint`] a trusted
/// backend assertion rather than a generic public constructor.
#[derive(Debug)]
pub struct TargetCacheLoweringIssuer {
    _private: (),
}

impl TargetCacheLoweringIssuer {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }

    pub fn mint(
        &self,
        claim: ProviderCacheBreakpointClaim,
    ) -> Result<TargetCacheLoweringCapability, CacheBreakpointEvidenceError> {
        let evidence = AuthoredCacheBreakpoint::from_provider_claim(claim);
        evidence.validate_rendered_identity()?;
        Ok(TargetCacheLoweringCapability { evidence })
    }
}

/// Compute the canonical prefix identity shared by provider authoring and
/// durable fork validation.
pub fn canonical_cache_prefix_identity(
    messages: &[Message],
    message_count: u64,
) -> Result<(String, u64), CacheBreakpointEvidenceError> {
    let boundary = usize::try_from(message_count).map_err(|_| {
        CacheBreakpointEvidenceError::BoundaryOutOfRange {
            message_count,
            message_len: messages.len(),
        }
    })?;
    let prefix =
        messages
            .get(..boundary)
            .ok_or(CacheBreakpointEvidenceError::BoundaryOutOfRange {
                message_count,
                message_len: messages.len(),
            })?;
    crate::session::canonical_transcript_prefix_identity(prefix).map_err(|error| {
        CacheBreakpointEvidenceError::CanonicalEncodingFailed {
            detail: error.to_string(),
        }
    })
}

/// Exact adapter-lowering inputs needed to claim one cache breakpoint.
pub struct ProviderCacheBreakpointClaimRequest<'a> {
    pub provider: Provider,
    pub model: &'a str,
    pub messages: &'a [Message],
    pub boundary: CacheBreakpointBoundary,
    pub ttl: ProviderCacheTtl,
    pub rendered_prefix: &'a [u8],
    pub lowered_request_encoding: LoweredRequestEncoding,
    pub lowered_request_body: &'a [u8],
}

/// Build a non-authoritative provider claim that the renderer inserted the
/// matching breakpoint into this exact lowered request.
///
/// Only core may promote this claim into durable source evidence or an
/// ephemeral target-lowering capability. Arbitrary callers therefore cannot
/// mint provider-authored session authority through this generic renderer.
pub fn provider_cache_breakpoint_claim(
    request: ProviderCacheBreakpointClaimRequest<'_>,
) -> Result<ProviderCacheBreakpointClaim, CacheBreakpointEvidenceError> {
    let (canonical_prefix_sha256, canonical_prefix_bytes) =
        canonical_cache_prefix_identity(request.messages, request.boundary.message_count())?;
    let rendered_prefix_sha256 = format!("sha256:{:x}", Sha256::digest(request.rendered_prefix));
    let rendered_prefix_bytes = u64::try_from(request.rendered_prefix.len()).unwrap_or(u64::MAX);
    let lowered_request_provenance = LoweredRequestProvenance::from_body(
        request.provider,
        request.lowered_request_encoding,
        request.lowered_request_body,
    );
    let evidence = AuthoredCacheBreakpoint {
        provider: request.provider,
        model: request.model.to_string(),
        boundary: request.boundary,
        canonical_prefix_sha256,
        canonical_prefix_bytes,
        rendered_prefix_sha256,
        rendered_prefix_bytes,
        lowered_request_provenance,
        ttl: request.ttl,
    };
    evidence.validate_rendered_identity()?;
    Ok(ProviderCacheBreakpointClaim { evidence })
}
