//! Ingress classification for incoming peer/event traffic.
//!
//! Runs synchronously in the sending task using receiver-owned trust/auth state.

use crate::inproc::InprocRegistry;
use crate::peer_comms_authority::{
    PeerCommsAuthority, PeerCommsEffect, PeerCommsInput, PeerCommsMutator,
};
use crate::trust::TrustedPeers;
use crate::types::InboxItem;
use meerkat_core::PeerInputClass;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::error;

/// Receiver-owned context for synchronous ingress classification.
///
/// Cloned into `InboxSender` so classification runs in the sending task
/// using receiver-owned trust/auth state.
pub(crate) struct IngressClassificationContext {
    pub(crate) require_peer_auth: bool,
    pub(crate) trusted_peers: Arc<parking_lot::RwLock<TrustedPeers>>,
    pub(crate) silent_intents: Arc<HashSet<String>>,
    pub(crate) dismiss_flag: Arc<AtomicBool>,
}

/// Result of classifying an inbox item for enqueue.
pub(crate) struct ClassificationResult {
    pub(crate) class: PeerInputClass,
    pub(crate) from_peer: Option<String>,
    pub(crate) lifecycle_peer: Option<String>,
    pub(crate) normalized_handling_mode: meerkat_core::types::HandlingMode,
}

pub(crate) enum ClassificationDecision {
    Drop,
    SetDismissFlag,
    Enqueue(ClassificationResult),
}

impl IngressClassificationContext {
    /// Classify an inbox item through the canonical PeerComms authority.
    pub(crate) fn classify(&self, item: &InboxItem) -> ClassificationDecision {
        let mut authority = PeerCommsAuthority::new();
        let transition = match item {
            InboxItem::External { envelope } => {
                let trusted = self.trusted_peers.read();
                let trusted_sender_name = trusted.get_peer(&envelope.from).map(|p| p.name.clone());
                let fallback_sender_name = trusted_sender_name.clone().unwrap_or_else(|| {
                    InprocRegistry::global()
                        .get_name_by_pubkey(&envelope.from)
                        .unwrap_or_else(|| envelope.from.to_peer_id())
                });
                authority.apply(PeerCommsInput::ClassifyExternalEnvelope {
                    require_peer_auth: self.require_peer_auth,
                    trusted_sender_name,
                    fallback_sender_name,
                    message_kind: envelope.kind.clone(),
                    silent_intents: (*self.silent_intents).clone(),
                })
            }
            InboxItem::PlainEvent {
                source,
                handling_mode,
                ..
            } => authority.apply(PeerCommsInput::ClassifyPlainEvent {
                source_name: source.to_string(),
                handling_mode: *handling_mode,
            }),
        };
        let transition = match transition {
            Ok(transition) => transition,
            Err(err) => {
                error!(?err, "peer comms authority rejected ingress classification");
                return ClassificationDecision::Drop;
            }
        };

        match transition.effects.into_iter().next() {
            Some(PeerCommsEffect::DropIngress) | None => ClassificationDecision::Drop,
            Some(PeerCommsEffect::SetDismissFlag) => {
                self.dismiss_flag.store(true, Ordering::SeqCst);
                ClassificationDecision::SetDismissFlag
            }
            Some(PeerCommsEffect::EnqueueClassifiedEntry {
                class,
                from_peer,
                lifecycle_peer,
                normalized_handling_mode,
            }) => ClassificationDecision::Enqueue(ClassificationResult {
                class,
                from_peer,
                lifecycle_peer,
                normalized_handling_mode,
            }),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::agent::types::MessageIntent;
    use crate::identity::{Keypair, PubKey, Signature};
    use crate::trust::TrustedPeer;
    use crate::types::{Envelope, MessageKind};
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use uuid::Uuid;

    fn make_keypair() -> Keypair {
        Keypair::generate()
    }

    fn make_context(
        require_peer_auth: bool,
        trusted_peers: TrustedPeers,
        silent_intents: Vec<&str>,
    ) -> IngressClassificationContext {
        IngressClassificationContext {
            require_peer_auth,
            trusted_peers: Arc::new(parking_lot::RwLock::new(trusted_peers)),
            silent_intents: Arc::new(silent_intents.into_iter().map(String::from).collect()),
            dismiss_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    fn expect_enqueue(result: ClassificationDecision) -> ClassificationResult {
        match result {
            ClassificationDecision::Enqueue(result) => result,
            ClassificationDecision::Drop => panic!("expected enqueue, got drop"),
            ClassificationDecision::SetDismissFlag => panic!("expected enqueue, got dismiss"),
        }
    }

    fn make_trusted_peers(name: &str, pubkey: &PubKey) -> TrustedPeers {
        TrustedPeers {
            peers: vec![TrustedPeer {
                name: name.to_string(),
                pubkey: *pubkey,
                addr: "tcp://127.0.0.1:4200".to_string(),
                meta: crate::PeerMeta::default(),
            }],
        }
    }

    fn make_envelope(from: &Keypair, kind: MessageKind) -> Envelope {
        Envelope {
            id: Uuid::new_v4(),
            from: from.public_key(),
            to: PubKey::new([2u8; 32]),
            kind,
            sig: Signature::new([0u8; 64]),
        }
    }

    #[test]
    fn classify_message_as_actionable_message() {
        let sender = make_keypair();
        let trusted = make_trusted_peers("sender-agent", &sender.public_key());
        let ctx = make_context(true, trusted, vec![]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Message {
                blocks: None,
                body: "hello".to_string(),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::ActionableMessage);
        assert_eq!(result.from_peer.as_deref(), Some("sender-agent"));
        assert!(result.lifecycle_peer.is_none());
    }

    #[test]
    fn classify_request_as_actionable_request() {
        let sender = make_keypair();
        let trusted = make_trusted_peers("sender-agent", &sender.public_key());
        let ctx = make_context(true, trusted, vec![]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Request {
                intent: "review".to_string(),
                params: serde_json::json!({}),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::ActionableRequest);
        assert_eq!(result.from_peer.as_deref(), Some("sender-agent"));
    }

    #[test]
    fn classify_response_as_response() {
        let sender = make_keypair();
        let trusted = make_trusted_peers("sender-agent", &sender.public_key());
        let ctx = make_context(true, trusted, vec![]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Response {
                in_reply_to: Uuid::new_v4(),
                status: crate::types::Status::Completed,
                result: serde_json::json!({}),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::Response);
    }

    #[test]
    fn classify_ack_drops() {
        let sender = make_keypair();
        let trusted = make_trusted_peers("sender-agent", &sender.public_key());
        let ctx = make_context(true, trusted, vec![]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Ack {
                in_reply_to: Uuid::new_v4(),
            },
        );
        let item = InboxItem::External { envelope };
        let result = ctx.classify(&item);
        assert!(matches!(result, ClassificationDecision::Drop));
    }

    #[test]
    fn classify_peer_added_lifecycle() {
        let sender = make_keypair();
        let trusted = make_trusted_peers("orchestrator", &sender.public_key());
        let ctx = make_context(true, trusted, vec![]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Request {
                intent: "mob.peer_added".to_string(),
                params: serde_json::json!({"peer": "new-agent"}),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::PeerLifecycleAdded);
        assert_eq!(result.lifecycle_peer.as_deref(), Some("new-agent"));
    }

    #[test]
    fn classify_peer_retired_lifecycle() {
        let sender = make_keypair();
        let trusted = make_trusted_peers("orchestrator", &sender.public_key());
        let ctx = make_context(true, trusted, vec![]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Request {
                intent: "mob.peer_retired".to_string(),
                params: serde_json::json!({"peer": "old-agent"}),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::PeerLifecycleRetired);
        assert_eq!(result.lifecycle_peer.as_deref(), Some("old-agent"));
    }

    #[test]
    fn classify_peer_unwired_lifecycle() {
        let sender = make_keypair();
        let trusted = make_trusted_peers("orchestrator", &sender.public_key());
        let ctx = make_context(true, trusted, vec![]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Request {
                intent: "mob.peer_unwired".to_string(),
                params: serde_json::json!({"peer": "other-agent"}),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::PeerLifecycleUnwired);
        assert_eq!(result.lifecycle_peer.as_deref(), Some("other-agent"));
    }

    #[test]
    fn classify_kickoff_failed_lifecycle() {
        let sender = make_keypair();
        let trusted = make_trusted_peers("orchestrator", &sender.public_key());
        let ctx = make_context(true, trusted, vec![]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Request {
                intent: "mob.kickoff_failed".to_string(),
                params: serde_json::json!({"peer": "worker-1", "role": "worker"}),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::PeerLifecycleKickoffFailed);
        assert_eq!(result.lifecycle_peer.as_deref(), Some("worker-1"));
    }

    #[test]
    fn classify_silent_request() {
        let sender = make_keypair();
        let trusted = make_trusted_peers("sender-agent", &sender.public_key());
        let ctx = make_context(true, trusted, vec!["my-silent-intent"]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Request {
                intent: "my-silent-intent".to_string(),
                params: serde_json::json!({}),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::SilentRequest);
    }

    #[test]
    fn classify_builtin_intent_in_silent_list() {
        // Silent matching must work for built-in intent names (e.g. "review"),
        // not just Custom variants. Regression test for P2 fix.
        let sender = make_keypair();
        let trusted = make_trusted_peers("sender-agent", &sender.public_key());
        let ctx = make_context(true, trusted, vec!["review"]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Request {
                intent: "review".to_string(),
                params: serde_json::json!({}),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::SilentRequest);
    }

    #[test]
    fn classify_untrusted_sender_auth_required_drops_at_ingress() {
        let sender = make_keypair();
        let trusted = TrustedPeers::new(); // sender NOT trusted
        let ctx = make_context(true, trusted, vec![]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Message {
                blocks: None,
                body: "hello".to_string(),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = ctx.classify(&item);
        assert!(matches!(result, ClassificationDecision::Drop));
    }

    #[test]
    fn classify_plain_event() {
        let ctx = make_context(false, TrustedPeers::new(), vec![]);
        let item = InboxItem::PlainEvent {
            blocks: None,
            body: "event".to_string(),
            source: meerkat_core::PlainEventSource::Tcp,
            handling_mode: meerkat_core::types::HandlingMode::Queue,
            interaction_id: None,
            render_metadata: None,
        };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::PlainEvent);
        assert_eq!(result.from_peer.as_deref(), Some("event:tcp"));
    }

    #[test]
    fn no_lifecycle_leakage_plain_events_remain_the_only_non_peer_inbox_class() {
        let ctx = make_context(false, TrustedPeers::new(), vec![]);
        let item = InboxItem::PlainEvent {
            body: "event".to_string(),
            source: meerkat_core::PlainEventSource::Tcp,
            handling_mode: meerkat_core::types::HandlingMode::Queue,
            interaction_id: None,
            blocks: None,
            render_metadata: None,
        };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::PlainEvent);
    }

    #[test]
    fn classify_lifecycle_without_peer_param_falls_back_to_sender() {
        let sender = make_keypair();
        let trusted = make_trusted_peers("orchestrator", &sender.public_key());
        let ctx = make_context(true, trusted, vec![]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Request {
                intent: "mob.peer_added".to_string(),
                params: serde_json::json!({}),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = expect_enqueue(ctx.classify(&item));
        assert_eq!(result.class, PeerInputClass::PeerLifecycleAdded);
        assert_eq!(result.lifecycle_peer.as_deref(), Some("orchestrator"));
    }

    #[test]
    fn classify_dismiss_sets_flag() {
        let sender = make_keypair();
        let trusted = make_trusted_peers("sender-agent", &sender.public_key());
        let ctx = make_context(true, trusted, vec![]);
        let envelope = make_envelope(
            &sender,
            MessageKind::Message {
                blocks: None,
                body: "DISMISS".to_string(),
                handling_mode: None,
            },
        );
        let item = InboxItem::External { envelope };
        let result = ctx.classify(&item);
        assert!(matches!(result, ClassificationDecision::SetDismissFlag));
        assert!(ctx.dismiss_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn message_intent_peer_added_roundtrip() {
        let intent = MessageIntent::from("mob.peer_added");
        assert_eq!(intent, MessageIntent::PeerAdded);
        assert_eq!(intent.as_str(), "mob.peer_added");
    }

    #[test]
    fn message_intent_peer_retired_roundtrip() {
        let intent = MessageIntent::from("mob.peer_retired");
        assert_eq!(intent, MessageIntent::PeerRetired);
        assert_eq!(intent.as_str(), "mob.peer_retired");
    }
}
