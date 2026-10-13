//! `approval/*` JSON-RPC handlers.

use std::sync::Arc;

use meerkat_contracts::{
    ApprovalDecideParams, ApprovalGetParams, ApprovalListParams, ApprovalListResult,
    ApprovalRequestParams,
};
use meerkat_core::ApprovalError;

use crate::error;
use crate::handlers::{RpcResponseExt, parse_params};
use crate::protocol::{RpcId, RpcResponse};
use crate::session_runtime::SessionRuntime;

fn approval_error_to_response(id: Option<RpcId>, error_value: ApprovalError) -> RpcResponse {
    let code = match error_value {
        ApprovalError::NotFound { .. } => error::INVALID_PARAMS,
        // Commitment truth stays distinct: nothing committed, another owner
        // won, or the commit is unknown (reconciled before reuse).
        ApprovalError::StoreUnavailable(_) => {
            meerkat_contracts::ErrorCode::ApprovalStoreUnavailable.jsonrpc_code()
        }
        ApprovalError::StoreConflict { .. } => {
            meerkat_contracts::ErrorCode::ApprovalConflict.jsonrpc_code()
        }
        ApprovalError::StoreCommitUnknown { .. } | ApprovalError::Unreconciled { .. } => {
            meerkat_contracts::ErrorCode::ApprovalCommitUnknown.jsonrpc_code()
        }
        ApprovalError::InvalidStoredRecord { .. } | ApprovalError::LifecycleInvariant(_) => {
            error::INTERNAL_ERROR
        }
        ApprovalError::AlreadyDecided { .. }
        | ApprovalError::Expired { .. }
        | ApprovalError::InvalidDecision { .. }
        | ApprovalError::EmptyAllowedDecisions
        | ApprovalError::InvalidPrincipal
        | ApprovalError::AuthenticationRequired { .. }
        | ApprovalError::NotEligible { .. }
        | ApprovalError::ConsentUnavailable { .. }
        | ApprovalError::InvalidMetadata(_) => error::INVALID_PARAMS,
    };
    RpcResponse::error(id, code, error_value.to_string())
}

pub async fn handle_request(
    id: Option<RpcId>,
    params: Option<&serde_json::value::RawValue>,
    runtime: Arc<SessionRuntime>,
) -> RpcResponse {
    let params: ApprovalRequestParams = match parse_params(params) {
        Ok(params) => params,
        Err(response) => return response.with_id(id),
    };
    match runtime.approval_service().request(params.request) {
        Ok(record) => RpcResponse::success(id, record),
        Err(error_value) => approval_error_to_response(id, error_value),
    }
}

pub async fn handle_list(
    id: Option<RpcId>,
    params: Option<&serde_json::value::RawValue>,
    runtime: Arc<SessionRuntime>,
) -> RpcResponse {
    let params: ApprovalListParams = match params {
        Some(_) => match parse_params(params) {
            Ok(params) => params,
            Err(response) => return response.with_id(id),
        },
        None => ApprovalListParams::default(),
    };
    match runtime.approval_service().list(params.filter) {
        Ok(approvals) => RpcResponse::success(id, ApprovalListResult { approvals }),
        Err(error_value) => approval_error_to_response(id, error_value),
    }
}

pub async fn handle_get(
    id: Option<RpcId>,
    params: Option<&serde_json::value::RawValue>,
    runtime: Arc<SessionRuntime>,
) -> RpcResponse {
    let params: ApprovalGetParams = match parse_params(params) {
        Ok(params) => params,
        Err(response) => return response.with_id(id),
    };
    match runtime.approval_service().get(&params.approval_id) {
        Ok(record) => RpcResponse::success(id, record),
        Err(error_value) => approval_error_to_response(id, error_value),
    }
}

/// Parameters of the host-only consent decision method. They only name the
/// approval and the decision, and must equal what the authenticated event
/// binds: an approver, actor, assurance, authentication scope or reason in
/// the body is an invalid parameter, never an identity.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsentDecideParams {
    approval_id: meerkat_core::ApprovalId,
    decision: meerkat_core::ApprovalDecision,
}

/// Decide native consent from one authenticated decision event, reached only
/// through the host-only dispatch entry. The event, not the request, supplies
/// the approver, its authentication scope and the reason; a request that
/// names another approval or decision is refused before the owner is asked.
pub fn handle_consent_decide(
    id: Option<RpcId>,
    params: Option<&serde_json::value::RawValue>,
    runtime: Arc<SessionRuntime>,
    event: meerkat_core::approval::consent::AuthenticatedConsentEvent,
) -> RpcResponse {
    let params: ConsentDecideParams = match parse_params(params) {
        Ok(params) => params,
        Err(response) => return response.with_id(id),
    };
    if &params.approval_id != event.approval_id() || params.decision != event.decision().decision {
        return RpcResponse::error(
            id,
            error::INVALID_PARAMS,
            "the consent decision does not match its authenticated event",
        );
    }
    match runtime.approval_service().decide_consent_event(event) {
        Ok(record) => RpcResponse::success(id, record),
        Err(error_value) => approval_error_to_response(id, error_value),
    }
}

pub async fn handle_decide(
    id: Option<RpcId>,
    params: Option<&serde_json::value::RawValue>,
    runtime: Arc<SessionRuntime>,
) -> RpcResponse {
    let params: ApprovalDecideParams = match parse_params(params) {
        Ok(params) => params,
        Err(response) => return response.with_id(id),
    };
    match runtime.approval_service().decide(
        &params.approval_id,
        params.decision,
        params.actor,
        params.reason,
        params.provenance,
    ) {
        Ok(record) => RpcResponse::success(id, record),
        Err(error_value) => approval_error_to_response(id, error_value),
    }
}
