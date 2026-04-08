//! Canonical ingress classification authority for peer comms.
//!
//! This is intentionally a degenerate machine: it has no persistent lifecycle
//! state and exists purely to own classification semantics at ingress. Shell
//! code provides snapshots (trust state, sender naming, wire payload) and
//! executes the resulting effects.

use crate::agent::types::MessageIntent;
use crate::types::MessageKind;
use meerkat_core::PeerInputClass;
use meerkat_core::types::HandlingMode;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerCommsPhase {
    Ready,
}

#[derive(Debug, Clone)]
pub(crate) enum PeerCommsInput {
    ClassifyExternalEnvelope {
        require_peer_auth: bool,
        trusted_sender_name: Option<String>,
        fallback_sender_name: String,
        message_kind: MessageKind,
        silent_intents: HashSet<String>,
    },
    ClassifyPlainEvent {
        source_name: String,
        handling_mode: HandlingMode,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PeerCommsEffect {
    DropIngress,
    SetDismissFlag,
    EnqueueClassifiedEntry {
        class: PeerInputClass,
        from_peer: Option<String>,
        lifecycle_peer: Option<String>,
        normalized_handling_mode: HandlingMode,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerCommsTransition {
    pub next_phase: PeerCommsPhase,
    pub effects: Vec<PeerCommsEffect>,
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub(crate) enum PeerCommsError {
    #[error("peer comms authority does not transition out of Ready")]
    IllegalTransition,
}

mod sealed {
    pub trait Sealed {}
}

pub(crate) trait PeerCommsMutator: sealed::Sealed {
    fn apply(&mut self, input: PeerCommsInput) -> Result<PeerCommsTransition, PeerCommsError>;
}

#[derive(Debug, Default, Clone)]
pub(crate) struct PeerCommsAuthority;

impl sealed::Sealed for PeerCommsAuthority {}

impl PeerCommsAuthority {
    pub(crate) fn new() -> Self {
        Self
    }

    fn classify_external(
        &self,
        require_peer_auth: bool,
        trusted_sender_name: Option<String>,
        fallback_sender_name: String,
        message_kind: MessageKind,
        silent_intents: HashSet<String>,
    ) -> PeerCommsTransition {
        let from_peer = trusted_sender_name.or({
            if require_peer_auth {
                None
            } else {
                Some(fallback_sender_name)
            }
        });
        if require_peer_auth && from_peer.is_none() {
            return PeerCommsTransition {
                next_phase: PeerCommsPhase::Ready,
                effects: vec![PeerCommsEffect::DropIngress],
            };
        }

        let effect = match message_kind {
            MessageKind::Message {
                body,
                handling_mode,
                ..
            } => {
                if body.trim().eq_ignore_ascii_case("DISMISS") {
                    PeerCommsEffect::SetDismissFlag
                } else {
                    PeerCommsEffect::EnqueueClassifiedEntry {
                        class: PeerInputClass::ActionableMessage,
                        from_peer,
                        lifecycle_peer: None,
                        normalized_handling_mode: handling_mode.unwrap_or(HandlingMode::Queue),
                    }
                }
            }
            MessageKind::Request {
                intent,
                params,
                handling_mode,
            } => {
                let typed_intent = MessageIntent::from(intent.as_str());
                let lifecycle_peer = params
                    .get("peer")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(std::string::ToString::to_string)
                    .or_else(|| from_peer.clone());

                let class = match typed_intent {
                    MessageIntent::PeerAdded => PeerInputClass::PeerLifecycleAdded,
                    MessageIntent::PeerRetired => PeerInputClass::PeerLifecycleRetired,
                    MessageIntent::PeerUnwired => PeerInputClass::PeerLifecycleUnwired,
                    MessageIntent::KickoffFailed => PeerInputClass::PeerLifecycleKickoffFailed,
                    MessageIntent::KickoffCancelled => {
                        PeerInputClass::PeerLifecycleKickoffCancelled
                    }
                    _ if silent_intents.contains(intent.as_str()) => PeerInputClass::SilentRequest,
                    _ => PeerInputClass::ActionableRequest,
                };

                PeerCommsEffect::EnqueueClassifiedEntry {
                    class,
                    from_peer,
                    lifecycle_peer: match class {
                        PeerInputClass::PeerLifecycleAdded
                        | PeerInputClass::PeerLifecycleRetired
                        | PeerInputClass::PeerLifecycleUnwired
                        | PeerInputClass::PeerLifecycleKickoffFailed
                        | PeerInputClass::PeerLifecycleKickoffCancelled => lifecycle_peer,
                        _ => None,
                    },
                    normalized_handling_mode: handling_mode.unwrap_or(HandlingMode::Queue),
                }
            }
            MessageKind::Response { handling_mode, .. } => {
                PeerCommsEffect::EnqueueClassifiedEntry {
                    class: PeerInputClass::Response,
                    from_peer,
                    lifecycle_peer: None,
                    normalized_handling_mode: handling_mode.unwrap_or(HandlingMode::Queue),
                }
            }
            MessageKind::Ack { .. } => PeerCommsEffect::DropIngress,
        };

        PeerCommsTransition {
            next_phase: PeerCommsPhase::Ready,
            effects: vec![effect],
        }
    }
}

impl PeerCommsMutator for PeerCommsAuthority {
    fn apply(&mut self, input: PeerCommsInput) -> Result<PeerCommsTransition, PeerCommsError> {
        let transition = match input {
            PeerCommsInput::ClassifyExternalEnvelope {
                require_peer_auth,
                trusted_sender_name,
                fallback_sender_name,
                message_kind,
                silent_intents,
            } => self.classify_external(
                require_peer_auth,
                trusted_sender_name,
                fallback_sender_name,
                message_kind,
                silent_intents,
            ),
            PeerCommsInput::ClassifyPlainEvent {
                source_name,
                handling_mode,
            } => PeerCommsTransition {
                next_phase: PeerCommsPhase::Ready,
                effects: vec![PeerCommsEffect::EnqueueClassifiedEntry {
                    class: PeerInputClass::PlainEvent,
                    from_peer: Some(format!("event:{source_name}")),
                    lifecycle_peer: None,
                    normalized_handling_mode: handling_mode,
                }],
            },
        };
        Ok(transition)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::iter::FromIterator;

    #[test]
    fn untrusted_sender_drops_when_auth_required() {
        let mut authority = PeerCommsAuthority::new();
        let transition = authority
            .apply(PeerCommsInput::ClassifyExternalEnvelope {
                require_peer_auth: true,
                trusted_sender_name: None,
                fallback_sender_name: "fallback".to_string(),
                message_kind: MessageKind::Message {
                    body: "hello".to_string(),
                    blocks: None,
                    handling_mode: None,
                },
                silent_intents: HashSet::new(),
            })
            .unwrap();
        assert_eq!(transition.effects, vec![PeerCommsEffect::DropIngress]);
    }

    #[test]
    fn dismiss_is_handled_at_ingress() {
        let mut authority = PeerCommsAuthority::new();
        let transition = authority
            .apply(PeerCommsInput::ClassifyExternalEnvelope {
                require_peer_auth: false,
                trusted_sender_name: Some("peer".to_string()),
                fallback_sender_name: "fallback".to_string(),
                message_kind: MessageKind::Message {
                    body: "DISMISS".to_string(),
                    blocks: None,
                    handling_mode: None,
                },
                silent_intents: HashSet::new(),
            })
            .unwrap();
        assert_eq!(transition.effects, vec![PeerCommsEffect::SetDismissFlag]);
    }

    #[test]
    fn lifecycle_request_classifies_and_normalizes_handling_mode() {
        let mut authority = PeerCommsAuthority::new();
        let transition = authority
            .apply(PeerCommsInput::ClassifyExternalEnvelope {
                require_peer_auth: false,
                trusted_sender_name: Some("peer".to_string()),
                fallback_sender_name: "fallback".to_string(),
                message_kind: MessageKind::Request {
                    intent: "mob.kickoff_failed".to_string(),
                    params: json!({"peer": "helper"}),
                    handling_mode: None,
                },
                silent_intents: HashSet::new(),
            })
            .unwrap();
        assert_eq!(
            transition.effects,
            vec![PeerCommsEffect::EnqueueClassifiedEntry {
                class: PeerInputClass::PeerLifecycleKickoffFailed,
                from_peer: Some("peer".to_string()),
                lifecycle_peer: Some("helper".to_string()),
                normalized_handling_mode: HandlingMode::Queue,
            }]
        );
    }

    #[test]
    fn silent_request_classifies_from_wire_intent() {
        let mut authority = PeerCommsAuthority::new();
        let transition = authority
            .apply(PeerCommsInput::ClassifyExternalEnvelope {
                require_peer_auth: false,
                trusted_sender_name: Some("peer".to_string()),
                fallback_sender_name: "fallback".to_string(),
                message_kind: MessageKind::Request {
                    intent: "review".to_string(),
                    params: json!({}),
                    handling_mode: Some(HandlingMode::Steer),
                },
                silent_intents: HashSet::from_iter(["review".to_string()]),
            })
            .unwrap();
        assert_eq!(
            transition.effects,
            vec![PeerCommsEffect::EnqueueClassifiedEntry {
                class: PeerInputClass::SilentRequest,
                from_peer: Some("peer".to_string()),
                lifecycle_peer: None,
                normalized_handling_mode: HandlingMode::Steer,
            }]
        );
    }
}
