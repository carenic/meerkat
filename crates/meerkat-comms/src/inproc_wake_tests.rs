//! Wake routes: a send to a seated member with no live route waits for the
//! member's owner to wake it, inside the sender's own budget, then hands the
//! envelope off once; every refusal is typed and hands nothing off.
//!
//! These rows drive the registry with scripted owners. The owner's own
//! machine verdicts are exercised by the mob crate's wake rows.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::classify::test_support;
use crate::inbox::Inbox;
use crate::trust::TrustStore;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

const BUDGET: Duration = Duration::from_secs(30);

fn message(body: &str) -> MessageKind {
    MessageKind::Message {
        objective_id: None,
        content_taint: None,
        blocks: None,
        body: body.to_string(),
        handling_mode: None,
        in_reply_to: None,
    }
}

/// A response keeps capacity-only semantics: admitted means `Queued`, with no
/// drain needed, which keeps these rows about routing rather than ingress.
fn response() -> MessageKind {
    MessageKind::Response {
        in_reply_to: Uuid::new_v4(),
        status: crate::types::Status::Completed,
        result: serde_json::json!({ "ok": true }),
        blocks: None,
        content_taint: None,
        handling_mode: None,
        objective_id: None,
    }
}

fn queued(inbox: &Inbox) -> usize {
    inbox
        .classified_snapshot()
        .map_or(0, |snapshot| snapshot.total_count)
}

fn open_inbox() -> (Inbox, InboxSender) {
    Inbox::new_classified(test_support::classification_context(
        TrustStore::new(),
        false,
    ))
}

/// What a scripted owner does when it is asked to wake its member.
enum OnWake {
    /// Register `sender` as the member's live route, then answer ready.
    Materialize {
        registry: Arc<InprocRegistry>,
        namespace: String,
        name: String,
        pubkey: PubKey,
        sender: std::sync::Mutex<Option<InboxSender>>,
    },
    /// Release the stale generation, register the fresh one, answer ready
    /// (an owner rebuilding a member whose old inbox closed).
    Replace {
        registry: Arc<InprocRegistry>,
        namespace: String,
        name: String,
        pubkey: PubKey,
        stale: InboxSender,
        fresh: std::sync::Mutex<Option<InboxSender>>,
    },
    /// Answer ready without registering anything.
    ReadyWithoutRoute,
    /// Answer with this refusal.
    Refuse(PeerWakeRefusal),
    /// Answer that the key is no longer a member (a stale route).
    NotAMember,
    /// Answer that the owner consumed a silent control notice.
    ConsumeNotice,
    /// Never answer.
    Hang,
}

struct ScriptedOwner {
    on_wake: OnWake,
    waking: AtomicBool,
    calls: AtomicUsize,
    requests: parking_lot::Mutex<Vec<PeerWakeRequest>>,
}

impl ScriptedOwner {
    fn new(on_wake: OnWake) -> Arc<Self> {
        Arc::new(Self {
            on_wake,
            waking: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            requests: parking_lot::Mutex::new(Vec::new()),
        })
    }

    fn waking(self: Arc<Self>) -> Arc<Self> {
        self.waking.store(true, Ordering::SeqCst);
        self
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn weak(self: &Arc<Self>) -> Weak<dyn InprocPeerWaker> {
        let owner: Arc<dyn InprocPeerWaker> = Arc::clone(self) as Arc<dyn InprocPeerWaker>;
        Arc::downgrade(&owner)
    }
}

impl InprocPeerWaker for ScriptedOwner {
    fn wake(&self, request: PeerWakeRequest) -> PeerWakeFuture {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().push(request);
        let answer = match &self.on_wake {
            OnWake::Materialize {
                registry,
                namespace,
                name,
                pubkey,
                sender,
            } => {
                if let Some(sender) = sender.lock().unwrap().take() {
                    let outcome = registry.register_with_meta_in_namespace(
                        namespace,
                        name.clone(),
                        *pubkey,
                        sender,
                        PeerMeta::default(),
                    );
                    assert!(!outcome.is_rejected(), "{outcome:?}");
                }
                Some(Ok(PeerWakeAnswer::Ready))
            }
            OnWake::Replace {
                registry,
                namespace,
                name,
                pubkey,
                stale,
                fresh,
            } => {
                registry.unregister_sender_in_namespace(namespace, pubkey, stale);
                if let Some(fresh) = fresh.lock().unwrap().take() {
                    let outcome = registry.register_with_meta_in_namespace(
                        namespace,
                        name.clone(),
                        *pubkey,
                        fresh,
                        PeerMeta::default(),
                    );
                    assert!(!outcome.is_rejected(), "{outcome:?}");
                }
                Some(Ok(PeerWakeAnswer::Ready))
            }
            OnWake::ReadyWithoutRoute => Some(Ok(PeerWakeAnswer::Ready)),
            OnWake::Refuse(refusal) => Some(Err(PeerWakeDenial::Refused(refusal.clone()))),
            OnWake::NotAMember => Some(Err(PeerWakeDenial::NotAMember)),
            OnWake::ConsumeNotice => Some(Ok(PeerWakeAnswer::ControlNoticeConsumed)),
            OnWake::Hang => None,
        };
        Box::pin(async move {
            match answer {
                Some(answer) => answer,
                None => std::future::pending().await,
            }
        })
    }

    fn member_is_waking(&self, _member: &str) -> bool {
        self.waking.load(Ordering::SeqCst)
    }
}

/// R1 (comms half): a dormant member's key has only a wake route. The send
/// asks the owner once, then hands the envelope off once to the inbox the
/// wake registered; a second send finds the live route and wakes nothing.
#[tokio::test]
async fn a_send_to_a_dormant_member_wakes_it_once_then_delivers_once() {
    let registry = Arc::new(InprocRegistry::new());
    let member = make_keypair();
    let (inbox, inbox_sender) = open_inbox();
    let owner = ScriptedOwner::new(OnWake::Materialize {
        registry: Arc::clone(&registry),
        namespace: "realm".to_string(),
        name: "worker-1".to_string(),
        pubkey: member.public_key(),
        sender: std::sync::Mutex::new(Some(inbox_sender)),
    });
    assert!(registry.publish_wake_route("realm", member.public_key(), "worker-1", owner.weak()));
    let sender = make_keypair();

    let first = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &sender,
            &member.public_key(),
            Uuid::new_v4(),
            response(),
            true,
            BUDGET,
        )
        .await
        .expect("the wake answered ready and the envelope was handed off");
    assert_eq!(
        first.outcome,
        InprocDeliveryOutcome::Resolved(IngressDeliveryOutcome::Queued)
    );
    assert_eq!(owner.calls(), 1);
    let request = owner.requests.lock()[0].clone();
    assert_eq!(request.member, "worker-1");
    assert_eq!(request.recipient, member.public_key());
    assert_eq!(request.sender, sender.public_key());
    assert!(request.observed_missing);
    assert!(!request.sender_waking);
    assert_eq!(request.input_class, PeerWakeInputClass::Content);

    registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &sender,
            &member.public_key(),
            Uuid::new_v4(),
            response(),
            true,
            BUDGET,
        )
        .await
        .expect("the live route serves the second send");
    assert_eq!(owner.calls(), 1, "a live route is never woken");
    assert_eq!(
        queued(&inbox),
        2,
        "each envelope was handed off exactly once"
    );
}

/// R3 (comms half): a key with neither route is not a member.
#[tokio::test]
async fn a_key_with_no_route_at_all_is_peer_not_found() {
    let registry = InprocRegistry::new();
    let result = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &make_keypair(),
            &make_keypair().public_key(),
            Uuid::new_v4(),
            message("hello"),
            true,
            BUDGET,
        )
        .await;
    assert!(matches!(result, Err(InprocSendError::PeerNotFound(_))));
}

/// R4/R5 (comms half): a refused wake is typed and hands nothing off.
#[tokio::test]
async fn a_refused_wake_is_typed_and_never_peer_not_found() {
    let registry = InprocRegistry::new();
    let member = make_keypair();
    let refusal = PeerWakeRefusal::MemberBroken {
        reason: "missing durable session snapshot".to_string(),
        hold: None,
    };
    let owner = ScriptedOwner::new(OnWake::Refuse(refusal.clone()));
    registry.publish_wake_route("realm", member.public_key(), "worker-1", owner.weak());

    let result = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &make_keypair(),
            &member.public_key(),
            Uuid::new_v4(),
            message("hello"),
            true,
            BUDGET,
        )
        .await;
    match result {
        Err(InprocSendError::WakeRefused(actual)) => assert_eq!(actual, refusal),
        other => panic!("expected the typed refusal, got {other:?}"),
    }
}

/// R8 (comms half): a closed live route of a seated member is stale. It takes
/// the wake path (observed missing), and the envelope reaches the member's
/// next inbox exactly once; the closed generation never received it.
#[tokio::test]
async fn a_closed_stale_live_route_takes_the_wake_path() {
    let registry = Arc::new(InprocRegistry::new());
    let member = make_keypair();
    let (stale_inbox, stale_sender) = open_inbox();
    registry.register_with_meta_in_namespace(
        "realm",
        "worker-1",
        member.public_key(),
        stale_sender.clone(),
        PeerMeta::default(),
    );
    drop(stale_inbox);
    let (fresh_inbox, fresh_sender) = open_inbox();
    let owner = ScriptedOwner::new(OnWake::Replace {
        registry: Arc::clone(&registry),
        namespace: "realm".to_string(),
        name: "worker-1".to_string(),
        pubkey: member.public_key(),
        stale: stale_sender,
        fresh: std::sync::Mutex::new(Some(fresh_sender)),
    });
    registry.publish_wake_route("realm", member.public_key(), "worker-1", owner.weak());

    let delivered = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &make_keypair(),
            &member.public_key(),
            Uuid::new_v4(),
            response(),
            true,
            BUDGET,
        )
        .await
        .expect("the fresh generation receives the envelope");
    assert_eq!(
        delivered.outcome,
        InprocDeliveryOutcome::Resolved(IngressDeliveryOutcome::Queued)
    );
    assert_eq!(owner.calls(), 1);
    assert!(owner.requests.lock()[0].observed_missing);
    assert_eq!(queued(&fresh_inbox), 1, "handed off exactly once");
}

/// R8 (comms half): a member that answers ready but has no route twice is
/// `materialization_lost` after exactly two wakes, and nothing was handed off.
#[tokio::test]
async fn ready_without_a_route_twice_is_materialization_lost() {
    let registry = InprocRegistry::new();
    let member = make_keypair();
    let owner = ScriptedOwner::new(OnWake::ReadyWithoutRoute);
    registry.publish_wake_route("realm", member.public_key(), "worker-1", owner.weak());

    let result = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &make_keypair(),
            &member.public_key(),
            Uuid::new_v4(),
            message("hello"),
            true,
            BUDGET,
        )
        .await;
    assert!(matches!(
        result,
        Err(InprocSendError::WakeRefused(
            PeerWakeRefusal::MaterializationLost
        ))
    ));
    assert_eq!(
        owner.calls(),
        2,
        "one wake for the miss, one re-wake after ready"
    );
}

/// R93 (comms half): the sender's budget bounds the wait for the wake. It
/// ends as `wake_timed_out` (not delivered); the owner's wake goes on.
#[tokio::test(start_paused = true)]
async fn a_pending_wake_ends_with_the_senders_budget_as_wake_timed_out() {
    let registry = InprocRegistry::new();
    let member = make_keypair();
    let owner = ScriptedOwner::new(OnWake::Hang);
    registry.publish_wake_route("realm", member.public_key(), "worker-1", owner.weak());

    let result = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &make_keypair(),
            &member.public_key(),
            Uuid::new_v4(),
            message("hello"),
            true,
            Duration::from_secs(5),
        )
        .await;
    assert!(matches!(
        result,
        Err(InprocSendError::WakeRefused(PeerWakeRefusal::WakeTimedOut))
    ));
    assert_eq!(owner.calls(), 1);
}

/// R108 (comms half): a sender that is itself being woken never waits on a
/// recipient that is waking too, even through the recipient's early live
/// route: the recipient's owner is asked first with `sender_waking`, answers
/// from its machine, and nothing is handed off.
#[tokio::test]
async fn a_waking_sender_never_waits_on_a_waking_recipient() {
    let registry = InprocRegistry::new();
    let sender = make_keypair();
    let recipient = make_keypair();
    let sender_owner = ScriptedOwner::new(OnWake::Hang).waking();
    registry.publish_wake_route("realm", sender.public_key(), "a", sender_owner.weak());
    let recipient_owner =
        ScriptedOwner::new(OnWake::Refuse(PeerWakeRefusal::SenderWaking)).waking();
    registry.publish_wake_route("realm", recipient.public_key(), "b", recipient_owner.weak());
    // The recipient's inbox is already registered (its wake is in flight).
    let (inbox, inbox_sender) = open_inbox();
    registry.register_with_meta_in_namespace(
        "realm",
        "b",
        recipient.public_key(),
        inbox_sender,
        PeerMeta::default(),
    );

    let result = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &sender,
            &recipient.public_key(),
            Uuid::new_v4(),
            message("from inside a build"),
            true,
            BUDGET,
        )
        .await;
    assert!(matches!(
        result,
        Err(InprocSendError::WakeRefused(PeerWakeRefusal::SenderWaking))
    ));
    let request = recipient_owner.requests.lock()[0].clone();
    assert!(request.sender_waking);
    assert!(!request.observed_missing);
    assert_eq!(queued(&inbox), 0, "nothing was handed off");
    assert_eq!(
        sender_owner.calls(),
        0,
        "the sender's own owner is only observed"
    );
}

/// R13: a wake route is namespace-scoped. A member dormant in namespace A is
/// not wakeable from namespace B: `PeerNotFound`, and its owner is not asked.
#[tokio::test]
async fn a_wake_route_never_crosses_namespaces() {
    let registry = InprocRegistry::new();
    let member = make_keypair();
    let owner = ScriptedOwner::new(OnWake::ReadyWithoutRoute);
    registry.publish_wake_route("realm-a", member.public_key(), "worker-1", owner.weak());

    let result = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm-b",
            &make_keypair(),
            &member.public_key(),
            Uuid::new_v4(),
            message("hello"),
            true,
            BUDGET,
        )
        .await;
    assert!(matches!(result, Err(InprocSendError::PeerNotFound(_))));
    assert_eq!(owner.calls(), 0);
}

/// A wake route whose owner is gone (its mob actor exited without
/// retracting it) is a member nothing in this process can wake: typed
/// `mob_unavailable`, never `PeerNotFound`.
#[tokio::test]
async fn a_route_whose_owner_is_gone_is_mob_unavailable() {
    let registry = InprocRegistry::new();
    let member = make_keypair();
    let owner = ScriptedOwner::new(OnWake::ReadyWithoutRoute);
    registry.publish_wake_route("realm", member.public_key(), "worker-1", owner.weak());
    drop(owner);

    let result = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &make_keypair(),
            &member.public_key(),
            Uuid::new_v4(),
            message("hello"),
            true,
            BUDGET,
        )
        .await;
    assert!(matches!(
        result,
        Err(InprocSendError::WakeRefused(
            PeerWakeRefusal::MobUnavailable
        ))
    ));
}

/// Retraction is owner-exact: another owner cannot retract a route it did not
/// publish, and a retracted key is no longer a member here.
#[test]
fn wake_route_retraction_is_owner_exact() {
    let registry = InprocRegistry::new();
    let member = make_keypair().public_key();
    let owner = ScriptedOwner::new(OnWake::ReadyWithoutRoute);
    let stranger = ScriptedOwner::new(OnWake::ReadyWithoutRoute);
    assert!(registry.publish_wake_route("realm", member, "worker-1", owner.weak()));
    assert!(!registry.retract_wake_route("realm", &member, &stranger.weak()));
    assert!(registry.has_wake_route_in_namespace("realm", &member));
    assert!(registry.retract_wake_route("realm", &member, &owner.weak()));
    assert!(!registry.has_wake_route_in_namespace("realm", &member));
    assert!(!registry.publish_wake_route("realm", PubKey([0u8; 32]), "zero", owner.weak()));
}

/// A wake route never claims a name: a live registration under the member's
/// name still succeeds, and a lifecycle notice classifies as lifecycle.
#[test]
fn a_wake_route_claims_no_name_and_classifies_lifecycle_notices() {
    let registry = InprocRegistry::new();
    let member = make_keypair().public_key();
    let owner = ScriptedOwner::new(OnWake::ReadyWithoutRoute);
    registry.publish_wake_route("realm", member, "worker-1", owner.weak());
    let (_inbox, sender) = open_inbox();
    assert!(
        !registry
            .register_with_meta_in_namespace(
                "realm",
                "worker-1",
                member,
                sender,
                PeerMeta::default()
            )
            .is_rejected()
    );
    let lifecycle = MessageKind::Lifecycle {
        kind: meerkat_core::comms::PeerLifecycleKind::PeerAdded,
        params: serde_json::json!({}),
    };
    assert_eq!(
        PeerWakeInputClass::of(&lifecycle),
        PeerWakeInputClass::Lifecycle
    );
    let kickoff = MessageKind::Lifecycle {
        kind: meerkat_core::comms::PeerLifecycleKind::KickoffPending,
        params: serde_json::json!({}),
    };
    assert_eq!(
        PeerWakeInputClass::of(&kickoff),
        PeerWakeInputClass::Content
    );
    assert_eq!(
        PeerWakeInputClass::of(&message("m")),
        PeerWakeInputClass::Content
    );
}

/// R9 (comms half): a live route closed for peer ingress stops resolving, so a
/// send meets the member's owner; reopening restores the same generation with
/// its metadata.
#[tokio::test]
async fn a_closed_live_route_routes_to_the_owner_until_reopened() {
    let registry = InprocRegistry::new();
    let member = make_keypair();
    let (inbox, inbox_sender) = open_inbox();
    registry.register_with_meta_in_namespace(
        "realm",
        "worker-1",
        member.public_key(),
        inbox_sender.clone(),
        PeerMeta::default().with_description("kept"),
    );
    let owner = ScriptedOwner::new(OnWake::Refuse(PeerWakeRefusal::MobNotRunning));
    registry.publish_wake_route("realm", member.public_key(), "worker-1", owner.weak());

    assert!(registry.close_sender_in_namespace("realm", &member.public_key(), &inbox_sender));
    let refused = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &make_keypair(),
            &member.public_key(),
            Uuid::new_v4(),
            response(),
            true,
            BUDGET,
        )
        .await;
    assert!(matches!(
        refused,
        Err(InprocSendError::WakeRefused(PeerWakeRefusal::MobNotRunning))
    ));
    assert_eq!(queued(&inbox), 0);

    assert!(registry.reopen_sender_in_namespace("realm", &member.public_key(), &inbox_sender));
    let peers = registry.peers_in_namespace("realm");
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].meta.description.as_deref(), Some("kept"));
    registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &make_keypair(),
            &member.public_key(),
            Uuid::new_v4(),
            response(),
            true,
            BUDGET,
        )
        .await
        .expect("the reopened route delivers");
    assert_eq!(queued(&inbox), 1);
    assert_eq!(owner.calls(), 1);
}

/// R7 (comms half): a member whose wake is in flight already has its inbox
/// registered. A send from a live sender that finds that early live route
/// joins the wake through the owner first (no observed miss), and hands off
/// only after the owner answered ready.
#[tokio::test]
async fn an_early_live_route_of_a_waking_member_joins_its_wake_first() {
    let registry = InprocRegistry::new();
    let member = make_keypair();
    let (inbox, inbox_sender) = open_inbox();
    registry.register_with_meta_in_namespace(
        "realm",
        "worker-1",
        member.public_key(),
        inbox_sender,
        PeerMeta::default(),
    );
    let owner = ScriptedOwner::new(OnWake::ReadyWithoutRoute).waking();
    registry.publish_wake_route("realm", member.public_key(), "worker-1", owner.weak());

    registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &make_keypair(),
            &member.public_key(),
            Uuid::new_v4(),
            response(),
            true,
            BUDGET,
        )
        .await
        .expect("delivered once the owner answered ready");
    assert_eq!(owner.calls(), 1);
    let request = owner.requests.lock()[0].clone();
    assert!(!request.observed_missing);
    assert!(!request.sender_waking);
    assert_eq!(queued(&inbox), 1);
}

/// A stale wake route (its owner no longer holds the key as a member) is a
/// non-member: `PeerNotFound`, the only construction of it on the wake path.
#[tokio::test]
async fn a_stale_route_whose_owner_denies_membership_is_peer_not_found() {
    let registry = InprocRegistry::new();
    let member = make_keypair();
    let owner = ScriptedOwner::new(OnWake::NotAMember);
    registry.publish_wake_route("realm", member.public_key(), "gone", owner.weak());
    let result = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &make_keypair(),
            &member.public_key(),
            Uuid::new_v4(),
            message("hello"),
            true,
            BUDGET,
        )
        .await;
    assert!(matches!(result, Err(InprocSendError::PeerNotFound(_))));
}

/// R4/R100 (comms half): a silent lifecycle notice to a dormant member is
/// consumed by its owner without a wake, as a live member's drain consumes
/// one; the sender gets the volatile hand-off fact, and no inbox received
/// anything.
#[tokio::test]
async fn a_silent_lifecycle_notice_to_a_dormant_member_is_consumed_by_its_owner() {
    let registry = InprocRegistry::new();
    let member = make_keypair();
    let owner = ScriptedOwner::new(OnWake::ConsumeNotice);
    registry.publish_wake_route("realm", member.public_key(), "worker-1", owner.weak());
    let notice = MessageKind::Lifecycle {
        kind: meerkat_core::comms::PeerLifecycleKind::PeerAdded,
        params: serde_json::json!({}),
    };
    let delivery = registry
        .send_to_pubkey_in_namespace_with_id_wait(
            "realm",
            &make_keypair(),
            &member.public_key(),
            Uuid::new_v4(),
            notice,
            true,
            BUDGET,
        )
        .await
        .expect("the owner consumed the notice");
    assert_eq!(
        delivery.outcome,
        InprocDeliveryOutcome::Resolved(IngressDeliveryOutcome::VolatileHandedOff)
    );
    assert_eq!(
        owner.requests.lock()[0].input_class,
        PeerWakeInputClass::Lifecycle
    );
}

fn make_keypair() -> Keypair {
    Keypair::generate()
}
