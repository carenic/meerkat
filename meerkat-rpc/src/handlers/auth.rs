//! `auth/*` + `realm/*` method handlers.
//!
//! Phase 4d-partial RPC surface. Read-side methods (list/get, status,
//! realm projection) resolve against the active `Config.realm` map;
//! write-side methods (create/delete/login/test/logout) currently
//! surface a typed "not yet implemented" error — the server-side
//! TokenStore persistence + OAuth interactive flow for RPC lands in a
//! follow-up.
//!
//! The method names + shapes match the RPC catalog entries registered
//! in meerkat-contracts::rpc_catalog::rpc_method_catalog (Phase 4c).

use serde_json::value::RawValue;

use meerkat_contracts::{
    WireAuthProfile, WireBackendProfile, WireProviderBinding, WireRealmConnectionSet,
};
use meerkat_core::RealmConnectionSet;

use super::{RpcResponseExt, parse_params};
use crate::error;
use crate::protocol::{RpcId, RpcResponse};
use crate::session_runtime::SessionRuntime;

#[derive(serde::Deserialize)]
struct RealmIdParams {
    realm_id: String,
}

#[derive(serde::Deserialize)]
struct AuthProfileIdParams {
    realm_id: String,
    profile_id: String,
}

async fn load_config(runtime: &SessionRuntime) -> Result<meerkat_core::Config, RpcResponse> {
    if let Some(cfg_runtime) = runtime.config_runtime() {
        cfg_runtime
            .get()
            .await
            .map(|snap| snap.config)
            .map_err(|e| {
                RpcResponse::error(
                    None,
                    error::INTERNAL_ERROR,
                    format!("Failed to load config: {e}"),
                )
            })
    } else {
        Ok(meerkat_core::Config::default())
    }
}

async fn resolve_realm(
    runtime: &SessionRuntime,
    realm_id: &str,
) -> Result<RealmConnectionSet, RpcResponse> {
    let config = load_config(runtime).await?;
    let section = config.realm.get(realm_id).ok_or_else(|| {
        RpcResponse::error(
            None,
            error::INVALID_PARAMS,
            format!("Unknown realm: {realm_id}"),
        )
    })?;
    RealmConnectionSet::from_config(realm_id, section).map_err(|e| {
        RpcResponse::error(
            None,
            error::INTERNAL_ERROR,
            format!("Realm config invalid: {e}"),
        )
    })
}

// --- Realm projection -------------------------------------------------

pub async fn handle_realm_list(id: Option<RpcId>, runtime: &SessionRuntime) -> RpcResponse {
    let config = match load_config(runtime).await {
        Ok(c) => c,
        Err(r) => return r.with_id(id),
    };
    let realms: Vec<serde_json::Value> = config
        .realm
        .iter()
        .map(|(realm_id, section)| {
            serde_json::json!({
                "realm_id": realm_id,
                "default_binding": section.default_binding,
                "backend_count": section.backend.len(),
                "auth_profile_count": section.auth.len(),
                "binding_count": section.binding.len(),
            })
        })
        .collect();
    RpcResponse::success(id, serde_json::json!({ "realms": realms }))
}

pub async fn handle_realm_get(
    id: Option<RpcId>,
    params: Option<&RawValue>,
    runtime: &SessionRuntime,
) -> RpcResponse {
    let parsed: RealmIdParams = match parse_params(params) {
        Ok(v) => v,
        Err(r) => return r.with_id(id),
    };
    let realm = match resolve_realm(runtime, &parsed.realm_id).await {
        Ok(r) => r,
        Err(r) => return r.with_id(id),
    };
    let wire = WireRealmConnectionSet::from(&realm);
    match serde_json::to_value(wire) {
        Ok(v) => RpcResponse::success(id, v),
        Err(e) => RpcResponse::error(id, error::INTERNAL_ERROR, format!("Serialize error: {e}")),
    }
}

// --- Auth profile CRUD ------------------------------------------------

pub async fn handle_auth_profile_list(
    id: Option<RpcId>,
    params: Option<&RawValue>,
    runtime: &SessionRuntime,
) -> RpcResponse {
    let parsed: RealmIdParams = match parse_params(params) {
        Ok(v) => v,
        Err(r) => return r.with_id(id),
    };
    let realm = match resolve_realm(runtime, &parsed.realm_id).await {
        Ok(r) => r,
        Err(r) => return r.with_id(id),
    };
    let profiles: Vec<WireAuthProfile> = realm
        .auth_profiles
        .values()
        .map(WireAuthProfile::from)
        .collect();
    let backends: Vec<WireBackendProfile> = realm
        .backends
        .values()
        .map(WireBackendProfile::from)
        .collect();
    let bindings: Vec<WireProviderBinding> = realm
        .bindings
        .values()
        .map(WireProviderBinding::from)
        .collect();
    RpcResponse::success(
        id,
        serde_json::json!({
            "realm_id": realm.realm_id,
            "auth_profiles": profiles,
            "backend_profiles": backends,
            "bindings": bindings,
        }),
    )
}

pub async fn handle_auth_profile_get(
    id: Option<RpcId>,
    params: Option<&RawValue>,
    runtime: &SessionRuntime,
) -> RpcResponse {
    let parsed: AuthProfileIdParams = match parse_params(params) {
        Ok(v) => v,
        Err(r) => return r.with_id(id),
    };
    let realm = match resolve_realm(runtime, &parsed.realm_id).await {
        Ok(r) => r,
        Err(r) => return r.with_id(id),
    };
    match realm.auth_profiles.get(&parsed.profile_id) {
        Some(p) => match serde_json::to_value(WireAuthProfile::from(p)) {
            Ok(v) => RpcResponse::success(id, v),
            Err(e) => {
                RpcResponse::error(id, error::INTERNAL_ERROR, format!("Serialize error: {e}"))
            }
        },
        None => RpcResponse::error(
            id,
            error::INVALID_PARAMS,
            format!(
                "Auth profile {}:{} not found",
                parsed.realm_id, parsed.profile_id
            ),
        ),
    }
}

pub async fn handle_auth_profile_create(id: Option<RpcId>) -> RpcResponse {
    RpcResponse::error(
        id,
        error::INVALID_REQUEST,
        "auth/profile/create not yet available via RPC — edit the realm config \
         TOML or call the CLI `rkat auth login` command; RPC interactive \
         OAuth flow lands in a follow-up commit.",
    )
}

pub async fn handle_auth_profile_delete(id: Option<RpcId>) -> RpcResponse {
    RpcResponse::error(
        id,
        error::INVALID_REQUEST,
        "auth/profile/delete not yet available via RPC — edit the realm config \
         TOML or call the CLI `rkat auth logout` command.",
    )
}

pub async fn handle_auth_profile_test(
    id: Option<RpcId>,
    params: Option<&RawValue>,
    runtime: &SessionRuntime,
) -> RpcResponse {
    // Read-only test: look up the profile in the realm config and
    // attempt to resolve it through the provider runtime registry.
    #[derive(serde::Deserialize)]
    struct TestParams {
        realm_id: String,
        binding_id: String,
    }
    let parsed: TestParams = match parse_params(params) {
        Ok(v) => v,
        Err(r) => return r.with_id(id),
    };
    let realm = match resolve_realm(runtime, &parsed.realm_id).await {
        Ok(r) => r,
        Err(r) => return r.with_id(id),
    };
    let registry = meerkat_client::ProviderRuntimeRegistry::default();
    let env = meerkat_client::ResolverEnvironment::with_process_env();
    let result = registry.resolve(&realm, &parsed.binding_id, &env).await;
    match result {
        Ok(conn) => RpcResponse::success(
            id,
            serde_json::json!({
                "state": "valid",
                "provider": conn.provider.as_str(),
                "backend_profile_id": conn.backend_profile.id,
                "has_credential": !matches!(
                    conn.shim_credential,
                    meerkat_client::ShimCredential::None
                ),
            }),
        ),
        Err(e) => RpcResponse::error(
            id,
            error::INVALID_REQUEST,
            format!("Binding resolution failed: {e}"),
        ),
    }
}

// --- Login (interactive) ---------------------------------------------

pub async fn handle_auth_login_start(id: Option<RpcId>) -> RpcResponse {
    RpcResponse::error(
        id,
        error::INVALID_REQUEST,
        "auth/login/start: interactive RPC OAuth lands in a follow-up; \
         use `rkat auth login` on the CLI for now.",
    )
}

pub async fn handle_auth_login_complete(id: Option<RpcId>) -> RpcResponse {
    RpcResponse::error(
        id,
        error::INVALID_REQUEST,
        "auth/login/complete: interactive RPC OAuth lands in a follow-up.",
    )
}

pub async fn handle_auth_login_device_start(id: Option<RpcId>) -> RpcResponse {
    RpcResponse::error(
        id,
        error::INVALID_REQUEST,
        "auth/login/device_start: device-code flow via RPC lands in a follow-up.",
    )
}

pub async fn handle_auth_status_get(
    id: Option<RpcId>,
    params: Option<&RawValue>,
    runtime: &SessionRuntime,
) -> RpcResponse {
    let parsed: AuthProfileIdParams = match parse_params(params) {
        Ok(v) => v,
        Err(r) => return r.with_id(id),
    };
    let realm = match resolve_realm(runtime, &parsed.realm_id).await {
        Ok(r) => r,
        Err(r) => return r.with_id(id),
    };
    match realm.auth_profiles.get(&parsed.profile_id) {
        Some(profile) => RpcResponse::success(
            id,
            serde_json::json!({
                "profile_id": profile.id,
                "provider": profile.provider.as_str(),
                "auth_method": profile.auth_method,
                "state": "unknown",
                "expires_at": null,
                "last_refresh_at": null,
                "account_id": null,
                "last_error": null,
            }),
        ),
        None => RpcResponse::error(
            id,
            error::INVALID_PARAMS,
            format!(
                "Auth profile {}:{} not found",
                parsed.realm_id, parsed.profile_id
            ),
        ),
    }
}

pub async fn handle_auth_logout(id: Option<RpcId>) -> RpcResponse {
    RpcResponse::error(
        id,
        error::INVALID_REQUEST,
        "auth/logout: persistent TokenStore clear via RPC lands in a follow-up; \
         use `rkat auth logout <profile_id>` on the CLI for now.",
    )
}
