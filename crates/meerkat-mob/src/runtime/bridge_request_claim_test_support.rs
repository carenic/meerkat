//! Fixture for the mob bridge request recorders (`host_actor` and
//! `upcall_responder`): a registered generated peer-interaction authority, a
//! comms runtime that exposes it and counts completed interactions, and a
//! supervisor bridge request from a chosen sender under a chosen id.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use meerkat_core::agent::CommsRuntime as CoreCommsRuntime;
use meerkat_core::comms::{PeerId, SUPERVISOR_BRIDGE_INTENT};
use meerkat_core::handles::PeerInteractionHandle;
use meerkat_core::interaction::{
    InboxInteraction, InteractionContent, InteractionId, PeerIngressAuthDecision,
    PeerIngressAuthExemption, PeerIngressConvention, PeerIngressFact, PeerIngressIdentity,
    PeerIngressKind, PeerInputCandidate, PeerInputClass,
};
use meerkat_core::types::HandlingMode;
use meerkat_runtime::handles::{HandleDslAuthority, RuntimePeerInteractionHandle};
use meerkat_runtime::meerkat_machine::dsl as mm_dsl;

use crate::tokio;

/// A generated peer-interaction authority for a registered session, so its
/// peer request transitions apply.
pub(crate) fn registered_peer_handle() -> Arc<RuntimePeerInteractionHandle> {
    let dsl = Arc::new(HandleDslAuthority::ephemeral());
    dsl.apply_signal(mm_dsl::MeerkatMachineSignal::Initialize, "test::initialize")
        .expect("Initialize signal");
    dsl.apply_input(
        mm_dsl::MeerkatMachineInput::RegisterSession {
            session_id: mm_dsl::SessionId::from("bridge-request-claim-test".to_string()),
            runtime_epoch_id: None,
            initial_run_start_holds: std::collections::BTreeSet::new(),
        },
        "test::register_session",
    )
    .expect("RegisterSession input");
    Arc::new(RuntimePeerInteractionHandle::new(dsl))
}

/// A comms runtime with complete peer request authority that counts the
/// interactions it is asked to mark complete.
pub(crate) struct ClaimRecordingRuntime {
    handle: Arc<dyn PeerInteractionHandle>,
    notify: Arc<tokio::sync::Notify>,
    completed: AtomicUsize,
}

impl ClaimRecordingRuntime {
    pub(crate) fn new(handle: Arc<dyn PeerInteractionHandle>) -> Self {
        Self {
            handle,
            notify: Arc::new(tokio::sync::Notify::new()),
            completed: AtomicUsize::new(0),
        }
    }

    pub(crate) fn completed(&self) -> usize {
        self.completed.load(Ordering::SeqCst)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl CoreCommsRuntime for ClaimRecordingRuntime {
    fn inbox_notify(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.notify)
    }

    fn peer_request_response_authority_handle(&self) -> Option<Arc<dyn PeerInteractionHandle>> {
        Some(Arc::clone(&self.handle))
    }

    fn mark_interaction_complete(&self, _id: &InteractionId) {
        self.completed.fetch_add(1, Ordering::SeqCst);
    }
}

/// A supervisor bridge request from `sender` under the envelope id `id`, as
/// ingress admits it (the bridge's authentication exemption, `sender` as the
/// admitted canonical peer).
pub(crate) fn bridge_request_from(sender: PeerId, id: InteractionId) -> PeerInputCandidate {
    let interaction = InboxInteraction {
        sender_taint: None,
        id,
        from_route: Some(sender),
        from: "mob/__mob_supervisor__".to_string(),
        content: InteractionContent::Request {
            intent: SUPERVISOR_BRIDGE_INTENT.to_string(),
            params: serde_json::json!({}),
            blocks: None,
        },
        rendered_text: "bridge request".to_string(),
        handling_mode: HandlingMode::Queue,
        render_metadata: None,
        objective_id: None,
    };
    let ingress = PeerIngressFact::peer(
        id,
        PeerInputClass::ActionableRequest,
        PeerIngressKind::Request,
        Some(PeerIngressAuthDecision::Exempt(
            PeerIngressAuthExemption::SupervisorBridge,
        )),
        PeerIngressIdentity::new(
            sender,
            "mob/__mob_supervisor__",
            PeerIngressConvention::Request {
                request_id: id.to_string(),
                intent: SUPERVISOR_BRIDGE_INTENT.to_string(),
            },
        ),
    );
    PeerInputCandidate::new(interaction, ingress, None)
}
