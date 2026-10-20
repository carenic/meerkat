//! #1497 member continuations resolve through the mob roster: an address names
//! one incarnation (identity at generation, in one incarnation of its mob),
//! served by its current bridge session, and retired by a respawn, a
//! retirement, or the destroy of its mob.

use super::*;
use crate::continuation::{MobContinuationResolver, MobHandleLookup};
use meerkat::{
    AddressObservation, AddressResolution, ContinuationAddressResolver, ContinuationDelivery,
    ContinuationHandling, ContinuationKey, ContinuationOwner, ContinuationOwnerService,
    ContinuationProducer, ContinuationResultRef, ContinuationStatus, ContinuationSubmitError,
    StrandedCause,
};
use meerkat_core::service::MobIncarnationId;
use meerkat_runtime::{InMemoryRuntimeStore, LogicalRuntimeId, RuntimeDeliveryInbox};

struct OneMob(MobHandle);

#[async_trait::async_trait]
impl MobHandleLookup for OneMob {
    async fn mob_handle(&self, mob_id: &MobId) -> Option<MobHandle> {
        (self.0.mob_id() == mob_id).then(|| self.0.clone())
    }
}

/// The mob a host holds under one id, which the test replaces as a host's
/// state does when the mob is destroyed and created again under its id.
struct CurrentMob(std::sync::Mutex<MobHandle>);

impl CurrentMob {
    fn replace(&self, handle: MobHandle) {
        *self.0.lock().expect("current mob") = handle;
    }
}

#[async_trait::async_trait]
impl MobHandleLookup for CurrentMob {
    async fn mob_handle(&self, mob_id: &MobId) -> Option<MobHandle> {
        let handle = self.0.lock().expect("current mob").clone();
        (handle.mob_id() == mob_id).then_some(handle)
    }
}

fn delivery(key: &str) -> ContinuationDelivery {
    ContinuationDelivery {
        key: ContinuationKey::new(key).expect("key"),
        result: ContinuationResultRef {
            producer: ContinuationProducer::Host {
                namespace: "tasks".into(),
            },
            producer_id: "op-1".into(),
            result_digest: "sha256:result".into(),
            summary: None,
        },
        body: "result".into(),
        handling: ContinuationHandling::Queue,
    }
}

#[tokio::test]
async fn member_continuations_follow_the_incarnation_across_respawn_and_retirement() {
    let (handle, _service) = create_test_mob(sample_definition()).await;
    let identity = AgentIdentity::from("w-cont");
    handle
        .spawn_spec(SpawnMemberSpec::new("worker", identity.as_str()))
        .await
        .expect("spawn");
    let resolver = Arc::new(MobContinuationResolver::new(Arc::new(OneMob(
        handle.clone(),
    ))));
    let continuations = ContinuationOwnerService::new(
        RuntimeDeliveryInbox::new(Arc::new(InMemoryRuntimeStore::new())),
        resolver.clone(),
        Arc::new(meerkat_runtime::MeerkatMachine::ephemeral()),
    );
    let owner = ContinuationOwner::Member {
        mob_id: handle.mob_id().as_str().to_string(),
        identity: identity.as_str().to_string(),
    };

    let first_session = handle
        .resolve_bridge_session_id(&identity)
        .await
        .expect("bridge session");
    let first = handle
        .submit_continuation(&continuations, &identity, delivery("task-1"), 1)
        .await
        .expect("submit at the first incarnation");
    let first_address = LogicalRuntimeId::new(first.address.clone());
    assert_eq!(
        resolver.current_address(&owner).await.expect("current"),
        Some(first_address.clone())
    );
    assert_eq!(
        resolver
            .resolve_address(&first_address)
            .await
            .expect("resolve"),
        AddressResolution::Session(first_session.clone()),
        "the live incarnation is served by its bridge session"
    );

    handle
        .respawn(identity.clone(), None)
        .await
        .expect("respawn");
    assert_eq!(
        resolver
            .resolve_address(&first_address)
            .await
            .expect("resolve"),
        AddressResolution::Retired,
        "a respawn retires the earlier incarnation's address"
    );
    assert_eq!(
        continuations
            .continuation_status(&owner, &ContinuationKey::new("task-1").expect("key"))
            .await
            .expect("status"),
        ContinuationStatus::Stranded {
            receipt: first.clone(),
            cause: StrandedCause::OwnerRetired
        }
    );
    assert_eq!(
        handle
            .submit_continuation(&continuations, &identity, delivery("task-1"), 2)
            .await
            .expect("replay"),
        first,
        "an old key replays its first incarnation"
    );
    let second = handle
        .submit_continuation(&continuations, &identity, delivery("task-2"), 3)
        .await
        .expect("a new key binds the successor");
    assert_ne!(second.address, first.address);
    let second_session = handle
        .resolve_bridge_session_id(&identity)
        .await
        .expect("successor session");
    assert_ne!(second_session, first_session);
    assert_eq!(
        resolver
            .resolve_address(&LogicalRuntimeId::new(second.address.clone()))
            .await
            .expect("resolve"),
        AddressResolution::Session(second_session)
    );

    handle.retire(identity.clone()).await.expect("retire");
    assert_eq!(
        resolver.current_address(&owner).await.expect("current"),
        None
    );
    assert_eq!(
        handle
            .submit_continuation(&continuations, &identity, delivery("task-3"), 4)
            .await
            .expect_err("no live incarnation"),
        ContinuationSubmitError::OwnerRetired
    );
    assert_eq!(
        resolver
            .resolve_address(&LogicalRuntimeId::new(second.address))
            .await
            .expect("resolve"),
        AddressResolution::Retired
    );
}

/// A mob the host does not manage now may be registered later (a host that
/// restores mob handles after building its mob state): its members' rows
/// wait, never strand.
#[tokio::test]
async fn a_member_address_of_an_unregistered_mob_is_not_served() {
    let (handle, _service) = create_test_mob(sample_definition()).await;
    let resolver = MobContinuationResolver::new(Arc::new(OneMob(handle)));
    let address = meerkat::member_delivery_address("no-such-mob", MobIncarnationId::mint(), "w", 1)
        .expect("member address");
    assert_eq!(
        resolver.resolve_address(&address).await.expect("resolve"),
        AddressResolution::NotServed
    );
}

/// A mob destroyed and created again under its id is a new mob incarnation
/// whose generations start over, so the new member under an old member's
/// identity is at the old member's generation. The address names the mob
/// incarnation: the old member's continuation strands as retired and is
/// never served by the new member, whose own continuations bind its own
/// address.
#[tokio::test]
async fn member_continuations_never_reach_a_mob_created_again_under_its_id() {
    let (first, _first_service) = create_test_mob(sample_definition()).await;
    let identity = AgentIdentity::from("w-cont");
    first
        .spawn_spec(SpawnMemberSpec::new("worker", identity.as_str()))
        .await
        .expect("spawn");
    let mobs = Arc::new(CurrentMob(std::sync::Mutex::new(first.clone())));
    let resolver = Arc::new(MobContinuationResolver::new(mobs.clone()));
    let continuations = ContinuationOwnerService::new(
        RuntimeDeliveryInbox::new(Arc::new(InMemoryRuntimeStore::new())),
        resolver.clone(),
        Arc::new(meerkat_runtime::MeerkatMachine::ephemeral()),
    );
    let owner = ContinuationOwner::Member {
        mob_id: first.mob_id().as_str().to_string(),
        identity: identity.as_str().to_string(),
    };
    let owed = first
        .submit_continuation(&continuations, &identity, delivery("task-1"), 1)
        .await
        .expect("submit to the first mob's member");
    let owed_address = LogicalRuntimeId::new(owed.address.clone());
    let generation = first
        .roster()
        .await
        .get_by_identity(&identity)
        .expect("member")
        .generation
        .get();

    first.destroy().await.expect("destroy the first mob");
    let (second, _second_service) = create_test_mob(sample_definition()).await;
    assert_eq!(second.mob_id(), first.mob_id());
    assert_ne!(
        second.incarnation(),
        first.incarnation(),
        "a create starts a new mob incarnation"
    );
    second
        .spawn_spec(SpawnMemberSpec::new("worker", identity.as_str()))
        .await
        .expect("spawn under the same identity");
    assert_eq!(
        second
            .roster()
            .await
            .get_by_identity(&identity)
            .expect("member")
            .generation
            .get(),
        generation,
        "the new mob's generations start over"
    );
    mobs.replace(second.clone());

    assert_eq!(
        resolver
            .resolve_address(&owed_address)
            .await
            .expect("resolve"),
        AddressResolution::Retired,
        "the destroyed mob's member is never served by the new one"
    );
    assert_eq!(
        resolver
            .observe_address(&owed_address)
            .await
            .expect("observe"),
        AddressObservation::Retired
    );
    assert_eq!(
        continuations
            .continuation_status(&owner, &ContinuationKey::new("task-1").expect("key"))
            .await
            .expect("status"),
        ContinuationStatus::Stranded {
            receipt: owed.clone(),
            cause: StrandedCause::OwnerRetired
        }
    );
    let fresh = second
        .submit_continuation(&continuations, &identity, delivery("task-2"), 2)
        .await
        .expect("a new key binds the new mob's member");
    assert_ne!(fresh.address, owed.address);
    let session = second
        .resolve_bridge_session_id(&identity)
        .await
        .expect("bridge session");
    assert_eq!(
        resolver
            .resolve_address(&LogicalRuntimeId::new(fresh.address))
            .await
            .expect("resolve"),
        AddressResolution::Session(session)
    );
}

/// A fork_off or council completion that ends after its mob was destroyed
/// submits through a handle kept from that mob. The continuation names the
/// mob by id, so it must not bind the member of a mob created again under
/// the id: the handle of a destroyed mob submits nothing.
#[tokio::test]
async fn a_handle_of_a_destroyed_mob_submits_nothing_to_the_mob_created_again() {
    let (first, _first_service) = create_test_mob(sample_definition()).await;
    let identity = AgentIdentity::from("w-cont");
    first
        .spawn_spec(SpawnMemberSpec::new("worker", identity.as_str()))
        .await
        .expect("spawn");
    first.destroy().await.expect("destroy the first mob");
    let (second, _second_service) = create_test_mob(sample_definition()).await;
    second
        .spawn_spec(SpawnMemberSpec::new("worker", identity.as_str()))
        .await
        .expect("spawn under the same identity");
    let continuations = ContinuationOwnerService::new(
        RuntimeDeliveryInbox::new(Arc::new(InMemoryRuntimeStore::new())),
        Arc::new(MobContinuationResolver::new(Arc::new(OneMob(
            second.clone(),
        )))),
        Arc::new(meerkat_runtime::MeerkatMachine::ephemeral()),
    );

    assert_eq!(
        first
            .submit_continuation(&continuations, &identity, delivery("late"), 1)
            .await
            .expect_err("the first mob is destroyed"),
        ContinuationSubmitError::OwnerRetired
    );
    let owner = ContinuationOwner::Member {
        mob_id: second.mob_id().as_str().to_string(),
        identity: identity.as_str().to_string(),
    };
    assert_eq!(
        continuations
            .continuation_status(&owner, &ContinuationKey::new("late").expect("key"))
            .await
            .expect("status"),
        ContinuationStatus::NotCommitted,
        "nothing was committed for the new mob's member"
    );
}

/// An address minted before 0.9.4 names no mob incarnation, and a mob
/// created since is a later incarnation under its id: it never serves the
/// rows owed to the members of an older mob.
#[tokio::test]
async fn an_address_without_a_mob_incarnation_is_retired_in_a_mob_created_since() {
    let (handle, _service) = create_test_mob(sample_definition()).await;
    let identity = AgentIdentity::from("w-cont");
    handle
        .spawn_spec(SpawnMemberSpec::new("worker", identity.as_str()))
        .await
        .expect("spawn");
    let generation = handle
        .roster()
        .await
        .get_by_identity(&identity)
        .expect("member")
        .generation
        .get();
    let resolver = MobContinuationResolver::new(Arc::new(OneMob(handle.clone())));
    // Nothing mints the old form any more: spell it as a 0.9.3 store holds it.
    let escape = |component: &str| component.replace('%', "%25").replace(':', "%3A");
    let legacy = LogicalRuntimeId::new(format!(
        "rt:member:{}:{}:{generation}",
        escape(handle.mob_id().as_str()),
        escape(identity.as_str()),
    ));
    assert_eq!(
        legacy
            .member_address()
            .expect("an address from before 0.9.4")
            .map(|member| member.incarnation),
        Some(None),
        "it names no mob incarnation"
    );
    assert_eq!(
        resolver.resolve_address(&legacy).await.expect("resolve"),
        AddressResolution::Retired
    );
    assert_eq!(
        resolver.observe_address(&legacy).await.expect("observe"),
        AddressObservation::Retired
    );
}

/// The incarnation a mob from before 0.9.4 gets at its first resume serves
/// the addresses minted before (which name no mob incarnation) as well as
/// its own; no mob serves another incarnation's addresses.
#[tokio::test]
async fn only_a_migrated_mob_serves_addresses_without_a_mob_incarnation() {
    let service = Arc::new(MockSessionService::new());
    let _ = service.enable_runtime_adapter();
    let directory = tempfile::tempdir().expect("temporary mob directory");
    let database = directory.path().join("mob.db");
    let created = MobBuilder::new(
        sample_definition(),
        MobStorage::persistent(&database).expect("open persistent mob storage"),
    )
    .with_session_service(service)
    .allow_ephemeral_sessions(true)
    .create()
    .await
    .expect("create mob");
    assert!(
        !created
            .serves_member_address_incarnation(None)
            .await
            .expect("read the creation record"),
        "a mob created since 0.9.4 serves no address from before"
    );
    assert!(
        created
            .serves_member_address_incarnation(Some(created.incarnation()))
            .await
            .expect("read")
    );
    created.shutdown().await.expect("stop the first runtime");
    drop(created);
    // The store as a 0.9.3 binary left it: the log, and no creation record.
    rusqlite::Connection::open(&database)
        .expect("open the store")
        .execute("DELETE FROM mob_runtime_incarnations", [])
        .expect("drop the creation record");

    let migrated = resume_persistent(&database).await.expect("first resume");
    assert!(
        migrated
            .serves_member_address_incarnation(None)
            .await
            .expect("read the creation record"),
        "the migrated incarnation serves the addresses its members had"
    );
    assert!(
        migrated
            .serves_member_address_incarnation(Some(migrated.incarnation()))
            .await
            .expect("read")
    );
    assert!(
        !migrated
            .serves_member_address_incarnation(Some(MobIncarnationId::mint()))
            .await
            .expect("read")
    );
    migrated.shutdown().await.expect("stop the resume");
}
