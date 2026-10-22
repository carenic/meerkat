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

// ---------------------------------------------------------------------------
// Host-originated continuations on a governed runtime: the submit side.
//
// The rows below assert behaviour only. How a host continuation is committed
// from its namespace's committed owner, and how that owner holds it, lives in
// `host_continuation_shape`, the one place that changes when the governed
// admission shape lands.
// ---------------------------------------------------------------------------

const INGEST_NAMESPACE: &str = "app.ingest";
const OBSERVATION: &str = "observation-1";
const OBSERVATION_DIGEST: &str = "sha256:observation-1";

/// One external observation under its host namespace, keyed `key`.
fn observation(key: &str, result_digest: &str) -> ContinuationDelivery {
    ContinuationDelivery {
        key: ContinuationKey::new(key).expect("key"),
        result: ContinuationResultRef {
            producer: ContinuationProducer::Host {
                namespace: INGEST_NAMESPACE.into(),
            },
            producer_id: OBSERVATION.into(),
            result_digest: result_digest.into(),
            summary: None,
        },
        body: "a new item arrived".into(),
        handling: ContinuationHandling::Queue,
    }
}

/// A continuation service whose runtime has a native work authorization
/// host installed.
fn governed_service(store: Arc<dyn RuntimeStore>) -> ContinuationOwnerService {
    let governed = meerkat_runtime::MeerkatMachine::ephemeral()
        .with_native_work_authorization_host(Arc::new(RefusingWorkAuthority))
        .expect("install the native work authorization host");
    ContinuationOwnerService::new(
        RuntimeDeliveryInbox::new(store),
        Arc::new(SessionAddressResolver),
        Arc::new(governed),
    )
}

async fn status_of(
    continuations: &ContinuationOwnerService,
    owner: &ContinuationOwner,
    key: &str,
) -> ContinuationStatus {
    continuations
        .continuation_status(owner, &ContinuationKey::new(key).expect("key"))
        .await
        .expect("status")
}

/// On a governed runtime a host continuation commits when its namespace's
/// committed owner confirms it, and a replay returns the original receipt
/// byte for byte. The public submit keeps refusing every host continuation:
/// the namespace owner's confirmation is the only way in.
#[tokio::test]
async fn a_governed_runtime_commits_a_host_continuation_its_namespace_owner_confirms() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let continuations = governed_service(store);
    let session_id = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session_id.clone(),
    };
    let source = host_continuation_shape::NamespaceOwner::holding(
        INGEST_NAMESPACE,
        &session_id,
        OBSERVATION_DIGEST,
    );
    let receipt = host_continuation_shape::commit(
        &continuations,
        &owner,
        observation("ingest:1", OBSERVATION_DIGEST),
        &source,
    )
    .await
    .expect("a confirmed host continuation commits on a governed runtime");
    assert_eq!(
        host_continuation_shape::commit(
            &continuations,
            &owner,
            observation("ingest:1", OBSERVATION_DIGEST),
            &source,
        )
        .await
        .expect("replay"),
        receipt,
        "a replay returns the original receipt"
    );
    assert_eq!(
        status_of(&continuations, &owner, "ingest:1").await,
        ContinuationStatus::Pending {
            receipt,
            sequence: 1,
            admitted: None
        }
    );
    assert_eq!(
        continuations
            .submit(&owner, observation("ingest:2", OBSERVATION_DIGEST), 1)
            .await
            .expect_err("the public submit carries no confirmation"),
        ContinuationSubmitError::NoAdmissibleWorkBinding
    );
    assert_eq!(
        status_of(&continuations, &owner, "ingest:2").await,
        ContinuationStatus::NotCommitted
    );
}

/// On a governed runtime a host continuation its namespace's committed owner
/// does not confirm commits nothing, refused with the existing typed
/// reasons: no owner holds it (`NoAdmissibleWorkBinding`), or the delivery
/// names other content or another owner than the one it was committed for
/// (`Invalid`).
#[tokio::test]
async fn a_governed_runtime_commits_no_host_continuation_its_namespace_owner_does_not_confirm() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let continuations = governed_service(store);
    let session_id = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session_id.clone(),
    };
    let other_namespace = host_continuation_shape::NamespaceOwner::holding(
        "app.other",
        &session_id,
        OBSERVATION_DIGEST,
    );
    assert_eq!(
        host_continuation_shape::commit(
            &continuations,
            &owner,
            observation("ingest:1", OBSERVATION_DIGEST),
            &other_namespace,
        )
        .await
        .expect_err("an owner of another namespace confirms nothing"),
        ContinuationSubmitError::NoAdmissibleWorkBinding
    );
    let forgotten = host_continuation_shape::NamespaceOwner::holding(
        INGEST_NAMESPACE,
        &session_id,
        OBSERVATION_DIGEST,
    );
    forgotten.forget();
    assert_eq!(
        host_continuation_shape::commit(
            &continuations,
            &owner,
            observation("ingest:1", OBSERVATION_DIGEST),
            &forgotten,
        )
        .await
        .expect_err("an owner that holds it no more confirms nothing"),
        ContinuationSubmitError::NoAdmissibleWorkBinding
    );
    let source = host_continuation_shape::NamespaceOwner::holding(
        INGEST_NAMESPACE,
        &session_id,
        OBSERVATION_DIGEST,
    );
    assert!(
        matches!(
            host_continuation_shape::commit(
                &continuations,
                &owner,
                observation("ingest:1", "sha256:another-observation"),
                &source,
            )
            .await,
            Err(ContinuationSubmitError::Invalid(_))
        ),
        "other content than the owner committed"
    );
    assert!(
        matches!(
            host_continuation_shape::commit(
                &continuations,
                &ContinuationOwner::Session {
                    session_id: SessionId::new()
                },
                observation("ingest:1", OBSERVATION_DIGEST),
                &source,
            )
            .await,
            Err(ContinuationSubmitError::Invalid(_))
        ),
        "owed to another owner than the owner committed it for"
    );
    assert_eq!(
        status_of(&continuations, &owner, "ingest:1").await,
        ContinuationStatus::NotCommitted,
        "nothing was committed"
    );
}

/// Without a native work authorization host nothing changes: a host
/// continuation commits plainly or from its namespace owner, and each replay
/// returns its original receipt.
#[tokio::test]
async fn an_ungoverned_runtime_commits_host_continuations_as_before() {
    let store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let (continuations, _) = service(store, Arc::new(SessionAddressResolver));
    let session_id = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session_id.clone(),
    };
    let plain = continuations
        .submit(&owner, observation("ingest:plain", OBSERVATION_DIGEST), 1)
        .await
        .expect("a plain host continuation commits");
    assert_eq!(
        continuations
            .submit(&owner, observation("ingest:plain", OBSERVATION_DIGEST), 2)
            .await
            .expect("replay"),
        plain
    );
    let source = host_continuation_shape::NamespaceOwner::holding(
        INGEST_NAMESPACE,
        &session_id,
        OBSERVATION_DIGEST,
    );
    let owned = host_continuation_shape::commit(
        &continuations,
        &owner,
        observation("ingest:owned", OBSERVATION_DIGEST),
        &source,
    )
    .await
    .expect("a host continuation commits from its namespace owner");
    assert_eq!(
        host_continuation_shape::commit(
            &continuations,
            &owner,
            observation("ingest:owned", OBSERVATION_DIGEST),
            &source,
        )
        .await
        .expect("replay"),
        owned
    );
    assert_ne!(plain.delivery_id, owned.delivery_id);
}

/// The only code of the host continuation rows that depends on the governed
/// admission shape: how a host continuation is committed from its
/// namespace's committed owner, and how that owner holds it. The rows above
/// assert behaviour only and do not change when the shape lands.
///
/// At this base a governed runtime has no admission for a host continuation:
/// it is committed from its owner's record as a retained completion that
/// retains no work, which a governed runtime refuses.
mod host_continuation_shape {
    use super::*;

    /// The committed owner of one host namespace, holding at most the one
    /// observation of these rows; a test edits it.
    pub(super) struct NamespaceOwner {
        namespace: String,
        facts: Mutex<Option<meerkat::RetainedJobFacts>>,
    }

    impl NamespaceOwner {
        /// An owner of `namespace` holding the observation owed to
        /// `session`, with the result `result_digest`.
        pub(super) fn holding(namespace: &str, session: &SessionId, result_digest: &str) -> Self {
            Self {
                namespace: namespace.into(),
                facts: Mutex::new(Some(meerkat::RetainedJobFacts {
                    owner_session_id: session.clone(),
                    retained_work: None,
                    result_digest: result_digest.into(),
                })),
            }
        }

        /// The owner holds the observation no more.
        pub(super) fn forget(&self) {
            *self.facts.lock().expect("lock") = None;
        }
    }

    #[async_trait::async_trait]
    impl meerkat::RetainedJobSource for NamespaceOwner {
        async fn retained_job(
            &self,
            producer: &ContinuationProducer,
            job_id: &str,
        ) -> meerkat::RetainedJobLookup {
            match producer {
                ContinuationProducer::Host { namespace }
                    if *namespace == self.namespace && job_id == OBSERVATION =>
                {
                    self.facts
                        .lock()
                        .expect("lock")
                        .clone()
                        .map_or(meerkat::RetainedJobLookup::Absent, |facts| {
                            meerkat::RetainedJobLookup::Found(facts)
                        })
                }
                _ => meerkat::RetainedJobLookup::Absent,
            }
        }
    }

    /// Commit `delivery` for `owner` from its namespace owner's record.
    pub(super) async fn commit(
        continuations: &ContinuationOwnerService,
        owner: &ContinuationOwner,
        delivery: ContinuationDelivery,
        source: &NamespaceOwner,
    ) -> Result<meerkat::ContinuationReceipt, ContinuationSubmitError> {
        let record = meerkat::RetainedJobRecord::from_owner(
            source,
            delivery.result.producer.clone(),
            delivery.result.producer_id.clone(),
        )
        .await
        .map_err(ContinuationSubmitError::Unavailable)?
        .ok_or(ContinuationSubmitError::NoAdmissibleWorkBinding)?;
        continuations
            .submit_retained_completion(owner, delivery, &record, 1)
            .await
    }
}
