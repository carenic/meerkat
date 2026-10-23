#![allow(clippy::expect_used, clippy::panic)]
//! Continuation submit and status: exact-key replay, conflict, first-binding
//! incarnation rules, and the store-only status overlay.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use meerkat::{
    AddressObservation, AddressResolution, ContinuationAddressResolver, ContinuationDelivery,
    ContinuationHandling, ContinuationKey, ContinuationObservation, ContinuationObserveError,
    ContinuationOwner, ContinuationOwnerService, ContinuationProducer, ContinuationResultRef,
    ContinuationStatus, ContinuationStatusError, ContinuationSubmitError, SessionAddressResolver,
    SessionId, StrandedCause, member_delivery_address,
};
use meerkat_runtime::{
    ContinuationAdmissionOutcome, ContinuationAdmissionTransition, InMemoryRuntimeStore,
    LogicalRuntimeId, RuntimeDeliveryId, RuntimeDeliveryInbox, RuntimeStore,
};

fn delivery(key: &str, text: &str) -> ContinuationDelivery {
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
        body: text.into(),
        handling: ContinuationHandling::Queue,
    }
}

fn service(
    store: Arc<dyn RuntimeStore>,
    resolver: Arc<dyn ContinuationAddressResolver>,
) -> (ContinuationOwnerService, RuntimeDeliveryInbox) {
    let inbox = RuntimeDeliveryInbox::new(store.clone());
    (
        ContinuationOwnerService::new(
            inbox.clone(),
            resolver,
            Arc::new(meerkat_runtime::MeerkatMachine::ephemeral()),
        ),
        inbox,
    )
}

#[tokio::test]
async fn a_session_continuation_replays_byte_identically_and_conflicts_only_on_new_content() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let (continuations, _) = service(store, Arc::new(SessionAddressResolver));
    let owner = ContinuationOwner::Session {
        session_id: SessionId::new(),
    };

    let receipt = continuations
        .submit(&owner, delivery("task-1", "result"), 10)
        .await
        .expect("commit");
    let replay = continuations
        .submit(&owner, delivery("task-1", "result"), 99)
        .await
        .expect("replay");
    assert_eq!(
        serde_json::to_vec(&replay).expect("encode"),
        serde_json::to_vec(&receipt).expect("encode"),
        "a replay returns the original receipt byte for byte"
    );

    let conflict = continuations
        .submit(&owner, delivery("task-1", "other result"), 11)
        .await
        .expect_err("same key, other content");
    assert_eq!(
        conflict,
        ContinuationSubmitError::Conflict {
            existing: Box::new(receipt.clone())
        }
    );
    let status = continuations
        .continuation_status(&owner, &ContinuationKey::new("task-1").expect("key"))
        .await
        .expect("status");
    assert_eq!(
        status,
        ContinuationStatus::Pending {
            receipt,
            sequence: 1,
            admitted: None
        },
        "the conflict left the original untouched"
    );
    assert_eq!(
        continuations
            .continuation_status(&owner, &ContinuationKey::new("never").expect("key"))
            .await
            .expect("status"),
        ContinuationStatus::NotCommitted
    );
}

#[cfg(feature = "sqlite-store")]
#[tokio::test]
async fn a_session_continuation_replays_after_reopening_a_sqlite_store() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("runtime.sqlite3");
    let owner = ContinuationOwner::Session {
        session_id: SessionId::new(),
    };
    let receipt = {
        let store: Arc<dyn RuntimeStore> =
            Arc::new(meerkat_runtime::SqliteRuntimeStore::new(&path).expect("open"));
        let (continuations, _) = service(store, Arc::new(SessionAddressResolver));
        continuations
            .submit(&owner, delivery("task-1", "result"), 10)
            .await
            .expect("commit")
    };
    let store: Arc<dyn RuntimeStore> =
        Arc::new(meerkat_runtime::SqliteRuntimeStore::new(&path).expect("reopen"));
    let (continuations, _) = service(store, Arc::new(SessionAddressResolver));
    assert_eq!(
        continuations
            .submit(&owner, delivery("task-1", "result"), 99)
            .await
            .expect("replay after reopen"),
        receipt
    );
}

/// A mob resolver test double: the member's current generation, which the
/// test moves to model a respawn or a retirement.
struct MemberResolver {
    generation: Mutex<Option<u64>>,
    retired_generations: Mutex<Vec<u64>>,
}

impl MemberResolver {
    fn at(generation: u64) -> Arc<Self> {
        Arc::new(Self {
            generation: Mutex::new(Some(generation)),
            retired_generations: Mutex::new(Vec::new()),
        })
    }

    fn respawn(&self, generation: u64) {
        let mut current = self.generation.lock().expect("lock");
        if let Some(previous) = current.replace(generation) {
            self.retired_generations
                .lock()
                .expect("lock")
                .push(previous);
        }
    }

    fn retire(&self) {
        if let Some(previous) = self.generation.lock().expect("lock").take() {
            self.retired_generations
                .lock()
                .expect("lock")
                .push(previous);
        }
    }
}

#[async_trait::async_trait]
impl ContinuationAddressResolver for MemberResolver {
    async fn current_address(
        &self,
        owner: &ContinuationOwner,
    ) -> Result<Option<LogicalRuntimeId>, String> {
        let ContinuationOwner::Member { mob_id, identity } = owner else {
            return Err("member resolver".into());
        };
        let Some(generation) = *self.generation.lock().expect("lock") else {
            return Ok(None);
        };
        member_delivery_address(mob_id, team_incarnation(), identity, generation)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    async fn resolve_address(
        &self,
        address: &LogicalRuntimeId,
    ) -> Result<AddressResolution, String> {
        let member = address
            .member_address()
            .map_err(|error| error.to_string())?
            .ok_or("not a member address")?;
        if self
            .retired_generations
            .lock()
            .expect("lock")
            .contains(&member.generation)
        {
            Ok(AddressResolution::Retired)
        } else {
            Ok(AddressResolution::NotServed)
        }
    }

    /// The double's records: a respawned or retired generation is retired.
    async fn observe_address(
        &self,
        address: &LogicalRuntimeId,
    ) -> Result<AddressObservation, String> {
        let member = address
            .member_address()
            .map_err(|error| error.to_string())?
            .ok_or("not a member address")?;
        Ok(
            if self
                .retired_generations
                .lock()
                .expect("lock")
                .contains(&member.generation)
            {
                AddressObservation::Retired
            } else {
                AddressObservation::MayServe
            },
        )
    }
}

#[tokio::test]
async fn a_member_key_keeps_its_first_incarnation_across_respawn_and_retirement() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let resolver = MemberResolver::at(1);
    let (continuations, _) = service(store, resolver.clone());
    let owner = ContinuationOwner::Member {
        mob_id: "team".into(),
        identity: "lead".into(),
    };

    let first = continuations
        .submit(&owner, delivery("task-1", "result"), 1)
        .await
        .expect("commit at generation 1");
    assert_eq!(
        first.address,
        member_delivery_address("team", team_incarnation(), "lead", 1)
            .expect("address")
            .0
    );

    resolver.respawn(2);
    assert_eq!(
        continuations
            .submit(&owner, delivery("task-1", "result"), 2)
            .await
            .expect("replay after respawn"),
        first,
        "an old key replays its first incarnation; it is never refreshed"
    );
    let second = continuations
        .submit(&owner, delivery("task-2", "result"), 3)
        .await
        .expect("a new key binds the new generation");
    assert_eq!(
        second.address,
        member_delivery_address("team", team_incarnation(), "lead", 2)
            .expect("address")
            .0
    );
    assert_eq!(
        continuations
            .continuation_status(&owner, &ContinuationKey::new("task-1").expect("key"))
            .await
            .expect("status"),
        ContinuationStatus::Stranded {
            receipt: first.clone(),
            cause: StrandedCause::OwnerRetired
        },
        "a never-admitted row of the retired generation is stranded"
    );

    resolver.retire();
    assert_eq!(
        continuations
            .submit(&owner, delivery("task-3", "result"), 4)
            .await
            .expect_err("no live incarnation"),
        ContinuationSubmitError::OwnerRetired
    );
    assert_eq!(
        continuations
            .submit(&owner, delivery("task-1", "result"), 5)
            .await
            .expect("replay after retirement"),
        first
    );
}

#[tokio::test]
async fn status_reports_admission_before_and_after_the_acknowledgement() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let (continuations, inbox) = service(store.clone(), Arc::new(SessionAddressResolver));
    let session_id = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session_id.clone(),
    };
    let key = ContinuationKey::new("task-1").expect("key");
    let receipt = continuations
        .submit(&owner, delivery("task-1", "result"), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    let input_id = meerkat_core::lifecycle::InputId::new();
    let transition = |transition| {
        let (store, address, delivery_id) =
            (store.clone(), address.clone(), receipt.delivery_id.clone());
        async move {
            let outcome = store
                .transition_continuation_admission(&address, &delivery_id, transition)
                .await
                .expect("admission index write");
            assert!(matches!(
                outcome,
                ContinuationAdmissionOutcome::Transitioned(_)
            ));
        }
    };
    transition(ContinuationAdmissionTransition::Reserve {
        session_id: session_id.clone(),
        input_id: input_id.clone(),
    })
    .await;
    assert_eq!(
        continuations
            .continuation_status(&owner, &key)
            .await
            .expect("status"),
        ContinuationStatus::Pending {
            receipt: receipt.clone(),
            sequence: 1,
            admitted: None
        },
        "a reservation is not an admission"
    );
    transition(ContinuationAdmissionTransition::Apply {
        session_id: session_id.clone(),
        input_id: input_id.clone(),
    })
    .await;
    assert_eq!(
        continuations
            .continuation_status(&owner, &key)
            .await
            .expect("status"),
        ContinuationStatus::Pending {
            receipt: receipt.clone(),
            sequence: 1,
            admitted: Some((session_id.clone(), input_id.clone()))
        },
        "admitted but not acknowledged is Pending, never Applied"
    );

    inbox
        .mark_applied(
            &address,
            &RuntimeDeliveryId::new(receipt.delivery_id.clone()).expect("id"),
            1,
        )
        .await
        .expect("acknowledge");
    assert_eq!(
        continuations
            .continuation_status(&owner, &key)
            .await
            .expect("status"),
        ContinuationStatus::Applied {
            receipt,
            session: session_id,
            input: input_id
        }
    );
}

/// A host whose native work authorization owner refuses everything; only its
/// installation matters here.
struct RefusingWorkAuthority;

impl meerkat_runtime::input_authority::NativeWorkAuthorizationHost for RefusingWorkAuthority {
    fn authenticate_association(
        &self,
        _runtime_id: &LogicalRuntimeId,
        _input: &meerkat_runtime::Input,
        _ingress: &meerkat_runtime::input_authority::NativeIngressContext,
        _association: &meerkat_authorization_contracts::work_association::InputAuthorityAssociation,
    ) -> Result<(), meerkat_runtime::input_authority::NativeAdmissionError> {
        Err(meerkat_core::OperationRefused::new(meerkat_core::OperationRefusalKind::Denied).into())
    }

    fn work_context(
        &self,
        _batch: &meerkat_runtime::input_authority::NativeWorkBatch,
    ) -> Result<
        meerkat_core::WorkAuthorizationContext,
        meerkat_runtime::input_authority::NativeWorkContextError,
    > {
        Err(meerkat_runtime::input_authority::NativeWorkContextError::MalformedAcceptedWork)
    }
}

/// On a runtime with a native work authorization host the caller gets a
/// local typed refusal and nothing is committed; without one the same
/// submission commits.
#[tokio::test]
async fn a_governed_runtime_refuses_the_submit_locally_and_commits_nothing() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let governed = meerkat_runtime::MeerkatMachine::ephemeral()
        .with_native_work_authorization_host(Arc::new(RefusingWorkAuthority))
        .expect("install the native work authorization host");
    let continuations = ContinuationOwnerService::new(
        RuntimeDeliveryInbox::new(store.clone()),
        Arc::new(SessionAddressResolver),
        Arc::new(governed),
    );
    let owner = ContinuationOwner::Session {
        session_id: SessionId::new(),
    };
    assert_eq!(
        continuations
            .submit(&owner, delivery("task-1", "result"), 1)
            .await
            .expect_err("governed runtime"),
        ContinuationSubmitError::NoAdmissibleWorkBinding
    );
    assert_eq!(
        continuations
            .continuation_status(&owner, &ContinuationKey::new("task-1").expect("key"))
            .await
            .expect("status"),
        ContinuationStatus::NotCommitted,
        "nothing was committed"
    );

    let (ungoverned, _) = service(store, Arc::new(SessionAddressResolver));
    ungoverned
        .submit(&owner, delivery("task-1", "result"), 2)
        .await
        .expect("an ungoverned runtime commits");
}

/// One committed fork_off job, as its owner reports it.
struct OneJob(meerkat::RetainedJobFacts);

#[async_trait::async_trait]
impl meerkat::RetainedJobSource for OneJob {
    async fn retained_job(
        &self,
        producer: &ContinuationProducer,
        job_id: &str,
    ) -> meerkat::RetainedJobLookup {
        if producer == &ContinuationProducer::ForkOff && job_id == "job-1" {
            meerkat::RetainedJobLookup::Found(self.0.clone())
        } else {
            meerkat::RetainedJobLookup::Absent
        }
    }
}

fn fork_completion(result_digest: &str) -> ContinuationDelivery {
    ContinuationDelivery {
        key: ContinuationKey::new("fork_off:job-1").expect("key"),
        result: ContinuationResultRef {
            producer: ContinuationProducer::ForkOff,
            producer_id: "job-1".into(),
            result_digest: result_digest.into(),
            summary: None,
        },
        body: "the fork finished".into(),
        handling: ContinuationHandling::Queue,
    }
}

fn retained_identity() -> meerkat_core::retained_work::RetainedWorkIdentity {
    meerkat_core::retained_work::RetainedWorkIdentity::new(
        "rt:session:fork-owner",
        meerkat_core::lifecycle::RunId::new(),
        Vec::new(),
        std::collections::BTreeMap::new(),
        None,
    )
}

/// A retained completion commits only the job's exact outcome for the job's
/// owner session; a replay returns the original receipt.
#[tokio::test]
async fn a_retained_completion_commits_only_its_jobs_exact_outcome() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let (continuations, _) = service(store, Arc::new(SessionAddressResolver));
    let session_id = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session_id.clone(),
    };
    let jobs = OneJob(meerkat::RetainedJobFacts {
        owner_session_id: session_id.clone(),
        retained_work: Some(retained_identity()),
        result_digest: "sha256:result".into(),
    });
    let record =
        meerkat::RetainedJobRecord::from_owner(&jobs, ContinuationProducer::ForkOff, "job-1")
            .await
            .expect("read")
            .expect("committed job");
    assert!(
        meerkat::RetainedJobRecord::from_owner(&jobs, ContinuationProducer::ForkOff, "job-2")
            .await
            .expect("read")
            .is_none(),
        "no record for a job the owner does not hold"
    );

    assert!(matches!(
        continuations
            .submit_retained_completion(&owner, fork_completion("sha256:another"), &record, 1)
            .await,
        Err(ContinuationSubmitError::Invalid(_))
    ));
    assert!(matches!(
        continuations
            .submit_retained_completion(
                &ContinuationOwner::Session {
                    session_id: SessionId::new()
                },
                fork_completion("sha256:result"),
                &record,
                1
            )
            .await,
        Err(ContinuationSubmitError::Invalid(_))
    ));
    let receipt = continuations
        .submit_retained_completion(&owner, fork_completion("sha256:result"), &record, 1)
        .await
        .expect("the job's exact outcome commits");
    assert_eq!(
        continuations
            .submit_retained_completion(&owner, fork_completion("sha256:result"), &record, 9)
            .await
            .expect("replay"),
        receipt
    );
}

/// On a governed runtime only a job that retained its dispatching run's
/// work can commit a completion; the public submit never can.
#[tokio::test]
async fn a_governed_runtime_commits_only_retained_completions() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let governed = meerkat_runtime::MeerkatMachine::ephemeral()
        .with_native_work_authorization_host(Arc::new(RefusingWorkAuthority))
        .expect("install the native work authorization host");
    let continuations = ContinuationOwnerService::new(
        RuntimeDeliveryInbox::new(store),
        Arc::new(SessionAddressResolver),
        Arc::new(governed),
    );
    let session_id = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session_id.clone(),
    };
    let legacy = OneJob(meerkat::RetainedJobFacts {
        owner_session_id: session_id.clone(),
        retained_work: None,
        result_digest: "sha256:result".into(),
    });
    let legacy =
        meerkat::RetainedJobRecord::from_owner(&legacy, ContinuationProducer::ForkOff, "job-1")
            .await
            .expect("read")
            .expect("committed job");
    assert_eq!(
        continuations
            .submit_retained_completion(&owner, fork_completion("sha256:result"), &legacy, 1)
            .await
            .expect_err("no retained work"),
        ContinuationSubmitError::NoAdmissibleWorkBinding
    );
    let retained = OneJob(meerkat::RetainedJobFacts {
        owner_session_id: session_id.clone(),
        retained_work: Some(retained_identity()),
        result_digest: "sha256:result".into(),
    });
    let retained =
        meerkat::RetainedJobRecord::from_owner(&retained, ContinuationProducer::ForkOff, "job-1")
            .await
            .expect("read")
            .expect("committed job");
    continuations
        .submit_retained_completion(&owner, fork_completion("sha256:result"), &retained, 1)
        .await
        .expect("a retained completion commits on a governed runtime");
    assert_eq!(
        continuations
            .submit(&owner, fork_completion("sha256:result"), 2)
            .await
            .expect_err("the public submit carries no binding"),
        ContinuationSubmitError::NoAdmissibleWorkBinding
    );
}

/// A session resolver whose observation the test sets, counting the
/// resolutions (the serving) asked of it.
struct ObservingResolver {
    observation: Mutex<Result<AddressObservation, String>>,
    resolutions: AtomicUsize,
}

#[async_trait::async_trait]
impl ContinuationAddressResolver for ObservingResolver {
    async fn current_address(
        &self,
        owner: &ContinuationOwner,
    ) -> Result<Option<LogicalRuntimeId>, String> {
        SessionAddressResolver.current_address(owner).await
    }

    async fn resolve_address(
        &self,
        address: &LogicalRuntimeId,
    ) -> Result<AddressResolution, String> {
        self.resolutions.fetch_add(1, Ordering::SeqCst);
        SessionAddressResolver.resolve_address(address).await
    }

    async fn observe_address(
        &self,
        _address: &LogicalRuntimeId,
    ) -> Result<AddressObservation, String> {
        self.observation.lock().expect("lock").clone()
    }
}

/// A status read observes the owner and never serves it. A row not yet
/// admitted reads pending while its owner is not proven retired, stranded
/// once its owner's records prove it retired, and unavailable, typed, when
/// those records cannot be read; the read-only retained observation answers
/// the same. No read asks the resolver to resolve (serve) the address.
#[tokio::test]
async fn a_status_read_observes_the_owner_and_never_serves_it() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let resolver = Arc::new(ObservingResolver {
        observation: Mutex::new(Ok(AddressObservation::MayServe)),
        resolutions: AtomicUsize::new(0),
    });
    let (continuations, _) = service(store, resolver.clone());
    let session_id = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session_id.clone(),
    };
    let jobs = OneJob(meerkat::RetainedJobFacts {
        owner_session_id: session_id,
        retained_work: Some(retained_identity()),
        result_digest: "sha256:result".into(),
    });
    let record =
        meerkat::RetainedJobRecord::from_owner(&jobs, ContinuationProducer::ForkOff, "job-1")
            .await
            .expect("read")
            .expect("committed job");
    let delivery = fork_completion("sha256:result");
    let receipt = continuations
        .submit_retained_completion(&owner, delivery.clone(), &record, 1)
        .await
        .expect("commit");
    let observe = || continuations.observe_retained_completion(&owner, &delivery, &record);

    let pending = ContinuationStatus::Pending {
        receipt: receipt.clone(),
        sequence: 1,
        admitted: None,
    };
    assert_eq!(
        continuations
            .continuation_status(&owner, &delivery.key)
            .await
            .expect("status"),
        pending,
        "an owner not proven retired may still be served"
    );
    assert_eq!(
        observe().await.expect("observe"),
        ContinuationObservation::Matched {
            receipt: receipt.clone(),
            status: pending,
        }
    );

    *resolver.observation.lock().expect("lock") = Ok(AddressObservation::Retired);
    let stranded = ContinuationStatus::Stranded {
        receipt: receipt.clone(),
        cause: StrandedCause::OwnerRetired,
    };
    assert_eq!(
        continuations
            .continuation_status(&owner, &delivery.key)
            .await
            .expect("status"),
        stranded,
        "a proven retirement strands the row"
    );
    assert_eq!(
        observe().await.expect("observe"),
        ContinuationObservation::Matched {
            receipt,
            status: stranded,
        }
    );

    *resolver.observation.lock().expect("lock") = Err("the owner records are unreadable".into());
    assert_eq!(
        continuations
            .continuation_status(&owner, &delivery.key)
            .await,
        Err(ContinuationStatusError::Unavailable(
            "the owner records are unreadable".into()
        ))
    );
    assert_eq!(
        observe().await,
        Err(ContinuationObserveError::Unavailable(
            "the owner records are unreadable".into()
        ))
    );
    assert_eq!(
        resolver.resolutions.load(Ordering::SeqCst),
        0,
        "no read asked the resolver to serve the owner"
    );
}

/// The incarnation of the mob `team` these tests address members of.
fn team_incarnation() -> meerkat_core::service::MobIncarnationId {
    "6a1d3c58-2f0e-4b7a-9c41-8e5d2b0f7a13"
        .parse()
        .expect("canonical mob incarnation id")
}

/// Two incarnations of the mob `team`: the one a stopped mob ran at, and the
/// one of a mob the host created again under the id.
fn earlier_incarnation() -> meerkat_core::service::MobIncarnationId {
    "1f0c6a9e-4b2d-4c3e-8a71-5d9b2e4f6a10"
        .parse()
        .expect("canonical mob incarnation id")
}

fn current_incarnation() -> meerkat_core::service::MobIncarnationId {
    "8c2e4a61-7d3f-4e59-b1a2-6f0d9c3b5e27"
        .parse()
        .expect("canonical mob incarnation id")
}

fn later_incarnation() -> meerkat_core::service::MobIncarnationId {
    "3d7b9f15-2a6c-4f80-9e14-c5a8b0d2e963"
        .parse()
        .expect("canonical mob incarnation id")
}

fn incarnation_of(address: &str) -> Option<meerkat_core::service::MobIncarnationId> {
    LogicalRuntimeId::new(address.to_string())
        .member_address()
        .expect("member address")
        .expect("a member address")
        .incarnation
}

/// A concurrent submit through a second service, run just before the
/// resolver answers: it binds the key between the commit's ledger read and
/// its claim. `then` is what the resolver answers afterwards.
struct RacingSubmit {
    racer: ContinuationOwnerService,
    delivery: ContinuationDelivery,
    then: Option<meerkat_core::service::MobIncarnationId>,
}

/// A mob resolver test double for incarnations: the member's current address
/// names the incarnation of the mob the host holds under the id, which the
/// test moves as a host does when it holds a mob created again. Armed with a
/// [`RacingSubmit`], its next answer first lets the racer bind the key.
struct IncarnationResolver {
    current: Mutex<Option<meerkat_core::service::MobIncarnationId>>,
    race: tokio::sync::Mutex<Option<RacingSubmit>>,
}

impl IncarnationResolver {
    fn at(incarnation: meerkat_core::service::MobIncarnationId) -> Arc<Self> {
        Arc::new(Self {
            current: Mutex::new(Some(incarnation)),
            race: tokio::sync::Mutex::new(None),
        })
    }

    fn hold(&self, incarnation: meerkat_core::service::MobIncarnationId) {
        *self.current.lock().expect("lock") = Some(incarnation);
    }
}

#[async_trait::async_trait]
impl ContinuationAddressResolver for IncarnationResolver {
    async fn current_address(
        &self,
        owner: &ContinuationOwner,
    ) -> Result<Option<LogicalRuntimeId>, String> {
        let ContinuationOwner::Member { mob_id, identity } = owner else {
            return Err("member resolver".into());
        };
        let answer = *self.current.lock().expect("lock");
        let race = self.race.lock().await.take();
        if let Some(race) = race {
            race.racer
                .submit(owner, race.delivery, 1)
                .await
                .expect("the racer binds the key");
            *self.current.lock().expect("lock") = race.then;
        }
        let Some(incarnation) = answer else {
            return Ok(None);
        };
        member_delivery_address(mob_id, incarnation, identity, 1)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    async fn resolve_address(
        &self,
        _address: &LogicalRuntimeId,
    ) -> Result<AddressResolution, String> {
        Ok(AddressResolution::NotServed)
    }

    async fn observe_address(
        &self,
        _address: &LogicalRuntimeId,
    ) -> Result<AddressObservation, String> {
        Ok(AddressObservation::MayServe)
    }
}

fn team_lead() -> ContinuationOwner {
    ContinuationOwner::Member {
        mob_id: "team".into(),
        identity: "lead".into(),
    }
}

/// A member commit bound to a mob incarnation binds a new key only while the
/// owner's current address names that incarnation: a caller of an earlier
/// incarnation commits nothing for the mob created again under the id.
#[tokio::test]
async fn a_member_commit_binds_a_new_key_only_at_its_own_incarnation() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let resolver = IncarnationResolver::at(current_incarnation());
    let (continuations, _) = service(store, resolver);
    let owner = team_lead();

    assert_eq!(
        continuations
            .submit_for_member_incarnation(
                &owner,
                earlier_incarnation(),
                delivery("task-1", "result"),
                1,
            )
            .await,
        Err(ContinuationSubmitError::OwnerRetired),
        "a caller of the earlier incarnation binds nothing"
    );
    assert_eq!(
        continuations
            .continuation_status(&owner, &ContinuationKey::new("task-1").expect("key"))
            .await
            .expect("status"),
        ContinuationStatus::NotCommitted
    );
    let receipt = continuations
        .submit_for_member_incarnation(
            &owner,
            current_incarnation(),
            delivery("task-1", "result"),
            2,
        )
        .await
        .expect("the current incarnation binds the key");
    assert_eq!(
        incarnation_of(&receipt.address),
        Some(current_incarnation())
    );
}

/// A key already bound, read from the ledger before the owner is resolved:
/// bound at the caller's incarnation it replays; bound at another one it
/// replays only for a caller of the current incarnation (a key an earlier
/// incarnation bound, reused), and is `OwnerRetired` for a stale caller.
#[tokio::test]
async fn a_bound_key_replays_for_its_own_or_the_current_incarnation_only() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let resolver = IncarnationResolver::at(earlier_incarnation());
    let (continuations, _) = service(store, resolver.clone());
    let owner = team_lead();
    let earlier = continuations
        .submit_for_member_incarnation(
            &owner,
            earlier_incarnation(),
            delivery("task-1", "result"),
            1,
        )
        .await
        .expect("bind at the earlier incarnation");
    resolver.hold(current_incarnation());

    assert_eq!(
        continuations
            .submit_for_member_incarnation(
                &owner,
                earlier_incarnation(),
                delivery("task-1", "result"),
                2,
            )
            .await
            .expect("a key bound at the caller's incarnation replays"),
        earlier
    );
    assert_eq!(
        continuations
            .submit_for_member_incarnation(
                &owner,
                current_incarnation(),
                delivery("task-1", "result"),
                3,
            )
            .await
            .expect("the current incarnation reusing the key replays it, as submit does"),
        earlier
    );
    let current = continuations
        .submit_for_member_incarnation(
            &owner,
            current_incarnation(),
            delivery("task-2", "result"),
            4,
        )
        .await
        .expect("bind at the current incarnation");
    assert_eq!(
        continuations
            .submit_for_member_incarnation(
                &owner,
                earlier_incarnation(),
                delivery("task-2", "result"),
                5,
            )
            .await,
        Err(ContinuationSubmitError::OwnerRetired),
        "a stale caller does not replay the current incarnation's receipt"
    );
    assert_eq!(
        incarnation_of(&current.address),
        Some(current_incarnation())
    );
}

/// A key a concurrent submit binds between the commit's ledger read and its
/// claim follows the same rule: replayed while the owner's current address
/// names the caller's incarnation, `OwnerRetired` once it names another.
#[tokio::test]
async fn a_key_bound_by_a_racing_submit_follows_the_same_rule() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let (racer, _) = service(
        store.clone(),
        IncarnationResolver::at(earlier_incarnation()),
    );
    let resolver = IncarnationResolver::at(current_incarnation());
    let (continuations, _) = service(store.clone(), resolver.clone());
    let owner = team_lead();

    *resolver.race.lock().await = Some(RacingSubmit {
        racer,
        delivery: delivery("task-1", "result"),
        then: Some(current_incarnation()),
    });
    let replayed = continuations
        .submit_for_member_incarnation(
            &owner,
            current_incarnation(),
            delivery("task-1", "result"),
            2,
        )
        .await
        .expect("the current incarnation replays the racer's binding");
    assert_eq!(
        incarnation_of(&replayed.address),
        Some(earlier_incarnation()),
        "the replay is the racer's first receipt"
    );

    let (racer, _) = service(store, IncarnationResolver::at(earlier_incarnation()));
    *resolver.race.lock().await = Some(RacingSubmit {
        racer,
        delivery: delivery("task-2", "result"),
        then: Some(later_incarnation()),
    });
    assert_eq!(
        continuations
            .submit_for_member_incarnation(
                &owner,
                current_incarnation(),
                delivery("task-2", "result"),
                3,
            )
            .await,
        Err(ContinuationSubmitError::OwnerRetired),
        "once the host holds yet another incarnation, the caller is stale"
    );
}

/// A session owner has no mob incarnation: binding a commit to one leaves it
/// unchanged, exactly a plain submit.
#[tokio::test]
async fn a_session_owner_is_unchanged_by_a_member_incarnation() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let (continuations, _) = service(store, Arc::new(SessionAddressResolver));
    let owner = ContinuationOwner::Session {
        session_id: SessionId::new(),
    };
    let receipt = continuations
        .submit_for_member_incarnation(
            &owner,
            earlier_incarnation(),
            delivery("task-1", "result"),
            1,
        )
        .await
        .expect("a session owner commits");
    assert_eq!(
        continuations
            .submit(&owner, delivery("task-1", "result"), 2)
            .await
            .expect("replay"),
        receipt
    );
}

/// A retained completion bound to a mob incarnation follows the same rule
/// as a submission: a caller of an earlier incarnation commits nothing.
#[tokio::test]
async fn a_retained_completion_binds_only_at_its_own_incarnation() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let (continuations, _) = service(store, IncarnationResolver::at(current_incarnation()));
    let owner = team_lead();
    let jobs = OneJob(meerkat::RetainedJobFacts {
        owner_session_id: SessionId::new(),
        retained_work: Some(retained_identity()),
        result_digest: "sha256:result".into(),
    });
    let record =
        meerkat::RetainedJobRecord::from_owner(&jobs, ContinuationProducer::ForkOff, "job-1")
            .await
            .expect("read")
            .expect("committed job");

    assert_eq!(
        continuations
            .submit_retained_completion_for_member_incarnation(
                &owner,
                earlier_incarnation(),
                fork_completion("sha256:result"),
                &record,
                1,
            )
            .await,
        Err(ContinuationSubmitError::OwnerRetired)
    );
    assert_eq!(
        continuations
            .continuation_status(
                &owner,
                &ContinuationKey::new("fork_off:job-1").expect("key")
            )
            .await
            .expect("status"),
        ContinuationStatus::NotCommitted
    );
    let receipt = continuations
        .submit_retained_completion_for_member_incarnation(
            &owner,
            current_incarnation(),
            fork_completion("sha256:result"),
            &record,
            2,
        )
        .await
        .expect("the current incarnation commits the job's outcome");
    assert_eq!(
        incarnation_of(&receipt.address),
        Some(current_incarnation())
    );
}
