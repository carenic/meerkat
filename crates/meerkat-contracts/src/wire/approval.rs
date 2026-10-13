//! Approval protocol wire contracts.
//!
//! The durable approval domain types live in `meerkat-core`; this module
//! re-exports them as the canonical wire shape and adds surface request
//! wrappers for JSON-RPC/REST methods.

use serde::{Deserialize, Serialize};

pub use meerkat_core::{
    ApprovalActionKind, ApprovalDecision, ApprovalDecisionRecord, ApprovalId, ApprovalListFilter,
    ApprovalMemberRef, ApprovalMobRef, ApprovalOwnerRef, ApprovalPrincipalId,
    ApprovalProposedAction, ApprovalRecord, ApprovalRequest, ApprovalResourceId,
    ApprovalResourceKind, ApprovalResourceRef, ApprovalRisk, ApprovalStatus,
};

/// `approval/request` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ApprovalRequestParams {
    #[serde(flatten)]
    pub request: ApprovalRequest,
}

/// `approval/get` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ApprovalGetParams {
    pub approval_id: ApprovalId,
}

/// `approval/list` parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ApprovalListParams {
    #[serde(default)]
    pub filter: ApprovalListFilter,
}

/// `approval/list` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ApprovalListResult {
    pub approvals: Vec<ApprovalRecord>,
}

/// `approval/decide` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ApprovalDecideParams {
    pub approval_id: ApprovalId,
    pub decision: ApprovalDecision,
    pub actor: ApprovalPrincipalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<serde_json::Value>,
}

/// The notification carrying one recorded R3 consent transition.
pub const CONSENT_NOTICE_NOTIFICATION: &str = "approval/consent_notice";

/// `approval/consent_notice` parameters: one recorded transition of an R3
/// human consent approval, delivered to the connection that serves its
/// approver after the approval owner released its locks. It names the
/// approval and the tool call and grants nothing: an `approved` notice
/// executes nothing, and only a fresh explicit attempt of the retained call
/// spends the consent. A `requested` notice carries the approval's
/// presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ConsentNoticeParams {
    pub approval_id: ApprovalId,
    pub tool_call_id: String,
    pub kind: ConsentNoticeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<ConsentPresentationParams>,
}

impl ConsentNoticeParams {
    /// This notice carrying `presentation`.
    #[must_use]
    pub fn with_presentation(mut self, presentation: ConsentPresentationParams) -> Self {
        self.presentation = Some(presentation);
        self
    }
}

/// What a `requested` consent notice asks its approver to decide: the
/// approval owner's own record of it, as issued. It names the exact action
/// (its kind, the tool and the call's arguments), its risk, the account the
/// operation acts with and the mandate it runs under (each as the
/// operation's owners stated it; absent when they stated none), the weakest
/// approver assurance the owner's rule accepts and the approval's expiry. It
/// is shown only on the connection that serves the approver, before any
/// decision, and grants nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ConsentPresentationParams {
    pub action: ApprovalProposedAction,
    pub risk: ApprovalRisk,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mandate: Option<String>,
    pub accepted_assurance: meerkat_core::StockConsentAssurance,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// The recorded consent transition a notice reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ConsentNoticeKind {
    // A consent approval was issued for the retained call.
    Requested,
    // An authenticated decision approved it; nothing executed.
    Approved,
    // An authenticated decision declined it.
    Denied,
    // A fresh attempt consumed it at its physical entry.
    Used,
    // The owner withdrew the unspent consent; nothing executed.
    Withdrawn,
}

impl From<meerkat_core::approval::consent::ConsentNoticeKind> for ConsentNoticeKind {
    fn from(kind: meerkat_core::approval::consent::ConsentNoticeKind) -> Self {
        use meerkat_core::approval::consent::ConsentNoticeKind as Native;
        match kind {
            Native::Requested => Self::Requested,
            Native::Approved => Self::Approved,
            Native::Denied => Self::Denied,
            Native::Used => Self::Used,
            Native::Withdrawn => Self::Withdrawn,
        }
    }
}

impl From<&meerkat_core::approval::consent::ConsentNotice> for ConsentNoticeParams {
    fn from(notice: &meerkat_core::approval::consent::ConsentNotice) -> Self {
        Self {
            approval_id: notice.approval_id.clone(),
            tool_call_id: notice.tool_call_id.to_string(),
            kind: notice.kind.into(),
            presentation: None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};

    fn principal(value: &str) -> ApprovalPrincipalId {
        ApprovalPrincipalId::new(value).expect("valid principal")
    }

    /// A consent notice projects the native transition to its typed wire
    /// shape: the approval, the tool call and a snake_case kind, nothing
    /// else.
    #[test]
    fn a_consent_notice_projects_the_native_transition() {
        use meerkat_core::approval::consent::{ConsentNotice, ConsentNoticeKind as Native};
        let approval_id = ApprovalId::new();
        for (native, kind) in [
            (Native::Requested, "requested"),
            (Native::Approved, "approved"),
            (Native::Denied, "denied"),
            (Native::Used, "used"),
            (Native::Withdrawn, "withdrawn"),
        ] {
            let params = ConsentNoticeParams::from(&ConsentNotice {
                approval_id: approval_id.clone(),
                tool_call_id: "call-1".into(),
                kind: native,
            });
            let encoded = serde_json::to_value(&params).expect("encodes");
            assert_eq!(
                encoded,
                json!({
                    "approval_id": approval_id.to_string(),
                    "tool_call_id": "call-1",
                    "kind": kind,
                })
            );
            let decoded: ConsentNoticeParams = serde_json::from_value(encoded).expect("decodes");
            assert_eq!(decoded, params);
        }
    }

    /// A `requested` notice carries its presentation: the exact action, the
    /// risk, the stated account and mandate, the accepted assurance and the
    /// expiry; an owner statement that is absent is omitted, never empty.
    #[test]
    fn a_requested_notice_carries_its_presentation() {
        use meerkat_core::approval::consent::{ConsentNotice, ConsentNoticeKind as Native};
        let approval_id = ApprovalId::new();
        let expires_at = chrono::DateTime::parse_from_rfc3339("2030-01-02T03:04:05Z")
            .expect("expiry")
            .with_timezone(&chrono::Utc);
        let presentation = ConsentPresentationParams {
            action: ApprovalProposedAction {
                kind: ApprovalActionKind::ToolCall,
                summary: "read".to_string(),
                body: Some(json!({"folder": "inbox"})),
            },
            risk: ApprovalRisk::High,
            account: Some("alice at https://issuer.example".to_string()),
            mandate: Some("the work's mandate".to_string()),
            accepted_assurance: meerkat_core::StockConsentAssurance::DeviceTrusted,
            expires_at,
        };
        let params = ConsentNoticeParams::from(&ConsentNotice {
            approval_id,
            tool_call_id: "call-1".into(),
            kind: Native::Requested,
        })
        .with_presentation(presentation.clone());
        let encoded = serde_json::to_value(&params).expect("encodes");
        assert_eq!(
            encoded["presentation"],
            json!({
                "action": {"kind": "tool_call", "summary": "read", "body": {"folder": "inbox"}},
                "risk": "high",
                "account": "alice at https://issuer.example",
                "mandate": "the work's mandate",
                "accepted_assurance": "device_trusted",
                "expires_at": "2030-01-02T03:04:05Z",
            })
        );
        let decoded: ConsentNoticeParams = serde_json::from_value(encoded).expect("decodes");
        assert_eq!(decoded, params);

        let unstated = ConsentPresentationParams {
            account: None,
            mandate: None,
            ..presentation
        };
        let encoded = serde_json::to_value(&unstated).expect("encodes");
        assert!(encoded.get("account").is_none() && encoded.get("mandate").is_none());
    }

    #[test]
    fn approval_request_uses_typed_status_decision_and_metadata_fields() {
        let params = ApprovalRequestParams {
            request: ApprovalRequest {
                requester: principal("human:alice"),
                owner: ApprovalOwnerRef::Session {
                    session_id: meerkat_core::SessionId::new(),
                },
                resource: ApprovalResourceRef {
                    kind: ApprovalResourceKind::ShellCommand,
                    id: meerkat_core::ApprovalResourceId::new("shell:danger"),
                },
                proposed_action: ApprovalProposedAction {
                    kind: ApprovalActionKind::ShellCommand,
                    summary: "run shell command".to_string(),
                    body: Some(json!({"cmd": "rm -rf target/tmp"})),
                },
                risk: ApprovalRisk::High,
                request_body: Some(json!({"why": "cleanup"})),
                allowed_decisions: BTreeSet::from([
                    ApprovalDecision::Approve,
                    ApprovalDecision::Deny,
                ]),
                expires_at: None,
                metadata: meerkat_core::SurfaceMetadata {
                    labels: BTreeMap::from([(
                        "client.thread_id".to_string(),
                        "thread-1".to_string(),
                    )]),
                    app_context: Some(json!({"client_ref": "opaque"})),
                },
                request_provenance: Some(json!({"tool_call_id": "call-1"})),
            },
        };

        let value = serde_json::to_value(&params).expect("serialize approval request");
        assert_eq!(value["requester"], "human:alice");
        assert_eq!(value["owner"]["owner_type"], "session");
        assert_eq!(value["resource"]["kind"], "shell_command");
        assert_eq!(value["allowed_decisions"][0], "approve");
        assert_eq!(value["metadata"]["labels"]["client.thread_id"], "thread-1");
        let parsed: ApprovalRequestParams =
            serde_json::from_value(value).expect("deserialize approval request");
        assert_eq!(parsed, params);
    }

    #[test]
    fn approval_decide_uses_typed_decision_and_actor() {
        let params = ApprovalDecideParams {
            approval_id: ApprovalId::new(),
            decision: ApprovalDecision::Deny,
            actor: principal("human:bob"),
            reason: Some("too risky".to_string()),
            provenance: Some(json!({"client": "mobile"})),
        };

        let value = serde_json::to_value(&params).expect("serialize decision");
        assert_eq!(value["decision"], "deny");
        assert_eq!(value["actor"], "human:bob");
        let parsed: ApprovalDecideParams =
            serde_json::from_value(value).expect("deserialize decision");
        assert_eq!(parsed, params);
    }
}
