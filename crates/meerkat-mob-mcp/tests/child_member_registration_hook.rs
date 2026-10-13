//! Host member destination selection for child mobs: a member of a mob
//! created through the agent `mob_create` registers through the host's hook,
//! so no child member registers without its destination installed.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::sync::{Arc, Mutex};

use meerkat_core::auth::{PrincipalKind, PrincipalRef};
use meerkat_core::types::ToolCallView;
use meerkat_core::{AgentToolDispatcher, SessionId};
use meerkat_mob::{
    AgentIdentity, AudienceRef, MemberDestinationSelection, MemberRuntimeBuildIdentity,
    MemberRuntimeRegistrationHook, MobError, MobId, MobRuntimeMode,
};
use meerkat_mob_mcp::AgentMobToolSurface;
use serde_json::json;
use support::{CouncilFixture, ScriptedTurn};

struct RecordingHost {
    seen: Mutex<Vec<MemberRuntimeBuildIdentity>>,
    surfaces: Mutex<Vec<(String, Vec<String>)>>,
    refuse: bool,
}

#[async_trait::async_trait]
impl MemberRuntimeRegistrationHook for RecordingHost {
    async fn select_destination(
        &self,
        identity: &MemberRuntimeBuildIdentity,
    ) -> Result<MemberDestinationSelection, MobError> {
        self.seen.lock().unwrap().push(identity.clone());
        if self.refuse {
            return Err(MobError::WiringError(
                "host refused the child member".into(),
            ));
        }
        let owner = PrincipalRef::new(PrincipalKind::SharedAgent, identity.member().member.clone())
            .unwrap();
        Ok(MemberDestinationSelection {
            logical_owner: owner.clone(),
            context: identity.session_document().clone(),
            context_generation: 1,
            audience: AudienceRef::Principal { principal: owner },
        })
    }

    async fn expected_destination(
        &self,
        _identity: &MemberRuntimeBuildIdentity,
    ) -> Result<MemberDestinationSelection, MobError> {
        Err(MobError::WiringError("not used by these tests".into()))
    }

    async fn verify_member_surface(
        &self,
        identity: &MemberRuntimeBuildIdentity,
        surface: &[meerkat_core::ToolDef],
    ) -> Result<(), MobError> {
        self.surfaces.lock().unwrap().push((
            identity.member().member.clone(),
            surface.iter().map(|tool| tool.name.to_string()).collect(),
        ));
        Ok(())
    }
}

fn fixture(refuse: bool) -> (CouncilFixture, Arc<RecordingHost>) {
    let host = Arc::new(RecordingHost {
        seen: Mutex::new(Vec::new()),
        surfaces: Mutex::new(Vec::new()),
        refuse,
    });
    let hook = Arc::clone(&host);
    let fixture = CouncilFixture::new_runtime_backed_with(
        |_request| ScriptedTurn::Text("ok".to_string()),
        move |state, _root| state.with_member_runtime_registration_hook(hook),
    );
    (fixture, host)
}

async fn agent_mob_create(fixture: &CouncilFixture, mob_id: &str) {
    let definition = json!({
        "id": mob_id,
        "profiles": {
            "worker": {
                "model": "claude-sonnet-4-6",
                "tools": { "comms": true }
            }
        }
    });
    let raw =
        serde_json::value::RawValue::from_string(json!({ "definition": definition }).to_string())
            .unwrap();
    let surface: Arc<dyn AgentToolDispatcher> = Arc::new(AgentMobToolSurface::new(
        Arc::clone(&fixture.state),
        None,
        meerkat_runtime::mob_operator_authority::create_only_mob_operator_authority()
            .expect("generated authority"),
        "claude-sonnet-4-5".to_string(),
        SessionId::new(),
        None,
        None,
        None,
    ));
    surface
        .dispatch(ToolCallView {
            id: "surface-call",
            name: "mob_create",
            args: &raw,
        })
        .await
        .expect("the agent creates a child mob");
}

async fn spawn_child_member(
    fixture: &CouncilFixture,
    mob_id: &str,
) -> Result<meerkat_mob::SpawnResult, MobError> {
    fixture
        .state
        .mob_spawn(
            &MobId::from(mob_id),
            "worker".into(),
            AgentIdentity::from("child-worker"),
            Some(MobRuntimeMode::TurnDriven),
            None,
            None,
        )
        .await
}

/// A child member spawned through the state registers through the host hook
/// with its own native identity. Its destination is governed custody: the
/// stock runtime-backed composition installs no governed work host, so the
/// selected destination cannot be installed and the child member is refused
/// rather than registered unscoped. (A member that does install reports its
/// actual surface to the host; see the mob unit tests.)
#[tokio::test(flavor = "multi_thread")]
async fn a_child_member_registers_through_the_host_hook() {
    let (fixture, host) = fixture(false);
    let mob_id = format!("child-{}", fixture.scope);
    agent_mob_create(&fixture, &mob_id).await;
    let error = spawn_child_member(&fixture, &mob_id)
        .await
        .expect_err("no governed work host, no unscoped child member");
    assert!(
        error
            .to_string()
            .contains("a scoped destination requires the governed work host"),
        "{error}"
    );
    let seen = host.seen.lock().unwrap().clone();
    assert!(
        seen.iter()
            .any(|identity| identity.member().mob_id == mob_id
                && identity.member().member == "child-worker"),
        "the hook selected the child member's destination: {:?}",
        seen.iter()
            .map(|identity| identity.member().clone())
            .collect::<Vec<_>>()
    );
    assert!(
        host.surfaces.lock().unwrap().is_empty(),
        "no member was built, so no surface was reported"
    );
}

/// A host refusal refuses the child member: no child member can bypass the
/// host's selection.
#[tokio::test(flavor = "multi_thread")]
async fn a_host_refusal_refuses_the_child_member() {
    let (fixture, host) = fixture(true);
    let mob_id = format!("child-{}", fixture.scope);
    agent_mob_create(&fixture, &mob_id).await;
    let error = spawn_child_member(&fixture, &mob_id)
        .await
        .expect_err("a refused child member does not spawn");
    assert!(
        error.to_string().contains("host refused the child member"),
        "{error}"
    );
    assert!(
        host.seen
            .lock()
            .unwrap()
            .iter()
            .any(|identity| identity.member().mob_id == mob_id),
        "the refusal came from the hook"
    );
}
