//! A member whose restore at a mob's start fails after it bound its
//! generated operation owner. The failed provision releases that binding, so
//! the start's binding pass binds the member's session afresh: the mob comes
//! up Running with that member Broken, and a later operation repairs it.

use super::*;
use crate::runtime::MemberRestorePolicy;
use crate::runtime::provisioner::provision_finish_test_hooks;

const STEP: Duration = Duration::from_secs(60);

fn turn_driven(identity: &AgentIdentity) -> SpawnMemberSpec {
    let mut spec = SpawnMemberSpec::new("worker", identity.clone());
    spec.runtime_mode = Some(crate::MobRuntimeMode::TurnDriven);
    spec
}

/// Two workers, a crash, and a start that restores every member. The
/// restore of one fails right after its generated bind. Before the fix the
/// binding it left named the aborted attempt's registry, the start's binding
/// pass met it as "a different operation-registry binding incarnation", and
/// the whole start fell to Stopped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restore_failing_after_its_generated_bind_leaves_the_start_running() {
    let definition = with_unique_mob_id(sample_definition(), "startup-binding-release");
    let service = Arc::new(MockSessionService::new());
    let _adapter = service.enable_runtime_adapter();
    let storage = MobStorage::in_memory();
    let events = storage.events.clone();
    let runtime_metadata = storage.runtime_metadata.clone();
    let handle = MobBuilder::new(definition, storage)
        .with_session_service(service.clone())
        .create()
        .await
        .expect("create the mob");
    let failing = AgentIdentity::from("restore-fails");
    let healthy = AgentIdentity::from("restore-succeeds");
    for identity in [&failing, &healthy] {
        handle
            .spawn_spec(turn_driven(identity))
            .await
            .expect("spawn a worker");
    }
    let session = handle
        .resolve_bridge_session_id(&failing)
        .await
        .expect("a session-backed member");
    let restarted = Arc::new(service.cold_restart_preserving_durable_state().await);
    let runtime = restarted.enable_runtime_adapter();
    crash_stop_and_release_routes(handle).await;
    drop(service);

    provision_finish_test_hooks::fail_after_generated_bind(&session);
    let resumed = tokio::time::timeout(
        STEP,
        Box::pin(
            MobBuilder::for_resume(MobStorage::with_events_and_runtime_metadata(
                events,
                runtime_metadata,
            ))
            .with_session_service(restarted.clone())
            .with_member_restore_policy(MemberRestorePolicy::Eager)
            .notify_orchestrator_on_resume(false)
            .resume(),
        ),
    )
    .await
    .expect("the start answers")
    .expect("the start succeeds");
    assert!(
        !provision_finish_test_hooks::failure_after_generated_bind_armed(&session),
        "the member's restore reached its generated bind and failed there"
    );

    assert_eq!(
        resumed.status().await.expect("the mob's status"),
        MobState::Running,
        "one member's failed restore does not stop the start"
    );
    let failed = resumed
        .member_status(&failing)
        .await
        .expect("the failed member's status");
    assert_eq!(
        failed.status,
        crate::runtime::handle::MobMemberStatus::Broken,
        "{failed:?}"
    );
    let restored = resumed
        .member_status(&healthy)
        .await
        .expect("the healthy member's status");
    assert_eq!(
        restored.status,
        crate::runtime::handle::MobMemberStatus::Active,
        "{restored:?}"
    );
    assert!(
        runtime.contains_session(&session).await,
        "the start's binding pass registered the failed member's session and bound it afresh"
    );

    // A later operation repairs the member.
    tokio::time::timeout(STEP, resumed.respawn(failing.clone(), None))
        .await
        .expect("the respawn answers")
        .expect("the broken member is respawned");
    let repaired = resumed
        .member_status(&failing)
        .await
        .expect("the repaired member's status");
    assert_eq!(
        repaired.status,
        crate::runtime::handle::MobMemberStatus::Active,
        "{repaired:?}"
    );
    let _ = tokio::time::timeout(STEP, resumed.stop()).await;
}
