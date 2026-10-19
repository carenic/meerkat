//! A live channel custody or playback-readiness read answers from one
//! machine snapshot (#1606). Each test holds a read right after it takes its
//! snapshot, commits a transition (activation, owner revocation, close), then
//! releases it: the read answers the phase its snapshot holds, never the
//! "not current" error a second snapshot would give, and the next read
//! answers the committed phase.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;

const CHANNEL: &str = "channel-custody-read";
const PENDING: &str = "pending-receipt";
const ACTIVATION: &str = "activation-receipt";
const OWNER: &str = "playback-owner";
const READINESS: &str = "playback-readiness";

fn channel() -> meerkat_core::LiveChannelId {
    meerkat_core::LiveChannelId::new(CHANNEL)
}

fn identity() -> dsl::SessionLlmIdentity {
    dsl::SessionLlmIdentity {
        model: "experimental-live".to_string(),
        provider: dsl::Provider::OpenAI,
        self_hosted_server_id: None,
        provider_params_repr: None,
        auth_binding: None,
    }
}

/// The session's runtime binding, with the channel opened on it.
struct Fixture {
    machine: Arc<MeerkatMachine>,
    session_id: SessionId,
    runtime_id: dsl::AgentRuntimeId,
    fence: dsl::FenceToken,
    generation: dsl::Generation,
}

impl Fixture {
    async fn open() -> Self {
        let machine = Arc::new(MeerkatMachine::ephemeral());
        let session_id = SessionId::new();
        machine
            .register_session(session_id.clone())
            .await
            .expect("register session");
        let authority = machine
            .session_dsl_authority(&session_id)
            .await
            .expect("registered session authority");
        let mut authority = authority
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut state = authority.state().clone();
        let session = session_id.to_string();
        let runtime_id = state
            .active_runtime_id
            .clone()
            .unwrap_or_else(|| dsl::AgentRuntimeId("runtime-custody-read".to_string()));
        let fence = state.active_fence_token.unwrap_or(dsl::FenceToken(1));
        let generation = state
            .active_runtime_generation
            .unwrap_or(dsl::Generation(1));
        state.active_runtime_id = Some(runtime_id.clone());
        state.active_fence_token = Some(fence);
        state.active_runtime_generation = Some(generation);
        state
            .live_active_channel_by_session
            .insert(session.clone(), CHANNEL.to_string());
        state
            .live_channel_session_by_channel
            .insert(CHANNEL.to_string(), session);
        state
            .live_channel_identity_by_channel
            .insert(CHANNEL.to_string(), identity());
        *authority = dsl::MeerkatMachineAuthority::recover_from_state(state)
            .expect("an opened live channel satisfies the generated invariants");
        drop(authority);
        Self {
            machine,
            session_id,
            runtime_id,
            fence,
            generation,
        }
    }

    async fn apply(&self, input: dsl::MeerkatMachineInput, what: &str) {
        let authority = self
            .machine
            .session_dsl_authority(&self.session_id)
            .await
            .expect("registered session authority");
        let mut authority = authority
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        dsl::MeerkatMachineMutator::apply(&mut *authority, input)
            .unwrap_or_else(|error| panic!("{what}: {error:?}"));
    }

    /// Resolve the mode, stage the channel and register its playback owner:
    /// the channel is Pending with a full stage.
    async fn stage(&self) {
        let session = self.session_id.to_string();
        self.apply(
            dsl::MeerkatMachineInput::ResolveLiveExecutionModeAdmission {
                session_id: session.clone(),
                channel_id: CHANNEL.to_string(),
                profile_id: "test-function-bridge".to_string(),
                requested_mode: dsl::LiveExecutionMode::FunctionBridge,
                function_bridge_available: true,
                client_context_available: false,
            },
            "resolve the execution mode",
        )
        .await;
        self.apply(
            dsl::MeerkatMachineInput::StageExperimentalLiveExecution {
                session_id: session.clone(),
                channel_id: CHANNEL.to_string(),
                runtime_id: self.runtime_id.clone(),
                fence_token: self.fence,
                generation: self.generation,
                canonical_seed_cursor: 0,
                pending_receipt: PENDING.to_string(),
            },
            "stage the channel",
        )
        .await;
        self.apply(
            dsl::MeerkatMachineInput::RegisterLivePlaybackOwner {
                session_id: session,
                channel_id: CHANNEL.to_string(),
                runtime_id: self.runtime_id.clone(),
                fence_token: self.fence,
                generation: self.generation,
                owner_id: OWNER.to_string(),
                readiness_id: READINESS.to_string(),
                pending_receipt: PENDING.to_string(),
            },
            "register the playback owner",
        )
        .await;
    }

    /// The provider answer binds the staged execution: the channel is Active.
    async fn activate(&self) {
        self.apply(
            dsl::MeerkatMachineInput::RecordLiveWebrtcAnswerAcceptedAndBindExecution {
                session_id: self.session_id.to_string(),
                channel_id: CHANNEL.to_string(),
                answer_observation_sequence: 1,
                runtime_id: self.runtime_id.clone(),
                fence_token: self.fence,
                generation: self.generation,
                canonical_seed_cursor: 0,
                activation_receipt: ACTIVATION.to_string(),
            },
            "activate the channel",
        )
        .await;
    }

    /// Revoke the close custody, then record the committed close.
    async fn close(&self, pending_receipt: Option<&str>, activation_receipt: Option<&str>) {
        let session = self.session_id.to_string();
        self.apply(
            dsl::MeerkatMachineInput::RevokeLiveChannelCloseCustody {
                session_id: session.clone(),
                channel_id: CHANNEL.to_string(),
                pending_receipt: pending_receipt.map(str::to_string),
                activation_receipt: activation_receipt.map(str::to_string),
            },
            "revoke the close custody",
        )
        .await;
        self.apply(
            dsl::MeerkatMachineInput::RecordLiveCloseClosed {
                session_id: session,
                channel_id: CHANNEL.to_string(),
                close_observation_sequence: 1,
            },
            "record the committed close",
        )
        .await;
    }

    async fn revoke_playback_owner(&self) {
        self.apply(
            dsl::MeerkatMachineInput::RevokeLivePlaybackOwner {
                session_id: self.session_id.to_string(),
                channel_id: CHANNEL.to_string(),
                owner_id: OWNER.to_string(),
                readiness_id: READINESS.to_string(),
            },
            "revoke the playback owner",
        )
        .await;
    }

    async fn custody_by_pending(&self) -> Result<LiveChannelCustodyProjection, RuntimeDriverError> {
        self.machine
            .validate_live_channel_custody_by_pending_receipt(&self.session_id, &channel(), PENDING)
            .await
    }

    async fn custody_by_activation(
        &self,
    ) -> Result<LiveChannelCustodyProjection, RuntimeDriverError> {
        self.machine
            .validate_live_channel_custody_by_activation_receipt(
                &self.session_id,
                &channel(),
                ACTIVATION,
            )
            .await
    }
}

fn assert_active(custody: &LiveChannelCustodyProjection) {
    match custody.state() {
        LiveChannelCustodyState::Active(activation) => {
            assert_eq!(activation.activation_receipt(), ACTIVATION);
        }
        other => panic!("expected the Active phase the snapshot holds, got {other:?}"),
    }
}

fn assert_closed(custody: &LiveChannelCustodyProjection) {
    assert!(
        matches!(custody.state(), LiveChannelCustodyState::Closed),
        "the next read answers the committed close: {:?}",
        custody.state()
    );
}

/// A pending-receipt read whose snapshot is Active answers Active while
/// the close commits under it; the next read answers Closed.
#[tokio::test]
async fn a_close_under_a_pending_receipt_read_reads_the_snapshot_phase() {
    let fixture = Arc::new(Fixture::open().await);
    fixture.stage().await;
    fixture.activate().await;

    let (entered, release) = fixture.machine.test_hold_next_live_custody_read();
    let read = tokio::spawn({
        let fixture = Arc::clone(&fixture);
        async move { fixture.custody_by_pending().await }
    });
    entered.await.expect("the read reaches its hold");
    fixture.close(Some(PENDING), None).await;
    release.send(()).expect("release the held read");

    let custody = read
        .await
        .expect("read task")
        .expect("a close in progress reads as a phase, not an error");
    assert_active(&custody);
    assert_closed(
        &fixture
            .custody_by_pending()
            .await
            .expect("read after the close"),
    );
}

/// An activation-receipt read whose snapshot is Active answers Active
/// while the close commits under it; the next read answers Closed.
#[tokio::test]
async fn a_close_under_an_activation_receipt_read_reads_the_snapshot_phase() {
    let fixture = Arc::new(Fixture::open().await);
    fixture.stage().await;
    fixture.activate().await;

    let (entered, release) = fixture.machine.test_hold_next_live_custody_read();
    let read = tokio::spawn({
        let fixture = Arc::clone(&fixture);
        async move { fixture.custody_by_activation().await }
    });
    entered.await.expect("the read reaches its hold");
    fixture.close(None, Some(ACTIVATION)).await;
    release.send(()).expect("release the held read");

    let custody = read
        .await
        .expect("read task")
        .expect("a close in progress reads as a phase, not an error");
    assert_active(&custody);
    assert_closed(
        &fixture
            .custody_by_activation()
            .await
            .expect("read after the close"),
    );
}

/// A pending-receipt read whose snapshot is Pending answers Pending
/// while the activation commits under it; the next read answers Active.
#[tokio::test]
async fn an_activation_under_a_pending_receipt_read_reads_the_snapshot_phase() {
    let fixture = Arc::new(Fixture::open().await);
    fixture.stage().await;

    let (entered, release) = fixture.machine.test_hold_next_live_custody_read();
    let read = tokio::spawn({
        let fixture = Arc::clone(&fixture);
        async move { fixture.custody_by_pending().await }
    });
    entered.await.expect("the read reaches its hold");
    fixture.activate().await;
    release.send(()).expect("release the held read");

    let custody = read
        .await
        .expect("read task")
        .expect("an activation in progress reads as a phase, not an error");
    match custody.state() {
        LiveChannelCustodyState::Pending(stage) => assert_eq!(stage.pending_receipt(), PENDING),
        other => panic!("expected the Pending phase the snapshot holds, got {other:?}"),
    }
    assert_active(
        &fixture
            .custody_by_pending()
            .await
            .expect("read after the activation"),
    );
}

/// A playback-readiness read answers from the one snapshot it holds, so
/// an owner revocation committed under it does not fail it half way; the
/// next read, whose snapshot holds the revocation, fails on the stage.
#[tokio::test]
async fn an_owner_revocation_under_a_readiness_read_reads_the_snapshot() {
    let fixture = Arc::new(Fixture::open().await);
    fixture.stage().await;

    let (entered, release) = fixture.machine.test_hold_next_live_custody_read();
    let read = tokio::spawn({
        let fixture = Arc::clone(&fixture);
        async move {
            fixture
                .machine
                .validate_live_playback_owner_readiness(
                    &fixture.session_id,
                    &channel(),
                    PENDING,
                    READINESS,
                )
                .await
        }
    });
    entered.await.expect("the read reaches its hold");
    fixture.revoke_playback_owner().await;
    release.send(()).expect("release the held read");

    let readiness = read
        .await
        .expect("read task")
        .expect("the readiness the snapshot holds answers whole");
    assert_eq!(readiness.owner_id(), OWNER);
    assert_eq!(readiness.readiness_id(), READINESS);
    assert!(
        fixture
            .machine
            .validate_live_playback_owner_readiness(
                &fixture.session_id,
                &channel(),
                PENDING,
                READINESS,
            )
            .await
            .is_err(),
        "a snapshot that holds the revocation has no readiness"
    );
}
