//! Shared cases for the resumed-session LLM identity rule.
//!
//! Every resume merge (the mob's, the factory's) must produce the same
//! identity for the same durable session, build values and resume mask. These
//! cases are the one table both merges are checked against: masks
//! {none, model, provider, model+provider, provider_params, auth_binding} over
//! a same-provider change, a provider change, an explicit binding, an
//! env-default durable binding and a self-hosted server change.

#![allow(clippy::expect_used)]

use crate::lifecycle::run_primitive::ProviderParamsOverride;
use crate::service::ResumeOverrideMask;
use crate::{AuthBindingRef, BindingId, BindingOrigin, Provider, RealmId, SessionLlmIdentity};

/// One resume: what the session persisted, what the build supplies, which
/// fields the build claims, and the identity the resumed build must seat.
#[derive(Debug, Clone)]
pub struct ResumeIdentityCase {
    pub name: String,
    pub durable: SessionLlmIdentity,
    /// The build's values. `auth_binding` is a binding the build names
    /// explicitly.
    pub candidate: SessionLlmIdentity,
    pub mask: ResumeOverrideMask,
    pub expected: SessionLlmIdentity,
}

fn binding(realm: &str, binding: &str, origin: BindingOrigin) -> AuthBindingRef {
    AuthBindingRef {
        realm: RealmId::parse(realm).expect("a literal realm id parses"),
        binding: BindingId::parse(binding).expect("a literal binding id parses"),
        profile: None,
        origin,
    }
}

fn configured(realm: &str, id: &str) -> AuthBindingRef {
    binding(realm, id, BindingOrigin::Configured)
}

fn params(temperature: f32) -> Option<ProviderParamsOverride> {
    Some(ProviderParamsOverride {
        temperature: Some(temperature),
        ..Default::default()
    })
}

fn identity(
    model: &str,
    provider: Provider,
    self_hosted_server_id: Option<&str>,
    provider_params: Option<ProviderParamsOverride>,
    auth_binding: Option<AuthBindingRef>,
) -> SessionLlmIdentity {
    SessionLlmIdentity {
        model: model.to_string(),
        provider,
        self_hosted_server_id: self_hosted_server_id.map(str::to_string),
        provider_params,
        auth_binding,
    }
}

const MASKS: [&str; 6] = [
    "none",
    "model",
    "provider",
    "model+provider",
    "provider_params",
    "auth_binding",
];

fn mask(name: &str) -> ResumeOverrideMask {
    let mut mask = ResumeOverrideMask::default();
    match name {
        "model" => mask.model = true,
        "provider" => mask.provider = true,
        "model+provider" => {
            mask.model = true;
            mask.provider = true;
        }
        "provider_params" => mask.provider_params = true,
        "auth_binding" => mask.auth_binding = true,
        _ => {}
    }
    mask
}

/// The full table.
#[must_use]
pub fn resume_identity_cases() -> Vec<ResumeIdentityCase> {
    let durable_params = params(0.9);
    let build_params = params(0.1);
    let openai = configured("global", "openai");
    let anthropic = configured("global", "anthropic");
    let muse = configured("global", "muse");
    let env_default = binding("env_default", "default", BindingOrigin::SyntheticEnvDefault);

    let durable_openai = identity(
        "gpt-5.4",
        Provider::OpenAI,
        None,
        durable_params.clone(),
        Some(openai.clone()),
    );
    let durable_env_default = identity(
        "gpt-5.4",
        Provider::OpenAI,
        None,
        durable_params.clone(),
        Some(env_default),
    );
    let durable_self_hosted = identity(
        "muse-glimmer-30b",
        Provider::SelfHosted,
        Some("muse_vllm"),
        durable_params,
        Some(muse.clone()),
    );

    let same_provider = identity(
        "gpt-5.5",
        Provider::OpenAI,
        None,
        build_params.clone(),
        None,
    );
    let provider_change = identity(
        "claude-sonnet-4-6",
        Provider::Anthropic,
        None,
        build_params.clone(),
        None,
    );
    let explicit_binding = identity(
        "claude-sonnet-4-6",
        Provider::Anthropic,
        None,
        build_params.clone(),
        Some(anthropic.clone()),
    );
    let server_change = identity(
        "cerebras-gpt-oss-120b",
        Provider::SelfHosted,
        None,
        build_params,
        None,
    );

    // (scenario, durable, candidate, expected per mask in MASKS order as
    // (model from build, provider from build, server, params from build,
    // binding)).
    struct Row {
        scenario: &'static str,
        durable: SessionLlmIdentity,
        candidate: SessionLlmIdentity,
        bindings: [Option<AuthBindingRef>; 6],
        servers: [Option<&'static str>; 6],
    }
    let rows = [
        Row {
            scenario: "same provider",
            durable: durable_openai.clone(),
            candidate: same_provider,
            bindings: [
                Some(openai.clone()),
                Some(openai.clone()),
                Some(openai.clone()),
                Some(openai.clone()),
                Some(openai.clone()),
                None,
            ],
            servers: [None; 6],
        },
        Row {
            scenario: "provider change",
            durable: durable_openai.clone(),
            candidate: provider_change.clone(),
            bindings: [
                Some(openai.clone()),
                Some(openai.clone()),
                None,
                None,
                Some(openai),
                None,
            ],
            servers: [None; 6],
        },
        Row {
            scenario: "explicit binding",
            durable: durable_openai,
            candidate: explicit_binding,
            bindings: [
                Some(anthropic.clone()),
                Some(anthropic.clone()),
                Some(anthropic.clone()),
                Some(anthropic.clone()),
                Some(anthropic.clone()),
                Some(anthropic),
            ],
            servers: [None; 6],
        },
        Row {
            scenario: "env-default durable binding",
            durable: durable_env_default,
            candidate: provider_change,
            bindings: [None, None, None, None, None, None],
            servers: [None; 6],
        },
        Row {
            scenario: "self-hosted server change",
            durable: durable_self_hosted,
            candidate: server_change,
            bindings: [
                Some(muse.clone()),
                Some(muse.clone()),
                Some(muse.clone()),
                Some(muse.clone()),
                Some(muse),
                None,
            ],
            // A build that claims the model or the provider re-derives the
            // server from its own values; otherwise the durable server stays.
            servers: [
                Some("muse_vllm"),
                None,
                None,
                None,
                Some("muse_vllm"),
                Some("muse_vllm"),
            ],
        },
    ];

    let mut cases = Vec::new();
    for row in rows {
        for (index, mask_name) in MASKS.iter().enumerate() {
            let mask = mask(mask_name);
            let expected = SessionLlmIdentity {
                model: if mask.model {
                    row.candidate.model.clone()
                } else {
                    row.durable.model.clone()
                },
                provider: if mask.provider {
                    row.candidate.provider
                } else {
                    row.durable.provider
                },
                self_hosted_server_id: row.servers[index].map(str::to_string),
                provider_params: if mask.provider_params {
                    row.candidate.provider_params.clone()
                } else {
                    row.durable.provider_params.clone()
                },
                auth_binding: row.bindings[index].clone(),
            };
            cases.push(ResumeIdentityCase {
                name: format!("{} / mask {mask_name}", row.scenario),
                durable: row.durable.clone(),
                candidate: row.candidate.clone(),
                mask,
                expected,
            });
        }
    }
    cases
}
