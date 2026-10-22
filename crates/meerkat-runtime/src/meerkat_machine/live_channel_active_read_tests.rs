//! [`MeerkatMachine::session_live_channel_active`] answers from the
//! session's own machine state: true while that session has an active live
//! channel, false for another registered session and for an unregistered
//! one.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;

const CHANNEL: &str = "channel-active-read";

/// Open `CHANNEL` on `session_id` the way the live open commits it: the
/// session's active channel, the channel's session and its model identity.
async fn open_channel(machine: &MeerkatMachine, session_id: &SessionId) {
    let authority = machine
        .session_dsl_authority(session_id)
        .await
        .expect("registered session authority");
    let mut authority = authority
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut state = authority.state().clone();
    let session = session_id.to_string();
    state.active_runtime_id = Some(
        state
            .active_runtime_id
            .clone()
            .unwrap_or_else(|| dsl::AgentRuntimeId("runtime-channel-active-read".to_string())),
    );
    state.active_fence_token = Some(state.active_fence_token.unwrap_or(dsl::FenceToken(1)));
    state.active_runtime_generation = Some(
        state
            .active_runtime_generation
            .unwrap_or(dsl::Generation(1)),
    );
    state
        .live_active_channel_by_session
        .insert(session.clone(), CHANNEL.to_string());
    state
        .live_channel_session_by_channel
        .insert(CHANNEL.to_string(), session);
    state.live_channel_identity_by_channel.insert(
        CHANNEL.to_string(),
        dsl::SessionLlmIdentity {
            model: "experimental-live".to_string(),
            provider: dsl::Provider::OpenAI,
            self_hosted_server_id: None,
            provider_params_repr: None,
            auth_binding: None,
        },
    );
    *authority = dsl::MeerkatMachineAuthority::recover_from_state(state)
        .expect("an opened live channel satisfies the generated invariants");
}

#[tokio::test]
async fn a_session_reports_its_own_open_live_channel_only() {
    let machine = MeerkatMachine::ephemeral();
    let session_id = SessionId::new();
    let other = SessionId::new();
    for session in [&session_id, &other] {
        machine
            .register_session(session.clone())
            .await
            .expect("register session");
    }
    let before = machine.session_live_channel_active(&session_id).await;

    open_channel(&machine, &session_id).await;

    assert_eq!(
        (
            before,
            machine.session_live_channel_active(&session_id).await,
            machine.session_live_channel_active(&other).await,
            machine.session_live_channel_active(&SessionId::new()).await,
        ),
        (false, true, false, false),
        "only the session whose channel is open reports one; an unregistered \
         session reports none"
    );
}
