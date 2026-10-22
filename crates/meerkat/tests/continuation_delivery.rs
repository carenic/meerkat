#![allow(clippy::expect_used, clippy::panic)]
//! Continuation rows through the library delivery owner: admission exactly
//! once, the reserve-first crash-cut matrix on the memory and SQLite stores
//! (admitted before the acknowledgement, reserved before admission, a repoint
//! in between), per-producer admission keys, unserved addresses, and hosts
//! that do not admit continuations.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meerkat::{
    AddressResolution, ContinuationDelivery, ContinuationDeliverySink, ContinuationHandling,
    ContinuationKey, ContinuationOwner, ContinuationOwnerService, ContinuationProducer,
    ContinuationResultRef, ContinuationStatus, JobDeliveryApplication, JobDeliverySink,
    MemoryDetachedJobStore, RuntimeDeliveryHost, RuntimeDeliveryOwner, RuntimeDeliveryPass,
    SessionAddressResolver, SessionId,
};
use meerkat::{
    ContinuationAdmitError, RetainedJobFacts, RetainedJobLookup, RetainedJobRecord,
    RetainedJobSource,
};
use meerkat_core::lifecycle::InputId;
use meerkat_core::retained_work::{RetainedContributorRef, RetainedWorkIdentity};
use meerkat_runtime::input_state::StoredInputState;
use meerkat_runtime::{
    ContinuationAdmission, ContinuationAdmissionOutcome, ContinuationAdmissionTransition,
    InMemoryRuntimeStore, LogicalRuntimeId, RuntimeDeliveryId, RuntimeDeliveryInbox, RuntimeStore,
};
use tokio::sync::watch;

const EVENT_GUARD: Duration = Duration::from_secs(10);

struct NoJobs;

#[async_trait::async_trait]
impl JobDeliverySink for NoJobs {
    async fn apply(
        &self,
        _application: JobDeliveryApplication,
    ) -> Result<(), meerkat::JobDeliveryApplyError> {
        Ok(())
    }
}

/// Records admissions per session; `known` stands in for each session's
/// input ledger, rows keyed by admission key.
#[derive(Default)]
struct RecordingSink {
    admitted: Mutex<Vec<(SessionId, String, InputId)>>,
    /// The inputs admitted, exactly as the owner built them.
    inputs: Mutex<Vec<meerkat_runtime::Input>>,
    known: Mutex<HashMap<(SessionId, String), StoredInputState>>,
    /// Stands in for a runtime with a native work authorization host.
    governed: bool,
    /// Governed resumes admitted, with the retained work each resumed.
    resumed: Mutex<Vec<(SessionId, InputId, RetainedWorkIdentity)>>,
    /// Forces the governed admission's verdict.
    resume_verdict: Mutex<Option<ContinuationAdmitError>>,
}

impl RecordingSink {
    fn admissions(&self) -> Vec<(SessionId, String, InputId)> {
        self.admitted.lock().expect("lock").clone()
    }

    fn resumes(&self) -> Vec<(SessionId, InputId, RetainedWorkIdentity)> {
        self.resumed.lock().expect("lock").clone()
    }

    fn governed() -> Self {
        Self {
            governed: true,
            ..Self::default()
        }
    }

    /// `session` already holds `input`, as its native admission records it.
    fn already_holds(&self, session: &SessionId, input: &meerkat_runtime::Input) {
        let key = input
            .header()
            .idempotency_key
            .as_ref()
            .expect("keyed")
            .to_string();
        self.known.lock().expect("lock").insert(
            (session.clone(), key),
            StoredInputState::accepted_for_test(input).expect("row"),
        );
    }

    /// `session` holds a row under `key` recorded without any replay
    /// identity (an older admission).
    fn holds_legacy_row(&self, session: &SessionId, key: &str) {
        let mut row = StoredInputState::new_accepted(InputId::new());
        row.state.idempotency_key = Some(meerkat_runtime::IdempotencyKey::new(key));
        self.known
            .lock()
            .expect("lock")
            .insert((session.clone(), key.to_string()), row);
    }
}

/// The exact input the owner admits for a delivery with `body`.
fn continuation_input(
    input_id: InputId,
    admission_key: &str,
    body: &str,
) -> meerkat_runtime::Input {
    meerkat_runtime::Input::Prompt(meerkat_runtime::PromptInput::continuation(
        input_id,
        admission_key,
        body.into(),
        meerkat_core::types::HandlingMode::Queue,
    ))
}

#[async_trait::async_trait]
impl ContinuationDeliverySink for RecordingSink {
    async fn admit(
        &self,
        session: &SessionId,
        input: meerkat_runtime::Input,
    ) -> Result<InputId, String> {
        assert!(
            !self.governed,
            "a governed runtime never receives an unbound continuation"
        );
        let header = input.header();
        assert!(
            header.authority_association.is_none() && header.ingress_context.is_none(),
            "a continuation carries no authority of its own"
        );
        let admission_key = header
            .idempotency_key
            .as_ref()
            .expect("continuations are keyed")
            .to_string();
        let input_id = input.id().clone();
        self.admitted.lock().expect("lock").push((
            session.clone(),
            admission_key,
            input_id.clone(),
        ));
        self.inputs.lock().expect("lock").push(input.clone());
        self.already_holds(session, &input);
        Ok(input_id)
    }

    async fn admitted_input(
        &self,
        session: &SessionId,
        admission_key: &str,
    ) -> Result<Option<StoredInputState>, String> {
        Ok(self
            .known
            .lock()
            .expect("lock")
            .get(&(session.clone(), admission_key.to_string()))
            .cloned())
    }

    fn governs_work_authority(&self) -> bool {
        self.governed
    }

    async fn admit_retained(
        &self,
        session: &SessionId,
        input: meerkat_runtime::Input,
        request: meerkat_runtime::retained_work::RetainedResumeRequest,
    ) -> Result<InputId, ContinuationAdmitError> {
        assert!(
            self.governed,
            "only a governed runtime resumes retained work"
        );
        if let Some(verdict) = self.resume_verdict.lock().expect("lock").clone() {
            return Err(verdict);
        }
        let header = input.header();
        assert!(
            header.authority_association.is_none() && header.ingress_context.is_none(),
            "a resume carries no authority of its own"
        );
        let key = header.idempotency_key.as_ref().expect("keyed").to_string();
        let mut row = StoredInputState::accepted_for_test(&input).expect("row");
        row.state.retained_resume = Some(meerkat_runtime::retained_work::RetainedResumeRecord {
            identity: request.identity().clone(),
            delivery: request.delivery().clone(),
        });
        self.known
            .lock()
            .expect("lock")
            .insert((session.clone(), key), row);
        self.resumed.lock().expect("lock").push((
            session.clone(),
            input.id().clone(),
            request.identity().clone(),
        ));
        Ok(input.id().clone())
    }
}

/// A host whose address resolution the test moves (a repoint, an unserved
/// member), with an optional continuation sink.
struct Host {
    sink: Option<Arc<RecordingSink>>,
    resolutions: Mutex<HashMap<LogicalRuntimeId, AddressResolution>>,
    job_source: Option<Arc<FakeJobs>>,
}

impl Host {
    fn new(sink: Option<Arc<RecordingSink>>) -> Arc<Self> {
        Arc::new(Self {
            sink,
            resolutions: Mutex::new(HashMap::new()),
            job_source: None,
        })
    }

    fn with_jobs(sink: Arc<RecordingSink>, jobs: Arc<FakeJobs>) -> Arc<Self> {
        Arc::new(Self {
            sink: Some(sink),
            resolutions: Mutex::new(HashMap::new()),
            job_source: Some(jobs),
        })
    }

    fn resolve(&self, address: &LogicalRuntimeId, resolution: AddressResolution) {
        self.resolutions
            .lock()
            .expect("lock")
            .insert(address.clone(), resolution);
    }
}

#[async_trait::async_trait]
impl RuntimeDeliveryHost for Host {
    async fn delivery_route(&self, _session_id: &SessionId) -> Option<meerkat::DeliveryRoute> {
        Some(meerkat::DeliveryRoute::ServedHere(Arc::new(NoJobs)))
    }

    async fn claim_cold_delivery(
        &self,
        session_id: &SessionId,
    ) -> Option<Result<meerkat_runtime::HostingClaim, meerkat_runtime::HostingRefused>> {
        // A single-process host: every session is served here.
        Some(meerkat_runtime::grant_session_hosting(
            &meerkat_runtime::HostingCapability::ProcessLocal,
            &meerkat_runtime::HostingOwner::mint(),
            session_id,
        ))
    }

    async fn resolve_address(&self, address: &LogicalRuntimeId) -> AddressResolution {
        self.resolutions
            .lock()
            .expect("lock")
            .get(address)
            .cloned()
            .unwrap_or_else(|| meerkat::default_address_resolution(address))
    }

    async fn continuation_sink(
        &self,
        _session_id: &SessionId,
    ) -> Option<Arc<dyn ContinuationDeliverySink>> {
        self.sink
            .clone()
            .map(|sink| sink as Arc<dyn ContinuationDeliverySink>)
    }

    fn retained_job_source(&self) -> Option<Arc<dyn RetainedJobSource>> {
        self.job_source
            .clone()
            .map(|jobs| jobs as Arc<dyn RetainedJobSource>)
    }
}

/// The committed owner of one producer's jobs (fork_off unless a test
/// says otherwise), as a table the test edits. It holds no other
/// producer's jobs.
#[derive(Default)]
struct FakeJobs {
    jobs: Mutex<HashMap<String, RetainedJobFacts>>,
    unavailable: Mutex<Option<String>>,
    /// The producer this owner holds jobs for; `None` is fork_off.
    producer: Mutex<Option<ContinuationProducer>>,
    /// Every lookup asked of this owner.
    lookups: std::sync::atomic::AtomicUsize,
    /// Parks the next lookup after it decided its answer, until released.
    held: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
}

impl FakeJobs {
    fn holding(job_id: &str, facts: RetainedJobFacts) -> Arc<Self> {
        let jobs = Arc::new(Self::default());
        jobs.jobs
            .lock()
            .expect("lock")
            .insert(job_id.to_string(), facts);
        jobs
    }

    fn change(&self, job_id: &str, change: impl FnOnce(&mut RetainedJobFacts)) {
        change(
            self.jobs
                .lock()
                .expect("lock")
                .get_mut(job_id)
                .expect("job"),
        );
    }

    fn lookups(&self) -> usize {
        self.lookups.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Park the next lookup once it decided its answer: the owner asking is
    /// then inside a pass, past the marks of its signals. The first receiver
    /// fires when it parks; the lookup answers once the sender fires.
    fn hold_next_lookup(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        *self.held.lock().expect("lock") = Some((entered_tx, release_rx));
        (entered_rx, release_tx)
    }

    fn answer(&self, producer: &ContinuationProducer, job_id: &str) -> RetainedJobLookup {
        let owned = self
            .producer
            .lock()
            .expect("lock")
            .clone()
            .unwrap_or(ContinuationProducer::ForkOff);
        if let Some(error) = self.unavailable.lock().expect("lock").clone() {
            return RetainedJobLookup::Unavailable(error);
        }
        if producer != &owned {
            return RetainedJobLookup::Absent;
        }
        self.jobs
            .lock()
            .expect("lock")
            .get(job_id)
            .cloned()
            .map_or(RetainedJobLookup::Absent, RetainedJobLookup::Found)
    }
}

#[async_trait::async_trait]
impl RetainedJobSource for FakeJobs {
    async fn retained_job(
        &self,
        producer: &ContinuationProducer,
        job_id: &str,
    ) -> RetainedJobLookup {
        self.lookups
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let answer = self.answer(producer, job_id);
        let held = self.held.lock().expect("lock").take();
        if let Some((entered, release)) = held {
            let _ = entered.send(());
            let _ = release.await;
        }
        answer
    }
}

struct Fixture {
    inbox: RuntimeDeliveryInbox,
    continuations: ContinuationOwnerService,
    _dir: Option<tempfile::TempDir>,
}

impl Fixture {
    fn new() -> Self {
        Self::on(Arc::new(InMemoryRuntimeStore::new()), None)
    }

    #[cfg(feature = "sqlite-store")]
    fn sqlite() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = meerkat_runtime::SqliteRuntimeStore::new(dir.path().join("runtime.sqlite3"))
            .expect("open sqlite runtime store");
        Self::on(Arc::new(store), Some(dir))
    }

    fn on(store: Arc<dyn RuntimeStore>, dir: Option<tempfile::TempDir>) -> Self {
        let inbox = RuntimeDeliveryInbox::new(store);
        Self {
            continuations: ContinuationOwnerService::new(
                inbox.clone(),
                Arc::new(SessionAddressResolver),
                Arc::new(meerkat_runtime::MeerkatMachine::ephemeral()),
            ),
            inbox,
            _dir: dir,
        }
    }

    /// Drive the admission index to `transition`, as the applier does before
    /// a modeled crash.
    async fn admission(
        &self,
        address: &LogicalRuntimeId,
        delivery_id: &str,
        transition: ContinuationAdmissionTransition,
    ) {
        let outcome = self
            .inbox
            .transition_continuation_admission(
                address,
                &RuntimeDeliveryId::new(delivery_id.to_string()).expect("id"),
                transition,
            )
            .await
            .expect("admission index write");
        assert!(
            matches!(outcome, ContinuationAdmissionOutcome::Transitioned(_)),
            "{outcome:?}"
        );
    }

    fn owner(&self) -> RuntimeDeliveryOwner {
        RuntimeDeliveryOwner::new(Arc::new(MemoryDetachedJobStore::new()), self.inbox.clone())
    }
}

fn delivery(key: &str, producer: ContinuationProducer) -> ContinuationDelivery {
    ContinuationDelivery {
        key: ContinuationKey::new(key).expect("key"),
        result: ContinuationResultRef {
            producer,
            producer_id: "op-1".into(),
            result_digest: "sha256:result".into(),
            summary: None,
        },
        body: "result".into(),
        handling: ContinuationHandling::Queue,
    }
}

fn host_producer() -> ContinuationProducer {
    ContinuationProducer::Host {
        namespace: "tasks".into(),
    }
}

async fn wait_for_pass(
    passes: &mut watch::Receiver<RuntimeDeliveryPass>,
    what: &str,
    predicate: impl FnMut(&RuntimeDeliveryPass) -> bool,
) -> RuntimeDeliveryPass {
    tokio::time::timeout(EVENT_GUARD, passes.wait_for(predicate))
        .await
        .unwrap_or_else(|_| panic!("delivery owner never reached: {what}"))
        .expect("owner pass channel open")
        .clone()
}

async fn wait_for_status(
    fixture: &Fixture,
    owner: &ContinuationOwner,
    key: &str,
    passes: &mut watch::Receiver<RuntimeDeliveryPass>,
    predicate: impl Fn(&ContinuationStatus) -> bool,
) -> ContinuationStatus {
    let key = ContinuationKey::new(key).expect("key");
    tokio::time::timeout(EVENT_GUARD, async {
        loop {
            let status = fixture
                .continuations
                .continuation_status(owner, &key)
                .await
                .expect("status");
            if predicate(&status) {
                return status;
            }
            passes.changed().await.expect("owner pass channel open");
        }
    })
    .await
    .expect("continuation status reached")
}

#[tokio::test]
async fn a_continuation_is_admitted_once_and_reads_applied() {
    let fixture = Fixture::new();
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let handle = fixture
        .owner()
        .arm(Host::new(Some(sink.clone())))
        .expect("arm");
    let mut passes = handle.subscribe_passes();

    let receipt = fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let status = wait_for_status(&fixture, &owner, "task-1", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;
    let admissions = sink.admissions();
    assert_eq!(admissions.len(), 1, "admitted exactly once");
    let (admitted_session, admission_key, input) = admissions[0].clone();
    assert_eq!(admitted_session, session);
    assert_eq!(
        admission_key, receipt.delivery_id,
        "host results admit under the hash-namespaced delivery id"
    );
    assert_eq!(
        status,
        ContinuationStatus::Applied {
            receipt,
            session,
            input
        }
    );
}

/// The E11 cut: the input reached session s1, the process died before the
/// inbox acknowledgement, and the owner was repointed to s2. Recovery finds
/// the s1 input by its admission key and admits nothing to s2.
async fn admitted_before_the_acknowledgement_then_repointed(fixture: Fixture) {
    let sink = Arc::new(RecordingSink::default());
    let s1 = SessionId::new();
    let s2 = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: s1.clone(),
    };
    let receipt = fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    let admitted = InputId::new();
    fixture
        .admission(
            &address,
            &receipt.delivery_id,
            ContinuationAdmissionTransition::Reserve {
                session_id: s1.clone(),
                input_id: admitted.clone(),
            },
        )
        .await;
    sink.already_holds(
        &s1,
        &continuation_input(admitted.clone(), &receipt.delivery_id, "result"),
    );

    let host = Host::new(Some(sink.clone()));
    host.resolve(&address, AddressResolution::Session(s2));
    let handle = fixture.owner().arm(host).expect("arm");
    let mut passes = handle.subscribe_passes();
    let status = wait_for_status(&fixture, &owner, "task-1", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;
    assert!(sink.admissions().is_empty(), "nothing is admitted again");
    assert_eq!(
        status,
        ContinuationStatus::Applied {
            receipt,
            session: s1,
            input: admitted
        }
    );
}

/// A reservation that never reached its session moves to the session that
/// serves the owner now, and is admitted there once.
async fn reserved_before_admission_then_repointed(fixture: Fixture) {
    let sink = Arc::new(RecordingSink::default());
    let s1 = SessionId::new();
    let s2 = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: s1.clone(),
    };
    let receipt = fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    fixture
        .admission(
            &address,
            &receipt.delivery_id,
            ContinuationAdmissionTransition::Reserve {
                session_id: s1.clone(),
                input_id: InputId::new(),
            },
        )
        .await;

    let host = Host::new(Some(sink.clone()));
    host.resolve(&address, AddressResolution::Session(s2.clone()));
    let handle = fixture.owner().arm(host).expect("arm");
    let mut passes = handle.subscribe_passes();
    let status = wait_for_status(&fixture, &owner, "task-1", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;
    let admissions = sink.admissions();
    assert_eq!(admissions.len(), 1);
    assert_eq!(admissions[0].0, s2);
    let ContinuationStatus::Applied { session, .. } = status else {
        unreachable!("waited for Applied");
    };
    assert_eq!(session, s2);

    // The superseded session can no longer be recorded as the admission.
    let late = fixture
        .inbox
        .transition_continuation_admission(
            &address,
            &RuntimeDeliveryId::new(receipt.delivery_id.clone()).expect("id"),
            ContinuationAdmissionTransition::Apply {
                session_id: s1,
                input_id: InputId::new(),
            },
        )
        .await
        .expect("admission index read");
    assert!(
        matches!(late, ContinuationAdmissionOutcome::Rejected { current: Some(ContinuationAdmission::Applied { ref session_id, .. }) } if *session_id == s2),
        "{late:?}"
    );
}

/// Reserved and never admitted, with the owner still in the same session:
/// admitted there once, under the reserved input id.
async fn reserved_before_admission_in_the_same_session(fixture: Fixture) {
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let receipt = fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    let reserved = InputId::new();
    fixture
        .admission(
            &address,
            &receipt.delivery_id,
            ContinuationAdmissionTransition::Reserve {
                session_id: session.clone(),
                input_id: reserved.clone(),
            },
        )
        .await;

    let handle = fixture
        .owner()
        .arm(Host::new(Some(sink.clone())))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let status = wait_for_status(&fixture, &owner, "task-1", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;
    assert_eq!(
        sink.admissions(),
        vec![(
            session.clone(),
            receipt.delivery_id.clone(),
            reserved.clone()
        )]
    );
    assert_eq!(
        status,
        ContinuationStatus::Applied {
            receipt,
            session,
            input: reserved
        }
    );
}

/// The index reached `Applied` in s1 and the process died before the inbox
/// acknowledgement; the owner moved to s2. Recovery acknowledges the row and
/// admits nothing.
async fn applied_in_the_index_before_the_acknowledgement_then_repointed(fixture: Fixture) {
    let sink = Arc::new(RecordingSink::default());
    let s1 = SessionId::new();
    let s2 = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: s1.clone(),
    };
    let receipt = fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    let admitted = InputId::new();
    for transition in [
        ContinuationAdmissionTransition::Reserve {
            session_id: s1.clone(),
            input_id: admitted.clone(),
        },
        ContinuationAdmissionTransition::Apply {
            session_id: s1.clone(),
            input_id: admitted.clone(),
        },
    ] {
        fixture
            .admission(&address, &receipt.delivery_id, transition)
            .await;
    }
    assert_eq!(
        fixture
            .continuations
            .continuation_status(&owner, &ContinuationKey::new("task-1").expect("key"))
            .await
            .expect("status"),
        ContinuationStatus::Pending {
            receipt: receipt.clone(),
            sequence: 1,
            admitted: Some((s1.clone(), admitted.clone()))
        }
    );

    let host = Host::new(Some(sink.clone()));
    host.resolve(&address, AddressResolution::Session(s2));
    let handle = fixture.owner().arm(host).expect("arm");
    let mut passes = handle.subscribe_passes();
    let status = wait_for_status(&fixture, &owner, "task-1", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;
    assert!(sink.admissions().is_empty(), "nothing is admitted again");
    assert_eq!(
        status,
        ContinuationStatus::Applied {
            receipt,
            session: s1,
            input: admitted
        }
    );
}

macro_rules! crash_cut {
    ($case:ident) => {
        mod $case {
            #[tokio::test]
            async fn memory() {
                super::$case(super::Fixture::new()).await;
            }

            #[cfg(feature = "sqlite-store")]
            #[tokio::test]
            async fn sqlite() {
                super::$case(super::Fixture::sqlite()).await;
            }
        }
    };
}

crash_cut!(admitted_before_the_acknowledgement_then_repointed);
crash_cut!(reserved_before_admission_then_repointed);
crash_cut!(reserved_before_admission_in_the_same_session);
crash_cut!(applied_in_the_index_before_the_acknowledgement_then_repointed);

/// A fork_off completion delivered live before it became a continuation
/// stands for it only as an exact replay; a host key that happens to equal
/// an unrelated input's key does not dedupe at all.
#[tokio::test]
async fn an_exact_live_replay_under_a_fork_off_key_is_applied_and_host_keys_never_dedupe() {
    let fixture = Fixture::new();
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let live = InputId::new();
    sink.already_holds(
        &session,
        &continuation_input(live.clone(), "fork_off:job-7", "result"),
    );
    sink.already_holds(
        &session,
        &continuation_input(InputId::new(), "host-collision", "result"),
    );

    let fork = fixture
        .continuations
        .submit(
            &owner,
            delivery("fork_off:job-7", ContinuationProducer::ForkOff),
            1,
        )
        .await
        .expect("fork continuation");
    let host = fixture
        .continuations
        .submit(&owner, delivery("host-collision", host_producer()), 2)
        .await
        .expect("host continuation");
    let handle = fixture
        .owner()
        .arm(Host::new(Some(sink.clone())))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let fork_status = wait_for_status(&fixture, &owner, "fork_off:job-7", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;
    wait_for_status(&fixture, &owner, "host-collision", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;
    assert_eq!(
        fork_status,
        ContinuationStatus::Applied {
            receipt: fork,
            session: session.clone(),
            input: live
        },
        "the exact live input stands for the fork completion"
    );
    let admissions = sink.admissions();
    assert_eq!(
        admissions.len(),
        1,
        "only the host continuation is admitted"
    );
    assert_eq!(admissions[0].1, host.delivery_id);
}

/// P3-B B5: a notice-bodied continuation (a fork_off or council completion)
/// admits exactly the completion input a live delivery admitted
/// (`PromptInput::detached_job_completed` under the job's key), so the
/// owner's transcript is byte for byte what it always was. Only the input id
/// (the reserved one) and the admission time differ.
#[tokio::test]
async fn a_notice_continuation_admits_exactly_the_live_completion_input() {
    let fixture = Fixture::new();
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let notice = meerkat_core::types::SystemNoticeMessage::persisted_background_job(
        "fork_off",
        "job-9",
        meerkat_core::event::BackgroundJobTerminalStatus::Completed,
        r#"{"status":"completed","text":"done"}"#.to_string(),
    );
    let mut fork = delivery("fork_off:job-9", ContinuationProducer::ForkOff);
    fork.body = meerkat::ContinuationBody::notice(notice.clone());
    fixture
        .continuations
        .submit(&owner, fork, 1)
        .await
        .expect("fork continuation");
    let handle = fixture
        .owner()
        .arm(Host::new(Some(sink.clone())))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    wait_for_status(&fixture, &owner, "fork_off:job-9", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;

    let inputs = sink.inputs.lock().expect("lock").clone();
    let [meerkat_runtime::Input::Prompt(admitted)] = inputs.as_slice() else {
        panic!("one prompt input is admitted: {inputs:?}");
    };
    let admitted = admitted.clone();
    let mut live = meerkat_runtime::PromptInput::detached_job_completed("fork_off:job-9", notice);
    live.header.id = admitted.header.id.clone();
    live.header.timestamp = admitted.header.timestamp;
    assert_eq!(
        serde_json::to_value(meerkat_runtime::Input::Prompt(admitted)).expect("encode"),
        serde_json::to_value(meerkat_runtime::Input::Prompt(live)).expect("encode"),
    );
}

/// A row under the same live key that is not this delivery (another result,
/// or an older admission with no recorded identity) is a conflict: the
/// delivery stays pending and visible, is never applied by key, and nothing
/// is admitted over it.
async fn another_row_under_the_live_key_is_a_conflict(
    seed: impl FnOnce(&RecordingSink, &SessionId),
) {
    let fixture = Fixture::new();
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    seed(&sink, &session);
    fixture
        .continuations
        .submit(
            &owner,
            delivery("fork_off:job-7", ContinuationProducer::ForkOff),
            1,
        )
        .await
        .expect("fork continuation");
    let handle = fixture
        .owner()
        .arm(Host::new(Some(sink.clone())))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let pass = wait_for_pass(&mut passes, "the conflicting row blocked", |pass| {
        !pass.blocked_runtimes.is_empty()
    })
    .await;
    assert!(
        pass.failures
            .iter()
            .any(|failure| failure.contains("is not this delivery")),
        "visible as a conflict: {:?}",
        pass.failures
    );
    assert!(sink.admissions().is_empty(), "nothing is admitted over it");
    assert!(matches!(
        fixture
            .continuations
            .continuation_status(
                &owner,
                &ContinuationKey::new("fork_off:job-7").expect("key")
            )
            .await
            .expect("status"),
        ContinuationStatus::Pending { admitted: None, .. }
    ));
}

#[tokio::test]
async fn a_changed_result_under_the_live_key_is_a_conflict() {
    another_row_under_the_live_key_is_a_conflict(|sink, session| {
        sink.already_holds(
            session,
            &continuation_input(InputId::new(), "fork_off:job-7", "an earlier result"),
        );
    })
    .await;
}

#[tokio::test]
async fn an_older_row_without_an_identity_under_the_live_key_is_a_conflict() {
    another_row_under_the_live_key_is_a_conflict(|sink, session| {
        sink.holds_legacy_row(session, "fork_off:job-7");
    })
    .await;
}

/// A member address no session serves stays blocked, and is applied once an
/// attachment commit retries it with a session serving it.
#[tokio::test]
async fn an_unserved_member_address_is_applied_when_an_attachment_serves_it() {
    let fixture = Fixture::new();
    let sink = Arc::new(RecordingSink::default());
    let address =
        meerkat::member_delivery_address("team", team_incarnation(), "lead", 1).expect("address");
    let session = SessionId::new();
    let submission = meerkat_runtime::RuntimeDeliverySubmission::new(
        RuntimeDeliveryId::new("continuation:member").expect("id"),
        meerkat_runtime::RuntimeDeliveryKind::Continuation,
        "host:tasks",
        1,
        "task-1",
        serde_json::to_vec(&delivery("task-1", host_producer())).expect("encode"),
    )
    .expect("submission");
    fixture
        .inbox
        .submit(&address, submission)
        .await
        .expect("commit");
    let host = Host::new(Some(sink.clone()));
    host.resolve(&address, AddressResolution::NotServed);
    let (attachments, attachment_commits) = watch::channel(0_u64);
    let handle = fixture
        .owner()
        .with_attachment_commits(attachment_commits)
        .arm(host.clone())
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    wait_for_pass(&mut passes, "the unserved address blocked", |pass| {
        pass.blocked_runtimes.contains(&address)
    })
    .await;
    assert!(sink.admissions().is_empty());

    host.resolve(&address, AddressResolution::Session(session.clone()));
    attachments.send_modify(|generation| *generation += 1);
    wait_for_pass(&mut passes, "the address applied", |pass| {
        !pass.blocked_runtimes.contains(&address) && pass.applied > 0
    })
    .await;
    assert_eq!(sink.admissions().len(), 1);
    assert_eq!(sink.admissions()[0].0, session);
}

/// A host without a continuation sink leaves continuation rows visibly
/// blocked as an unsupported kind.
#[tokio::test]
async fn a_host_without_a_continuation_sink_blocks_rows_as_unsupported() {
    let fixture = Fixture::new();
    let owner = ContinuationOwner::Session {
        session_id: SessionId::new(),
    };
    fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let handle = fixture.owner().arm(Host::new(None)).expect("arm");
    let mut passes = handle.subscribe_passes();
    let pass = wait_for_pass(&mut passes, "the unsupported row blocked", |pass| {
        !pass.blocked_runtimes.is_empty()
    })
    .await;
    assert!(
        pass.failures
            .iter()
            .any(|failure| failure.contains("UnsupportedKind")),
        "visible as unsupported: {:?}",
        pass.failures
    );
}

/// A historical continuation row (committed while the runtime was not
/// governed) reaching a runtime with a native work authorization host is
/// settled as refused, typed, before anything is reserved or admitted. It is
/// never applied, its status reads `Refused`, and the delivery committed
/// after it on the same runtime is delivered.
#[tokio::test]
async fn a_refused_continuation_never_holds_the_delivery_behind_it() {
    let fixture = Fixture::new();
    let sink = Arc::new(RecordingSink {
        governed: true,
        ..RecordingSink::default()
    });
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let receipt = fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit while the runtime is not governed");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    let job_id = meerkat::JobId::generated();
    let permitted = meerkat_runtime::RuntimeDeliverySubmission::new(
        RuntimeDeliveryId::new("job:permitted:1").expect("id"),
        meerkat_runtime::RuntimeDeliveryKind::JobNotification,
        job_id.as_str(),
        1,
        "lineage",
        serde_json::to_vec(&meerkat::JobNotificationDeliveryPayload {
            job_id: job_id.clone(),
            delivery_sequence: 1,
            origin_session_id: session.clone(),
            interaction_lineage_id: meerkat::InteractionLineageId::new(),
            targets: Vec::new(),
            notification: meerkat::JobNotification::new("n-1", "n-1", "progress", "halfway")
                .expect("notification"),
        })
        .expect("encode"),
    )
    .expect("submission");
    fixture
        .inbox
        .submit(&address, permitted)
        .await
        .expect("a later delivery on the same runtime");

    let handle = fixture
        .owner()
        .arm(Host::new(Some(sink.clone())))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let pass = wait_for_pass(&mut passes, "the refused row settled", |pass| {
        pass.refused > 0
    })
    .await;
    assert_eq!(
        pass.applied, 1,
        "the permitted delivery behind it is delivered"
    );
    assert!(
        pass.blocked_runtimes.is_empty(),
        "nothing holds the runtime"
    );
    assert!(
        pass.failures
            .iter()
            .any(|failure| failure.contains("refused (NoAdmissibleWorkBinding)")),
        "visible as a refusal: {:?}",
        pass.failures
    );
    assert!(sink.admissions().is_empty(), "nothing was admitted");
    assert_eq!(
        fixture
            .inbox
            .continuation_admission(
                &address,
                &RuntimeDeliveryId::new(receipt.delivery_id.clone()).expect("id"),
            )
            .await
            .expect("admission index"),
        None,
        "nothing was reserved"
    );
    assert_eq!(
        fixture
            .continuations
            .continuation_status(&owner, &ContinuationKey::new("task-1").expect("key"))
            .await
            .expect("status"),
        ContinuationStatus::Refused {
            receipt,
            sequence: 1,
            reason: meerkat_runtime::RuntimeDeliveryRefusalReason::NoAdmissibleWorkBinding
        }
    );
    assert_eq!(
        fixture
            .inbox
            .pending_delivery_total()
            .await
            .expect("backlog"),
        0,
        "the refusal is settled, not pending"
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

/// The machine sink reports a native work authorization host, which is what
/// makes the owner refuse continuations there.
#[tokio::test]
async fn the_machine_sink_reports_a_native_work_authorization_host() {
    let governed = meerkat_runtime::MeerkatMachine::ephemeral()
        .with_native_work_authorization_host(Arc::new(RefusingWorkAuthority))
        .expect("install the native work authorization host");
    assert!(meerkat::MachineContinuationSink::new(Arc::new(governed)).governs_work_authority());
    assert!(
        !meerkat::MachineContinuationSink::new(Arc::new(
            meerkat_runtime::MeerkatMachine::ephemeral()
        ))
        .governs_work_authority()
    );
}

fn work_identity(tag: &str) -> RetainedWorkIdentity {
    RetainedWorkIdentity::new(
        "rt:session:fork-owner",
        meerkat_core::lifecycle::RunId::new(),
        vec![RetainedContributorRef {
            input_id: InputId::new(),
            association_digest: format!("association-{tag}"),
            submission_digest: format!("submission-{tag}"),
        }],
        std::collections::BTreeMap::new(),
        None,
    )
}

fn fork_completion(job_id: &str, result_digest: &str) -> ContinuationDelivery {
    ContinuationDelivery {
        key: ContinuationKey::new(format!("fork_off:{job_id}")).expect("key"),
        result: ContinuationResultRef {
            producer: ContinuationProducer::ForkOff,
            producer_id: job_id.into(),
            result_digest: result_digest.into(),
            summary: None,
        },
        body: "the fork finished".into(),
        handling: ContinuationHandling::Queue,
    }
}

/// A fork_off completion committed from its job's record, owed to `session`.
async fn retained_fork_completion(
    fixture: &Fixture,
    session: &SessionId,
    jobs: &FakeJobs,
) -> meerkat::ContinuationReceipt {
    let record = RetainedJobRecord::from_owner(jobs, ContinuationProducer::ForkOff, "job-1")
        .await
        .expect("read the job owner")
        .expect("the job is committed");
    fixture
        .continuations
        .submit_retained_completion(
            &ContinuationOwner::Session {
                session_id: session.clone(),
            },
            fork_completion("job-1", "sha256:result"),
            &record,
            1,
        )
        .await
        .expect("commit the completion")
}

fn committed_job(session: &SessionId, identity: &RetainedWorkIdentity) -> RetainedJobFacts {
    RetainedJobFacts {
        owner_session_id: session.clone(),
        retained_work: Some(identity.clone()),
        result_digest: "sha256:result".into(),
    }
}

/// On a governed runtime a retained completion is admitted as a resume of
/// its retained work once the job's committed owner confirms it.
#[tokio::test]
async fn a_confirmed_retained_completion_is_resumed_on_a_governed_runtime() {
    let fixture = Fixture::new();
    let session = SessionId::new();
    let identity = work_identity("a");
    let jobs = FakeJobs::holding("job-1", committed_job(&session, &identity));
    let receipt = retained_fork_completion(&fixture, &session, &jobs).await;
    let sink = Arc::new(RecordingSink::governed());
    let handle = fixture
        .owner()
        .arm(Host::with_jobs(sink.clone(), jobs))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let status = wait_for_status(&fixture, &owner, "fork_off:job-1", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;
    let resumes = sink.resumes();
    assert_eq!(resumes.len(), 1);
    assert_eq!(resumes[0].0, session);
    assert_eq!(
        resumes[0].2, identity,
        "the resume names the job's retained work"
    );
    assert!(sink.admissions().is_empty(), "never admitted unbound");
    assert!(
        matches!(status, ContinuationStatus::Applied { receipt: applied, .. } if applied == receipt)
    );
}

/// A completion the job's owner does not confirm exactly (another retained
/// work, another owner session, another result) is an immutably invalid
/// binding: settled, never admitted, and the delivery behind it proceeds.
async fn an_unconfirmed_retained_completion_is_refused(change: impl FnOnce(&mut RetainedJobFacts)) {
    let fixture = Fixture::new();
    let session = SessionId::new();
    let identity = work_identity("a");
    let jobs = FakeJobs::holding("job-1", committed_job(&session, &identity));
    let receipt = retained_fork_completion(&fixture, &session, &jobs).await;
    jobs.change("job-1", change);
    let job_id = meerkat::JobId::generated();
    fixture
        .inbox
        .submit(
            &LogicalRuntimeId::new(receipt.address.clone()),
            meerkat_runtime::RuntimeDeliverySubmission::new(
                RuntimeDeliveryId::new("job:permitted:1").expect("id"),
                meerkat_runtime::RuntimeDeliveryKind::JobNotification,
                job_id.as_str(),
                1,
                "lineage",
                serde_json::to_vec(&meerkat::JobNotificationDeliveryPayload {
                    job_id: job_id.clone(),
                    delivery_sequence: 1,
                    origin_session_id: session.clone(),
                    interaction_lineage_id: meerkat::InteractionLineageId::new(),
                    targets: Vec::new(),
                    notification: meerkat::JobNotification::new(
                        "n-1", "n-1", "progress", "halfway",
                    )
                    .expect("notification"),
                })
                .expect("encode"),
            )
            .expect("submission"),
        )
        .await
        .expect("a permitted delivery behind it");
    let sink = Arc::new(RecordingSink::governed());
    let handle = fixture
        .owner()
        .arm(Host::with_jobs(sink.clone(), jobs))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let pass = wait_for_pass(&mut passes, "the unconfirmed row settled", |pass| {
        pass.refused > 0
    })
    .await;
    assert_eq!(
        pass.applied, 1,
        "the permitted delivery behind it is delivered"
    );
    assert!(sink.resumes().is_empty() && sink.admissions().is_empty());
    assert_eq!(
        fixture
            .continuations
            .continuation_status(
                &ContinuationOwner::Session {
                    session_id: session.clone()
                },
                &ContinuationKey::new("fork_off:job-1").expect("key")
            )
            .await
            .expect("status"),
        ContinuationStatus::Refused {
            receipt,
            sequence: 1,
            reason: meerkat_runtime::RuntimeDeliveryRefusalReason::NoAdmissibleWorkBinding
        }
    );
}

#[tokio::test]
async fn a_completion_naming_other_retained_work_is_refused() {
    an_unconfirmed_retained_completion_is_refused(|facts| {
        facts.retained_work = Some(work_identity("other"));
    })
    .await;
}

#[tokio::test]
async fn a_completion_retargeted_at_another_session_is_refused() {
    an_unconfirmed_retained_completion_is_refused(|facts| {
        facts.owner_session_id = SessionId::new();
    })
    .await;
}

#[tokio::test]
async fn a_completion_with_a_changed_result_is_refused() {
    an_unconfirmed_retained_completion_is_refused(|facts| {
        facts.result_digest = "sha256:another-result".into();
    })
    .await;
}

#[tokio::test]
async fn a_legacy_job_without_retained_work_is_refused() {
    an_unconfirmed_retained_completion_is_refused(|facts| {
        facts.retained_work = None;
    })
    .await;
}

/// Without a configured job owner a retained completion cannot be confirmed
/// now: it stays pending (retryable), never settled.
#[tokio::test]
async fn without_a_job_owner_a_retained_completion_stays_pending() {
    let fixture = Fixture::new();
    let session = SessionId::new();
    let identity = work_identity("a");
    let jobs = FakeJobs::holding("job-1", committed_job(&session, &identity));
    retained_fork_completion(&fixture, &session, &jobs).await;
    let sink = Arc::new(RecordingSink::governed());
    let handle = fixture
        .owner()
        .arm(Host::new(Some(sink.clone())))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let pass = wait_for_pass(&mut passes, "the row blocked", |pass| {
        !pass.blocked_runtimes.is_empty()
    })
    .await;
    assert_eq!(pass.refused, 0, "never settled");
    assert!(sink.resumes().is_empty());
}

/// The admission's verdict decides: an actual denial settles as
/// AuthorityDenied; a retryable failure keeps the row pending.
async fn a_governed_admission_verdict(
    verdict: ContinuationAdmitError,
) -> (RuntimeDeliveryPass, ContinuationStatus) {
    let fixture = Fixture::new();
    let session = SessionId::new();
    let identity = work_identity("a");
    let jobs = FakeJobs::holding("job-1", committed_job(&session, &identity));
    retained_fork_completion(&fixture, &session, &jobs).await;
    let sink = Arc::new(RecordingSink::governed());
    *sink.resume_verdict.lock().expect("lock") = Some(verdict);
    let handle = fixture
        .owner()
        .arm(Host::with_jobs(sink, jobs))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let pass = wait_for_pass(&mut passes, "the verdict applied", |pass| {
        pass.refused > 0 || !pass.blocked_runtimes.is_empty()
    })
    .await;
    let status = fixture
        .continuations
        .continuation_status(
            &ContinuationOwner::Session {
                session_id: session,
            },
            &ContinuationKey::new("fork_off:job-1").expect("key"),
        )
        .await
        .expect("status");
    (pass, status)
}

#[tokio::test]
async fn an_actual_denial_settles_the_retained_completion() {
    let (pass, status) = a_governed_admission_verdict(ContinuationAdmitError::Refused(
        meerkat_runtime::RuntimeDeliveryRefusalReason::AuthorityDenied,
    ))
    .await;
    assert_eq!(pass.refused, 1);
    assert!(matches!(
        status,
        ContinuationStatus::Refused {
            reason: meerkat_runtime::RuntimeDeliveryRefusalReason::AuthorityDenied,
            ..
        }
    ));
}

#[tokio::test]
async fn an_unavailable_owner_keeps_the_retained_completion_pending() {
    let (pass, status) =
        a_governed_admission_verdict(ContinuationAdmitError::Failed("owner unavailable".into()))
            .await;
    assert_eq!(pass.refused, 0);
    assert!(matches!(status, ContinuationStatus::Pending { .. }));
}

/// A job owner that definitively holds no such job is an immutably missing
/// binding; one that cannot be read now keeps the row pending.
#[tokio::test]
async fn an_absent_job_is_refused_and_an_unreadable_owner_keeps_the_row_pending() {
    for unreadable in [false, true] {
        let fixture = Fixture::new();
        let session = SessionId::new();
        let identity = work_identity("a");
        let jobs = FakeJobs::holding("job-1", committed_job(&session, &identity));
        retained_fork_completion(&fixture, &session, &jobs).await;
        if unreadable {
            *jobs.unavailable.lock().expect("lock") = Some("owner store offline".into());
        } else {
            jobs.jobs.lock().expect("lock").clear();
        }
        let sink = Arc::new(RecordingSink::governed());
        let handle = fixture
            .owner()
            .arm(Host::with_jobs(sink.clone(), jobs))
            .expect("arm");
        let mut passes = handle.subscribe_passes();
        let pass = wait_for_pass(&mut passes, "the lookup verdict applied", |pass| {
            pass.refused > 0 || !pass.blocked_runtimes.is_empty()
        })
        .await;
        assert_eq!(
            pass.refused,
            usize::from(!unreadable),
            "unreadable={unreadable}"
        );
        assert!(sink.resumes().is_empty());
    }
}

const SETUP_NAMESPACE: &str = "app.setup";

fn setup_producer(namespace: &str) -> ContinuationProducer {
    ContinuationProducer::Host {
        namespace: namespace.into(),
    }
}

/// A host's committed setup result, owed to `session`, as its owner
/// confirms it, committed as a retained continuation.
async fn retained_setup_completion(
    fixture: &Fixture,
    session: &SessionId,
    identity: &RetainedWorkIdentity,
) -> (Arc<FakeJobs>, meerkat::ContinuationReceipt) {
    let jobs = FakeJobs::holding("setup-1", committed_job(session, identity));
    *jobs.producer.lock().expect("lock") = Some(setup_producer(SETUP_NAMESPACE));
    let record = RetainedJobRecord::from_owner(&*jobs, setup_producer(SETUP_NAMESPACE), "setup-1")
        .await
        .expect("read the setup owner")
        .expect("the setup result is committed");
    let receipt = fixture
        .continuations
        .submit_retained_completion(
            &ContinuationOwner::Session {
                session_id: session.clone(),
            },
            ContinuationDelivery {
                key: ContinuationKey::new("setup:setup-1").expect("key"),
                result: ContinuationResultRef {
                    producer: setup_producer(SETUP_NAMESPACE),
                    producer_id: "setup-1".into(),
                    result_digest: "sha256:result".into(),
                    summary: None,
                },
                body: "setup finished".into(),
                handling: ContinuationHandling::Queue,
            },
            &record,
            1,
        )
        .await
        .expect("commit the setup completion");
    (jobs, receipt)
}

/// A host's committed setup result is confirmed through the same configured
/// job source as a fork_off or council completion, and resumes its retained
/// work on a governed runtime.
#[tokio::test]
async fn a_confirmed_host_setup_completion_is_resumed_on_a_governed_runtime() {
    let fixture = Fixture::new();
    let session = SessionId::new();
    let identity = work_identity("setup");
    let (jobs, receipt) = retained_setup_completion(&fixture, &session, &identity).await;
    let sink = Arc::new(RecordingSink::governed());
    let handle = fixture
        .owner()
        .arm(Host::with_jobs(sink.clone(), jobs))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let status = wait_for_status(&fixture, &owner, "setup:setup-1", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;
    let resumes = sink.resumes();
    assert_eq!(resumes.len(), 1);
    assert_eq!(resumes[0].0, session);
    assert_eq!(
        resumes[0].2, identity,
        "the resume names the setup's retained work"
    );
    assert!(sink.admissions().is_empty(), "never admitted unbound");
    assert!(
        matches!(status, ContinuationStatus::Applied { receipt: applied, .. } if applied == receipt)
    );
}

/// A host completion is confirmed only by a source that owns its exact
/// namespace and job: once the configured source no longer owns that
/// namespace, or its committed facts differ, the row is an immutably invalid
/// binding; an unreadable owner keeps it pending.
#[tokio::test]
async fn a_host_completion_needs_its_namespaces_owner_to_confirm_it_exactly() {
    for case in [
        "other_namespace",
        "other_result",
        "other_session",
        "unreadable",
    ] {
        let fixture = Fixture::new();
        let session = SessionId::new();
        let identity = work_identity("setup");
        let (jobs, receipt) = retained_setup_completion(&fixture, &session, &identity).await;
        match case {
            "other_namespace" => {
                *jobs.producer.lock().expect("lock") = Some(setup_producer("app.other"));
            }
            "other_result" => jobs.change("setup-1", |facts| {
                facts.result_digest = "sha256:another-result".into();
            }),
            "other_session" => jobs.change("setup-1", |facts| {
                facts.owner_session_id = SessionId::new();
            }),
            _ => *jobs.unavailable.lock().expect("lock") = Some("setup owner offline".into()),
        }
        let sink = Arc::new(RecordingSink::governed());
        let handle = fixture
            .owner()
            .arm(Host::with_jobs(sink.clone(), jobs))
            .expect("arm");
        let mut passes = handle.subscribe_passes();
        let pass = wait_for_pass(&mut passes, "the confirmation verdict applied", |pass| {
            pass.refused > 0 || !pass.blocked_runtimes.is_empty()
        })
        .await;
        assert!(
            sink.resumes().is_empty() && sink.admissions().is_empty(),
            "{case}"
        );
        let status = fixture
            .continuations
            .continuation_status(
                &ContinuationOwner::Session {
                    session_id: session.clone(),
                },
                &ContinuationKey::new("setup:setup-1").expect("key"),
            )
            .await
            .expect("status");
        if case == "unreadable" {
            assert_eq!(pass.refused, 0, "{case}: never settled");
            assert!(
                matches!(status, ContinuationStatus::Pending { .. }),
                "{case}"
            );
        } else {
            assert_eq!(
                status,
                ContinuationStatus::Refused {
                    receipt,
                    sequence: 1,
                    reason: meerkat_runtime::RuntimeDeliveryRefusalReason::NoAdmissibleWorkBinding
                },
                "{case}"
            );
        }
    }
}

fn setup_delivery(body: &str) -> ContinuationDelivery {
    ContinuationDelivery {
        key: ContinuationKey::new("setup:setup-1").expect("key"),
        result: ContinuationResultRef {
            producer: setup_producer(SETUP_NAMESPACE),
            producer_id: "setup-1".into(),
            result_digest: "sha256:result".into(),
            summary: None,
        },
        body: body.into(),
        handling: ContinuationHandling::Queue,
    }
}

/// The exact-content observation never commits: before a submission it
/// reports nothing submitted (and the key stays unbound); after it, the
/// exact content matches with its status; other content under the same key
/// is a typed mismatch, never that continuation's status.
#[tokio::test]
async fn an_exact_content_observation_never_commits_and_tells_other_content_apart() {
    let fixture = Fixture::new();
    let session = SessionId::new();
    let identity = work_identity("setup");
    let jobs = FakeJobs::holding("setup-1", committed_job(&session, &identity));
    *jobs.producer.lock().expect("lock") = Some(setup_producer(SETUP_NAMESPACE));
    let record = RetainedJobRecord::from_owner(&*jobs, setup_producer(SETUP_NAMESPACE), "setup-1")
        .await
        .expect("read the setup owner")
        .expect("the setup result is committed");
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let key = ContinuationKey::new("setup:setup-1").expect("key");

    assert_eq!(
        fixture
            .continuations
            .observe_retained_completion(&owner, &setup_delivery("setup finished"), &record)
            .await
            .expect("observe"),
        meerkat::ContinuationObservation::NotSubmitted
    );
    assert_eq!(
        fixture
            .continuations
            .continuation_status(&owner, &key)
            .await
            .expect("status"),
        ContinuationStatus::NotCommitted,
        "observing committed nothing"
    );

    let receipt = fixture
        .continuations
        .submit_retained_completion(&owner, setup_delivery("setup finished"), &record, 1)
        .await
        .expect("commit the setup completion");
    match fixture
        .continuations
        .observe_retained_completion(&owner, &setup_delivery("setup finished"), &record)
        .await
        .expect("observe")
    {
        meerkat::ContinuationObservation::Matched {
            receipt: observed,
            status,
        } => {
            assert_eq!(observed, receipt);
            assert_eq!(
                status,
                fixture
                    .continuations
                    .continuation_status(&owner, &key)
                    .await
                    .expect("status")
            );
        }
        other => panic!("the exact content matches: {other:?}"),
    }
    assert!(matches!(
        fixture
            .continuations
            .observe_retained_completion(&owner, &setup_delivery("another body"), &record)
            .await,
        Err(meerkat::ContinuationObserveError::ContentMismatch { existing }) if *existing == receipt
    ));
    let mut other_result = setup_delivery("setup finished");
    other_result.result.result_digest = "sha256:another-result".into();
    assert!(matches!(
        fixture
            .continuations
            .observe_retained_completion(&owner, &other_result, &record)
            .await,
        Err(meerkat::ContinuationObserveError::Invalid(_))
    ));
}

/// A delivery host whose member resolution and job owner come from late-bound
/// [`meerkat::ContinuationHostBindings`], as the product hosts' do.
struct BindingsHost {
    bindings: Arc<meerkat::ContinuationHostBindings>,
    sink: Arc<RecordingSink>,
}

#[async_trait::async_trait]
impl RuntimeDeliveryHost for BindingsHost {
    async fn delivery_route(&self, _session_id: &SessionId) -> Option<meerkat::DeliveryRoute> {
        Some(meerkat::DeliveryRoute::ServedHere(Arc::new(NoJobs)))
    }

    async fn claim_cold_delivery(
        &self,
        session_id: &SessionId,
    ) -> Option<Result<meerkat_runtime::HostingClaim, meerkat_runtime::HostingRefused>> {
        // A single-process host: every session is served here.
        Some(meerkat_runtime::grant_session_hosting(
            &meerkat_runtime::HostingCapability::ProcessLocal,
            &meerkat_runtime::HostingOwner::mint(),
            session_id,
        ))
    }

    async fn resolve_address(&self, address: &LogicalRuntimeId) -> AddressResolution {
        self.bindings.resolve_address(address).await
    }

    async fn continuation_sink(
        &self,
        _session_id: &SessionId,
    ) -> Option<Arc<dyn ContinuationDeliverySink>> {
        Some(self.sink.clone())
    }

    fn retained_job_source(&self) -> Option<Arc<dyn RetainedJobSource>> {
        self.bindings.job_source()
    }
}

/// Serves every member address by one session.
struct ServedBy(SessionId);

#[async_trait::async_trait]
impl meerkat::ContinuationAddressResolver for ServedBy {
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
        Ok(
            match address
                .member_address()
                .map_err(|error| error.to_string())?
            {
                Some(_) => AddressResolution::Session(self.0.clone()),
                None => meerkat::default_address_resolution(address),
            },
        )
    }
}

/// Before the host binds its continuation services (the window between
/// arming the delivery owner and building the mob state), a member row is
/// not served and stays blocked, never refused or settled. Once they are
/// bound, the owner's next wake applies it.
#[tokio::test]
async fn a_member_row_before_the_host_binds_stays_blocked_until_a_wake_after_binding() {
    let fixture = Fixture::new();
    let sink = Arc::new(RecordingSink::default());
    let address =
        meerkat::member_delivery_address("team", team_incarnation(), "lead", 1).expect("address");
    let submission = meerkat_runtime::RuntimeDeliverySubmission::new(
        RuntimeDeliveryId::new("continuation:member-window").expect("id"),
        meerkat_runtime::RuntimeDeliveryKind::Continuation,
        "host:tasks",
        1,
        "task-1",
        serde_json::to_vec(&delivery("task-1", host_producer())).expect("encode"),
    )
    .expect("submission");
    fixture
        .inbox
        .submit(&address, submission)
        .await
        .expect("commit");
    let bindings = Arc::new(meerkat::ContinuationHostBindings::default());
    let (attachments, attachment_commits) = watch::channel(0_u64);
    let handle = fixture
        .owner()
        .with_attachment_commits(attachment_commits)
        .arm(Arc::new(BindingsHost {
            bindings: Arc::clone(&bindings),
            sink: sink.clone(),
        }))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let pass = wait_for_pass(&mut passes, "the unbound member row blocked", |pass| {
        pass.blocked_runtimes.contains(&address)
    })
    .await;
    assert_eq!(pass.refused, 0, "never settled");
    assert!(sink.admissions().is_empty());

    let session = SessionId::new();
    let _binding = bindings
        .bind(
            Arc::new(ServedBy(session.clone())),
            Arc::new(FakeJobs::default()),
        )
        .expect("bind");
    attachments.send_modify(|generation| *generation += 1);
    wait_for_pass(&mut passes, "the member row applied", |pass| {
        !pass.blocked_runtimes.contains(&address) && pass.applied > 0
    })
    .await;
    assert_eq!(sink.admissions().len(), 1);
    assert_eq!(sink.admissions()[0].0, session);
}

/// On a governed runtime, a retained completion that reaches the owner
/// before the host binds its job owner stays pending, never refused; once
/// bound, the owner's next wake confirms and resumes it.
#[tokio::test]
async fn a_retained_completion_before_the_host_binds_stays_pending_until_a_wake_after_binding() {
    let fixture = Fixture::new();
    let session = SessionId::new();
    let identity = work_identity("a");
    let jobs = FakeJobs::holding("job-1", committed_job(&session, &identity));
    retained_fork_completion(&fixture, &session, &jobs).await;
    let sink = Arc::new(RecordingSink::governed());
    let bindings = Arc::new(meerkat::ContinuationHostBindings::default());
    let (attachments, attachment_commits) = watch::channel(0_u64);
    let handle = fixture
        .owner()
        .with_attachment_commits(attachment_commits)
        .arm(Arc::new(BindingsHost {
            bindings: Arc::clone(&bindings),
            sink: sink.clone(),
        }))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let pass = wait_for_pass(&mut passes, "the unconfirmed row blocked", |pass| {
        !pass.blocked_runtimes.is_empty()
    })
    .await;
    assert_eq!(pass.refused, 0, "never settled");
    assert!(sink.resumes().is_empty());

    let _binding = bindings
        .bind(Arc::new(SessionAddressResolver), jobs)
        .expect("bind");
    attachments.send_modify(|generation| *generation += 1);
    wait_for_pass(&mut passes, "the completion resumed", |pass| {
        pass.applied > 0
    })
    .await;
    assert_eq!(sink.resumes().len(), 1);
    assert_eq!(sink.resumes()[0].2, identity);
}

/// An acknowledged cutpoint of the owner loop: commit an unrelated
/// continuation and wait for the pass that settles it. The loop captures
/// every signal before the pass it runs, so each signal published before
/// this call has been consumed, and acted on if it had work, when it
/// returns.
async fn loop_cutpoint(
    fixture: &Fixture,
    passes: &mut watch::Receiver<RuntimeDeliveryPass>,
) -> RuntimeDeliveryPass {
    let before = passes.borrow().generation;
    fixture
        .continuations
        .submit(
            &ContinuationOwner::Session {
                session_id: SessionId::new(),
            },
            delivery(&format!("cutpoint-{before}"), host_producer()),
            1,
        )
        .await
        .expect("commit the cutpoint row");
    wait_for_pass(passes, "the cutpoint pass", |pass| {
        pass.generation > before && pass.applied + pass.refused > 0
    })
    .await
}

/// A retained completion whose job owner is bound after the owner armed,
/// while its custody is absent (the host's pin is not recorded yet), stays
/// blocked. Restoring the custody alone wakes nothing: up to an acknowledged
/// cutpoint no one asked the source again. The job owner's announcement
/// wakes the same owner, which asks the source once and resumes the
/// completion exactly once with its exact retained work. A repeated
/// announcement with nothing blocked runs no pass and replays nothing.
#[tokio::test]
async fn a_source_hint_retries_a_blocked_retained_completion_exactly_once() {
    let fixture = Fixture::new();
    let session = SessionId::new();
    let identity = work_identity("custody");
    let jobs = FakeJobs::holding("job-1", committed_job(&session, &identity));
    retained_fork_completion(&fixture, &session, &jobs).await;
    *jobs.unavailable.lock().expect("lock") = Some("custody is not restored".into());
    let sink = Arc::new(RecordingSink::governed());
    let bindings = Arc::new(meerkat::ContinuationHostBindings::default());
    let handle = fixture
        .owner()
        .with_source_changes(bindings.subscribe_source_changes())
        .arm(Arc::new(BindingsHost {
            bindings: Arc::clone(&bindings),
            sink: sink.clone(),
        }))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let unbound = wait_for_pass(&mut passes, "the unbound completion blocked", |pass| {
        !pass.blocked_runtimes.is_empty()
    })
    .await;
    let blocked = unbound.blocked_runtimes.clone();

    // Bound after arming while its custody is absent: the binding wakes the
    // owner, the source cannot answer, and the row stays blocked.
    let asked = jobs.lookups();
    let _binding = bindings
        .bind(Arc::new(SessionAddressResolver), jobs.clone())
        .expect("bind");
    let bound = wait_for_pass(&mut passes, "the bound source was asked", |pass| {
        pass.generation > unbound.generation
    })
    .await;
    assert_eq!(bound.blocked_runtimes, blocked, "still blocked");
    assert_eq!((bound.applied, bound.refused), (0, 0), "never settled");
    assert_eq!(
        jobs.lookups(),
        asked + 1,
        "the binding asked the source once"
    );

    // Custody restored but not announced: nothing retried it by the cutpoint.
    *jobs.unavailable.lock().expect("lock") = None;
    let asked = jobs.lookups();
    let quiet = loop_cutpoint(&fixture, &mut passes).await;
    assert_eq!(quiet.blocked_runtimes, blocked, "still blocked");
    assert_eq!(jobs.lookups(), asked, "nobody asked the source");
    assert!(sink.resumes().is_empty());

    // Announced: the same owner asks again and resumes it exactly once.
    bindings.source_custody_changed();
    let resumed = wait_for_pass(&mut passes, "the announced completion resumed", |pass| {
        pass.applied > 0
    })
    .await;
    assert!(resumed.blocked_runtimes.is_empty());
    assert_eq!(jobs.lookups(), asked + 1, "one fresh read of the source");
    assert_eq!(sink.resumes().len(), 1);
    assert_eq!(sink.resumes()[0].0, session);
    assert_eq!(sink.resumes()[0].2, identity);

    // Announced again: nothing is blocked, so no pass runs for it (the next
    // pass is the cutpoint's own) and nothing is replayed.
    bindings.source_custody_changed();
    let after = loop_cutpoint(&fixture, &mut passes).await;
    assert_eq!(
        after.generation,
        resumed.generation + 1,
        "no pass for the hint"
    );
    assert_eq!(jobs.lookups(), asked + 1, "the source was not asked again");
    assert_eq!(sink.resumes().len(), 1, "no replay");
    assert!(!handle.is_stopped());
}

/// A job owner bound before the owner armed is asked by the arming
/// reconcile. A custody change announced while a pass is inside its source
/// read (after it captured and marked the source revision) is not
/// swallowed: that read still answers what it saw, and the owner retries
/// once more, which resumes the completion exactly once.
#[tokio::test]
async fn a_source_change_published_during_a_blocked_pass_is_retried() {
    let fixture = Fixture::new();
    let session = SessionId::new();
    let identity = work_identity("during");
    let jobs = FakeJobs::holding("job-1", committed_job(&session, &identity));
    retained_fork_completion(&fixture, &session, &jobs).await;
    *jobs.unavailable.lock().expect("lock") = Some("custody is not restored".into());
    let sink = Arc::new(RecordingSink::governed());
    let bindings = Arc::new(meerkat::ContinuationHostBindings::default());
    let _binding = bindings
        .bind(Arc::new(SessionAddressResolver), jobs.clone())
        .expect("bind before arming");
    let asked = jobs.lookups();
    let handle = fixture
        .owner()
        .with_source_changes(bindings.subscribe_source_changes())
        .arm(Arc::new(BindingsHost {
            bindings: Arc::clone(&bindings),
            sink: sink.clone(),
        }))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let armed = wait_for_pass(
        &mut passes,
        "the arming reconcile asked the source",
        |pass| !pass.blocked_runtimes.is_empty(),
    )
    .await;
    assert_eq!(
        jobs.lookups(),
        asked + 1,
        "the reconcile asked the bound source"
    );

    // A retry whose source read parks after deciding "unavailable".
    let (entered, release) = jobs.hold_next_lookup();
    bindings.source_custody_changed();
    entered.await.expect("the retry is inside its source read");
    // Published after that pass captured and marked the revision.
    *jobs.unavailable.lock().expect("lock") = None;
    bindings.source_custody_changed();
    release.send(()).expect("release the parked read");

    let resumed = wait_for_pass(
        &mut passes,
        "the change during the pass was retried",
        |pass| pass.applied > 0,
    )
    .await;
    assert!(
        resumed.generation >= armed.generation + 2,
        "a pass after the parked one"
    );
    assert_eq!(jobs.lookups(), asked + 3, "the parked read and one retry");
    assert_eq!(sink.resumes().len(), 1);
    assert_eq!(sink.resumes()[0].2, identity);
}

/// Through the bundle's own delivery owner and bindings, the composition
/// every facade host arms: a retained completion that blocked before its job
/// owner was bound is resumed by the binding alone, with no attachment, run
/// or custody signal ever raised.
#[cfg(all(feature = "session-store", feature = "memory-store"))]
#[tokio::test]
async fn binding_the_job_owner_alone_resumes_a_blocked_retained_completion() {
    let session_store: Arc<dyn meerkat::SessionStore> = Arc::new(meerkat::MemoryStore::new());
    let bundle = meerkat::PersistenceBundle::new(
        session_store,
        Arc::new(InMemoryRuntimeStore::new()),
        Arc::new(meerkat::MemoryBlobStore::new()),
    )
    .expect("bundle");
    let continuations = ContinuationOwnerService::new(
        bundle.runtime_delivery_inbox(),
        Arc::new(SessionAddressResolver),
        Arc::new(meerkat_runtime::MeerkatMachine::ephemeral()),
    );
    let session = SessionId::new();
    let identity = work_identity("bind-alone");
    let jobs = FakeJobs::holding("job-1", committed_job(&session, &identity));
    let record = RetainedJobRecord::from_owner(&*jobs, ContinuationProducer::ForkOff, "job-1")
        .await
        .expect("read the job owner")
        .expect("the job is committed");
    continuations
        .submit_retained_completion(
            &ContinuationOwner::Session {
                session_id: session.clone(),
            },
            fork_completion("job-1", "sha256:result"),
            &record,
            1,
        )
        .await
        .expect("commit the completion");
    let sink = Arc::new(RecordingSink::governed());
    let bindings = bundle.continuation_bindings();
    let handle = bundle
        .runtime_delivery_owner()
        .arm(Arc::new(BindingsHost {
            bindings: Arc::clone(&bindings),
            sink: sink.clone(),
        }))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    wait_for_pass(&mut passes, "the unbound completion blocked", |pass| {
        !pass.blocked_runtimes.is_empty()
    })
    .await;
    assert!(sink.resumes().is_empty());

    let _binding = bindings
        .bind(Arc::new(SessionAddressResolver), jobs)
        .expect("bind");
    wait_for_pass(&mut passes, "the binding resumed the completion", |pass| {
        pass.applied > 0
    })
    .await;
    assert_eq!(sink.resumes().len(), 1);
    assert_eq!(sink.resumes()[0].2, identity);
}

/// A source hint grants nothing: the retry it wakes re-reads the job
/// owner's committed facts, and a completion they no longer confirm is
/// refused, never resumed.
#[tokio::test]
async fn a_source_hint_never_bypasses_the_current_refusal_checks() {
    let fixture = Fixture::new();
    let session = SessionId::new();
    let identity = work_identity("refused");
    let jobs = FakeJobs::holding("job-1", committed_job(&session, &identity));
    retained_fork_completion(&fixture, &session, &jobs).await;
    *jobs.unavailable.lock().expect("lock") = Some("custody is not restored".into());
    let sink = Arc::new(RecordingSink::governed());
    let bindings = Arc::new(meerkat::ContinuationHostBindings::default());
    let _binding = bindings
        .bind(Arc::new(SessionAddressResolver), jobs.clone())
        .expect("bind");
    let handle = fixture
        .owner()
        .with_source_changes(bindings.subscribe_source_changes())
        .arm(Arc::new(BindingsHost {
            bindings: Arc::clone(&bindings),
            sink: sink.clone(),
        }))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    wait_for_pass(&mut passes, "the completion blocked", |pass| {
        !pass.blocked_runtimes.is_empty()
    })
    .await;

    jobs.change("job-1", |facts| {
        facts.retained_work = Some(work_identity("another"));
    });
    *jobs.unavailable.lock().expect("lock") = None;
    bindings.source_custody_changed();
    let refused = wait_for_pass(&mut passes, "the retried completion refused", |pass| {
        pass.refused > 0
    })
    .await;
    assert!(refused.blocked_runtimes.is_empty(), "settled as refused");
    assert_eq!(refused.applied, 0);
    assert!(sink.resumes().is_empty(), "never resumed");
}

/// Dropping the bindings ends their source hint: the owner neither stops
/// nor runs a pass for the closed signal (the next pass is the cutpoint's
/// own), and it keeps serving its other signals.
#[tokio::test]
async fn dropped_bindings_end_their_source_hint_and_the_owner_keeps_serving() {
    let fixture = Fixture::new();
    let sink = Arc::new(RecordingSink::default());
    let hinting = Arc::new(meerkat::ContinuationHostBindings::default());
    let handle = fixture
        .owner()
        .with_source_changes(hinting.subscribe_source_changes())
        .arm(Arc::new(BindingsHost {
            bindings: Arc::new(meerkat::ContinuationHostBindings::default()),
            sink: sink.clone(),
        }))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let armed = wait_for_pass(&mut passes, "the reconcile pass", |pass| {
        pass.generation >= 1
    })
    .await;

    drop(hinting);
    let first = loop_cutpoint(&fixture, &mut passes).await;
    assert_eq!(
        first.generation,
        armed.generation + 1,
        "no pass for the closure"
    );
    let second = loop_cutpoint(&fixture, &mut passes).await;
    assert_eq!(second.generation, first.generation + 1);
    assert!(!handle.is_stopped(), "a closed hint never stops the owner");
    assert_eq!(sink.admissions().len(), 2, "both cutpoint rows admitted");
}

/// The binding slot is set once per binder. A second binder is refused while
/// the first lives; a takeover must name the current binding's generation,
/// and a stale one is refused, typed. A dropped binding frees the slot.
#[tokio::test]
async fn a_stale_or_blind_continuation_rebind_is_refused() {
    let bindings = meerkat::ContinuationHostBindings::default();
    let member =
        meerkat::member_delivery_address("team", team_incarnation(), "lead", 1).expect("address");
    let (a, b) = (SessionId::new(), SessionId::new());
    let first = bindings
        .bind(Arc::new(ServedBy(a.clone())), Arc::new(FakeJobs::default()))
        .expect("bind a free slot");
    assert_eq!(
        bindings
            .bind(Arc::new(ServedBy(b.clone())), Arc::new(FakeJobs::default()))
            .expect_err("a live binding holds the slot"),
        meerkat::ContinuationBindError::AlreadyBound(first.generation())
    );
    assert_eq!(
        bindings.resolve_address(&member).await,
        AddressResolution::Session(a.clone())
    );

    let second = bindings
        .rebind(
            first.generation(),
            Arc::new(ServedBy(b.clone())),
            Arc::new(FakeJobs::default()),
        )
        .expect("a takeover naming the current binding");
    assert_eq!(
        bindings
            .rebind(
                first.generation(),
                Arc::new(ServedBy(a.clone())),
                Arc::new(FakeJobs::default()),
            )
            .expect_err("a stale takeover"),
        meerkat::ContinuationBindError::StaleGeneration {
            replaced: first.generation(),
            current: second.generation(),
        }
    );
    assert_eq!(
        bindings.resolve_address(&member).await,
        AddressResolution::Session(b),
        "the stale takeover changed nothing"
    );

    drop(second);
    assert_eq!(
        bindings.resolve_address(&member).await,
        AddressResolution::NotServed,
        "a dropped binding serves nothing"
    );
    drop(first);
    bindings
        .bind(Arc::new(ServedBy(a)), Arc::new(FakeJobs::default()))
        .expect("a freed slot binds again");
}

/// #1813 on continuation rows: a continuation's one recipient is the session
/// serving its address, routed by that session's hosting claim BEFORE any
/// reservation or admission. A peer process is simulated by holding its OS
/// locks through separate open files (flock conflicts per open file, exactly
/// as across processes) on a SQLite store with OS-locked claims; its death is
/// closing them.
#[cfg(feature = "session-store")]
mod hosting {
    use super::*;

    use meerkat_runtime::{
        HostingCapability, HostingClaim, HostingOwner, HostingPaths, HostingRefused,
        SessionHostingAuthority, SessionServing, SqliteRuntimeStore,
    };

    /// The owner's store-watch sweep here: how soon a released claim or lock
    /// is noticed.
    const TEST_SWEEP: Duration = Duration::from_millis(50);

    struct Hosted {
        fixture: Fixture,
        paths: HostingPaths,
        capability: HostingCapability,
    }

    fn hosted() -> Hosted {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = HostingPaths {
            hosting_lock_dir: dir.path().join("hosting"),
            cold_delivery_lock: dir.path().join("delivery").join("cold-delivery.lock"),
            database: None,
        };
        let store = Arc::new(
            SqliteRuntimeStore::new(dir.path().join("runtime.sqlite3"))
                .expect("open store")
                .with_hosting_paths(paths.clone()),
        );
        let capability = RuntimeStore::hosting_capability(store.as_ref());
        assert!(capability.is_cross_process());
        Hosted {
            fixture: Fixture::on(store, Some(dir)),
            paths,
            capability,
        }
    }

    /// A peer process's hold on one lock file. Dropping it is its death.
    struct PeerLock {
        _file: std::fs::File,
    }

    fn peer_lock(path: &std::path::Path) -> PeerLock {
        std::fs::create_dir_all(path.parent().expect("lock parent")).expect("lock dir");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .expect("open lock file");
        file.try_lock().expect("the peer takes the lock");
        PeerLock { _file: file }
    }

    /// Records, for each admission, where the admitted session was served
    /// for the host's runtime owner at that moment.
    struct ObservingSink {
        inner: RecordingSink,
        authority: SessionHostingAuthority,
        served_at_admission: Mutex<Vec<SessionServing>>,
    }

    impl ObservingSink {
        fn served_at_admission(&self) -> Vec<SessionServing> {
            self.served_at_admission.lock().expect("lock").clone()
        }
    }

    #[async_trait::async_trait]
    impl ContinuationDeliverySink for ObservingSink {
        async fn admit(
            &self,
            session: &SessionId,
            input: meerkat_runtime::Input,
        ) -> Result<InputId, String> {
            self.served_at_admission
                .lock()
                .expect("lock")
                .push(self.authority.serving(session));
            self.inner.admit(session, input).await
        }

        async fn admitted_input(
            &self,
            session: &SessionId,
            admission_key: &str,
        ) -> Result<Option<StoredInputState>, String> {
            self.inner.admitted_input(session, admission_key).await
        }
    }

    /// Routes by its runtime owner's hosting claims, as a product host does:
    /// the claim registry for routing, a real grant for a cold claim.
    struct ClaimHost {
        authority: SessionHostingAuthority,
        sink: Arc<ObservingSink>,
        /// Member address resolutions the test moves (a repoint).
        resolutions: Mutex<HashMap<LogicalRuntimeId, AddressResolution>>,
    }

    impl ClaimHost {
        fn new(capability: &HostingCapability) -> Arc<Self> {
            let authority = SessionHostingAuthority::new(capability.clone(), HostingOwner::mint());
            Arc::new(Self {
                sink: Arc::new(ObservingSink {
                    inner: RecordingSink::default(),
                    authority: authority.clone(),
                    served_at_admission: Mutex::new(Vec::new()),
                }),
                authority,
                resolutions: Mutex::new(HashMap::new()),
            })
        }

        fn resolve(&self, address: &LogicalRuntimeId, resolution: AddressResolution) {
            self.resolutions
                .lock()
                .expect("lock")
                .insert(address.clone(), resolution);
        }
    }

    #[async_trait::async_trait]
    impl RuntimeDeliveryHost for ClaimHost {
        async fn delivery_route(&self, session_id: &SessionId) -> Option<meerkat::DeliveryRoute> {
            Some(match self.authority.serving(session_id) {
                SessionServing::HeldHere => meerkat::DeliveryRoute::ServedHere(Arc::new(NoJobs)),
                SessionServing::HeldByAnotherLocalOwner => meerkat::DeliveryRoute::ServedElsewhere,
                SessionServing::NotHeldInThisProcess => {
                    meerkat::DeliveryRoute::Unserved(Arc::new(NoJobs))
                }
            })
        }

        async fn claim_cold_delivery(
            &self,
            session_id: &SessionId,
        ) -> Option<Result<HostingClaim, HostingRefused>> {
            Some(self.authority.grant(session_id))
        }

        async fn continuation_sink(
            &self,
            _session_id: &SessionId,
        ) -> Option<Arc<dyn ContinuationDeliverySink>> {
            Some(self.sink.clone())
        }

        async fn resolve_address(&self, address: &LogicalRuntimeId) -> AddressResolution {
            self.resolutions
                .lock()
                .expect("lock")
                .get(address)
                .cloned()
                .unwrap_or_else(|| meerkat::default_address_resolution(address))
        }
    }

    impl Hosted {
        fn arm(&self, host: Arc<ClaimHost>) -> meerkat::RuntimeDeliveryOwnerHandle {
            self.fixture
                .owner()
                .with_store_sweep(TEST_SWEEP)
                .arm(host)
                .expect("arm")
        }

        async fn submit(&self, session: &SessionId, key: &str) -> meerkat::ContinuationReceipt {
            self.fixture
                .continuations
                .submit(
                    &ContinuationOwner::Session {
                        session_id: session.clone(),
                    },
                    delivery(key, host_producer()),
                    1,
                )
                .await
                .expect("commit")
        }

        /// The row is untouched: no reservation or admission recorded, and
        /// its status still pending with nothing admitted.
        async fn assert_untouched(
            &self,
            session: &SessionId,
            receipt: &meerkat::ContinuationReceipt,
        ) {
            assert_eq!(
                self.fixture
                    .inbox
                    .continuation_admission(
                        &LogicalRuntimeId::new(receipt.address.clone()),
                        &RuntimeDeliveryId::new(receipt.delivery_id.clone()).expect("id"),
                    )
                    .await
                    .expect("admission index"),
                None,
                "no reservation was written for {:?}",
                receipt.key
            );
            let status = self
                .fixture
                .continuations
                .continuation_status(
                    &ContinuationOwner::Session {
                        session_id: session.clone(),
                    },
                    &receipt.key,
                )
                .await
                .expect("status");
            assert!(
                matches!(status, ContinuationStatus::Pending { admitted: None, .. }),
                "{status:?}"
            );
        }

        async fn wait_applied(
            &self,
            session: &SessionId,
            key: &str,
            passes: &mut watch::Receiver<RuntimeDeliveryPass>,
        ) {
            wait_for_status(
                &self.fixture,
                &ContinuationOwner::Session {
                    session_id: session.clone(),
                },
                key,
                passes,
                |status| matches!(status, ContinuationStatus::Applied { .. }),
            )
            .await;
        }
    }

    async fn first_pass(passes: &mut watch::Receiver<RuntimeDeliveryPass>) -> RuntimeDeliveryPass {
        wait_for_pass(passes, "the reconcile pass", |pass| pass.generation >= 1).await
    }

    /// Another runtime owner of this process hosts the session (the route is
    /// ServedElsewhere): both rows stay pending and untouched, the second in
    /// order behind the first. Once that owner releases the session, the cold
    /// owner admits each exactly once, in order, under its own claim.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_continuation_for_a_session_another_local_owner_hosts_waits_untouched_in_order() {
        let hosted = hosted();
        let session = SessionId::new();
        let other_owner =
            SessionHostingAuthority::new(hosted.capability.clone(), HostingOwner::mint());
        let held = other_owner
            .grant(&session)
            .expect("the other owner hosts it");
        let first = hosted.submit(&session, "task-1").await;
        let second = hosted.submit(&session, "task-2").await;

        let host = ClaimHost::new(&hosted.capability);
        let handle = hosted.arm(host.clone());
        let mut passes = handle.subscribe_passes();
        let pass = first_pass(&mut passes).await;
        assert_eq!(pass.awaiting_other_hosts, 1, "{pass:?}");
        assert_eq!(pass.applied, 0, "{pass:?}");
        assert!(host.sink.inner.admissions().is_empty());
        hosted.assert_untouched(&session, &first).await;
        hosted.assert_untouched(&session, &second).await;
        assert_eq!(
            hosted
                .fixture
                .inbox
                .pending_delivery_total()
                .await
                .expect("backlog"),
            2,
            "the later row stays pending behind the first"
        );

        drop(held);
        hosted.wait_applied(&session, "task-2", &mut passes).await;
        let keys: Vec<String> = host
            .sink
            .inner
            .admissions()
            .into_iter()
            .map(|(admitted, key, _)| {
                assert_eq!(admitted, session);
                key
            })
            .collect();
        assert_eq!(keys.len(), 2, "each admitted exactly once: {keys:?}");
        assert!(
            keys[0].contains(&first.delivery_id) && keys[1].contains(&second.delivery_id),
            "in inbox order: {keys:?}"
        );
        assert_eq!(
            host.sink.served_at_admission(),
            vec![SessionServing::HeldHere, SessionServing::HeldHere],
            "admitted only under this owner's claim"
        );
    }

    /// Two processes on one SQLite realm: a peer process hosts the session.
    /// The route is Unserved (nothing in this process holds the claim), this
    /// process is the cold owner, and its claim attempt is refused: no
    /// reservation is written. After the peer dies the cold owner's claim
    /// succeeds and it admits the row exactly once, under that claim.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refused_cold_claim_writes_no_reservation_until_the_peer_process_dies() {
        let hosted = hosted();
        let session = SessionId::new();
        let peer = peer_lock(&meerkat_core::session_hosting::session_hosting_lock_path(
            &hosted.paths.hosting_lock_dir,
            &session,
        ));
        let receipt = hosted.submit(&session, "task-1").await;

        let host = ClaimHost::new(&hosted.capability);
        let handle = hosted.arm(host.clone());
        let mut passes = handle.subscribe_passes();
        let pass = first_pass(&mut passes).await;
        assert!(pass.applies_cold_deliveries, "{pass:?}");
        assert_eq!(pass.awaiting_other_hosts, 1, "{pass:?}");
        assert!(host.sink.inner.admissions().is_empty());
        hosted.assert_untouched(&session, &receipt).await;

        drop(peer);
        hosted.wait_applied(&session, "task-1", &mut passes).await;
        assert_eq!(
            host.sink.inner.admissions().len(),
            1,
            "admitted exactly once"
        );
        assert_eq!(
            host.sink.served_at_admission(),
            vec![SessionServing::HeldHere],
            "admitted under the cold claim taken first"
        );
    }

    /// While another process is the store's cold-delivery owner, a
    /// continuation for a session no process hosts is not this process's to
    /// apply: untouched. When the cold lock is released this process takes it
    /// and admits the row exactly once.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cold_continuation_is_left_to_the_cold_delivery_owner() {
        let hosted = hosted();
        let other_cold_owner = peer_lock(&hosted.paths.cold_delivery_lock);
        let session = SessionId::new();
        let receipt = hosted.submit(&session, "task-1").await;

        let host = ClaimHost::new(&hosted.capability);
        let handle = hosted.arm(host.clone());
        let mut passes = handle.subscribe_passes();
        let pass = first_pass(&mut passes).await;
        assert!(!pass.applies_cold_deliveries, "{pass:?}");
        assert_eq!(pass.awaiting_other_hosts, 1, "{pass:?}");
        assert!(host.sink.inner.admissions().is_empty());
        hosted.assert_untouched(&session, &receipt).await;

        drop(other_cold_owner);
        hosted.wait_applied(&session, "task-1", &mut passes).await;
        assert_eq!(
            host.sink.inner.admissions().len(),
            1,
            "admitted exactly once"
        );
        assert_eq!(
            host.sink.served_at_admission(),
            vec![SessionServing::HeldHere]
        );
    }

    /// The store selected OS-locked claims, but the session's claim is
    /// unavailable (a regular file where the lock directory belongs): the
    /// cold claim is refused typed and nothing is reserved. Once the lock
    /// directory is usable the row is admitted exactly once.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unavailable_session_claim_leaves_the_continuation_untouched() {
        let hosted = hosted();
        let session = SessionId::new();
        std::fs::write(&hosted.paths.hosting_lock_dir, b"not a directory")
            .expect("block the lock dir");
        let receipt = hosted.submit(&session, "task-1").await;

        let host = ClaimHost::new(&hosted.capability);
        assert!(matches!(
            host.authority.grant(&session),
            Err(HostingRefused::Unavailable(_))
        ));
        let handle = hosted.arm(host.clone());
        let mut passes = handle.subscribe_passes();
        let pass = first_pass(&mut passes).await;
        assert!(pass.applies_cold_deliveries, "{pass:?}");
        assert_eq!(pass.awaiting_other_hosts, 1, "{pass:?}");
        assert!(host.sink.inner.admissions().is_empty());
        hosted.assert_untouched(&session, &receipt).await;

        std::fs::remove_file(&hosted.paths.hosting_lock_dir).expect("unblock the lock dir");
        hosted.wait_applied(&session, "task-1", &mut passes).await;
        assert_eq!(
            host.sink.inner.admissions().len(),
            1,
            "admitted exactly once"
        );
        assert_eq!(
            host.sink.served_at_admission(),
            vec![SessionServing::HeldHere]
        );
    }

    /// A member continuation committed while its address resolves to A,
    /// which another runtime owner hosts, then the member repoints to B
    /// without changing its address. Nothing new is committed (the delivery
    /// generation does not move) and A stays hosted elsewhere.
    async fn repointed_member(
        fixture: &Fixture,
        capability: &HostingCapability,
        host: &ClaimHost,
    ) -> (
        LogicalRuntimeId,
        SessionId,
        SessionId,
        SessionHostingAuthority,
        HostingClaim,
    ) {
        let address = meerkat::member_delivery_address("team", team_incarnation(), "lead", 1)
            .expect("address");
        let (a, b) = (SessionId::new(), SessionId::new());
        let elsewhere = SessionHostingAuthority::new(capability.clone(), HostingOwner::mint());
        let a_claim = elsewhere.grant(&a).expect("another owner hosts A");
        host.resolve(&address, AddressResolution::Session(a.clone()));
        let submission = meerkat_runtime::RuntimeDeliverySubmission::new(
            RuntimeDeliveryId::new("continuation:member").expect("id"),
            meerkat_runtime::RuntimeDeliveryKind::Continuation,
            "host:tasks",
            1,
            "task-1",
            serde_json::to_vec(&delivery("task-1", host_producer())).expect("encode"),
        )
        .expect("submission");
        fixture
            .inbox
            .submit(&address, submission)
            .await
            .expect("commit");
        (address, a, b, elsewhere, a_claim)
    }

    /// ADR correction: the store-watch retry of a waiting continuation
    /// re-resolves its address instead of routing the cached session, so the
    /// row progresses through B once B is served here, although A is still
    /// hosted elsewhere and nothing new was committed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_waiting_member_continuation_follows_a_repoint_on_the_store_sweep() {
        let hosted = hosted();
        let host = ClaimHost::new(&hosted.capability);
        let (address, a, b, _elsewhere, _a_claim) =
            repointed_member(&hosted.fixture, &hosted.capability, &host).await;
        let generation = hosted
            .fixture
            .inbox
            .delivery_generation()
            .await
            .expect("generation");
        let handle = hosted.arm(host.clone());
        let mut passes = handle.subscribe_passes();
        let pass = first_pass(&mut passes).await;
        assert_eq!(
            pass.awaiting_other_hosts, 1,
            "A is hosted elsewhere: {pass:?}"
        );
        assert!(host.sink.inner.admissions().is_empty());
        // While A is still resolved, the waiting pass committed nothing.
        // Only then is B made eligible: from there on the owner may admit
        // the row, which legitimately advances the generation.
        assert_eq!(
            hosted
                .fixture
                .inbox
                .delivery_generation()
                .await
                .expect("generation"),
            generation,
            "nothing new was committed while the row waited on A"
        );
        host.resolve(&address, AddressResolution::Session(b.clone()));
        let _b_claim = host.authority.grant(&b).expect("B is served here");
        wait_for_pass(&mut passes, "the row applied through B", |pass| {
            pass.applied > 0 || pass.awaiting_other_hosts == 0
        })
        .await;
        assert_eq!(
            host.sink
                .inner
                .admissions()
                .iter()
                .map(|(session, ..)| session.clone())
                .collect::<Vec<_>>(),
            vec![b.clone()],
            "admitted once, into B, never into A ({a})"
        );
        assert_eq!(
            host.sink.served_at_admission(),
            vec![SessionServing::HeldHere]
        );
        assert_eq!(
            hosted
                .fixture
                .inbox
                .pending_delivery_total()
                .await
                .expect("backlog"),
            0
        );
    }

    /// ADR correction, attachment path: the delivery store is process-local,
    /// so the owner runs no store watcher and no tick can ever retry the row
    /// (the claims the host routes by live in their own OS-locked registry).
    /// Only the attachment wake that serves B (the host's mapping change) can
    /// retry the waiting continuation, with a fresh resolution.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_waiting_member_continuation_follows_a_repoint_on_an_attachment_wake() {
        let fixture = Fixture::new();
        let claims_dir = tempfile::tempdir().expect("tempdir");
        let capability = HostingCapability::OsLock(Arc::new(HostingPaths {
            hosting_lock_dir: claims_dir.path().join("hosting"),
            cold_delivery_lock: claims_dir
                .path()
                .join("delivery")
                .join("cold-delivery.lock"),
            database: None,
        }));
        let host = ClaimHost::new(&capability);
        let (address, a, b, _elsewhere, _a_claim) =
            repointed_member(&fixture, &capability, &host).await;
        let (attachments, attachment_commits) = watch::channel(0_u64);
        let handle = fixture
            .owner()
            .with_attachment_commits(attachment_commits)
            .arm(host.clone())
            .expect("arm");
        let mut passes = handle.subscribe_passes();
        let pass = first_pass(&mut passes).await;
        assert_eq!(
            pass.awaiting_other_hosts, 1,
            "A is hosted elsewhere: {pass:?}"
        );
        assert!(
            pass.cross_process_wake_unavailable.is_none()
                && !pass.blocked_runtimes.contains(&address),
            "a process-local store is unwatched and the row only awaits: {pass:?}"
        );

        host.resolve(&address, AddressResolution::Session(b.clone()));
        let _b_claim = host.authority.grant(&b).expect("B is served here");
        attachments.send_modify(|generation| *generation += 1);
        wait_for_pass(&mut passes, "the row applied through B", |pass| {
            pass.applied > 0 || pass.awaiting_other_hosts == 0
        })
        .await;
        assert_eq!(
            host.sink
                .inner
                .admissions()
                .iter()
                .map(|(session, ..)| session.clone())
                .collect::<Vec<_>>(),
            vec![b.clone()],
            "admitted once, into B, never into A ({a})"
        );
        assert_eq!(
            fixture
                .inbox
                .pending_delivery_total()
                .await
                .expect("backlog"),
            0
        );
    }

    /// The store's kept delivery-generation reader under a delivery
    /// owner. Unix only: the store keeps the reader on unix.
    #[cfg(unix)]
    mod kept_reader {
        use super::*;

        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

        use meerkat_runtime::store::sqlite::delivery_generation_test_hooks as hooks;

        /// A store watched for other processes' commits, with its database
        /// path (the key of the delivery-generation test hooks).
        fn watched_store() -> (Fixture, std::path::PathBuf, HostingCapability) {
            let dir = tempfile::tempdir().expect("tempdir");
            let paths = HostingPaths {
                hosting_lock_dir: dir.path().join("hosting"),
                cold_delivery_lock: dir.path().join("delivery").join("cold-delivery.lock"),
                database: None,
            };
            let store = Arc::new(
                SqliteRuntimeStore::new(dir.path().join("runtime.sqlite3"))
                    .expect("open store")
                    .with_hosting_paths(paths),
            );
            let path = store.path().to_path_buf();
            let capability = RuntimeStore::hosting_capability(store.as_ref());
            assert!(capability.is_cross_process());
            (Fixture::on(store, Some(dir)), path, capability)
        }

        /// Counts the kept reads of the database at `path`: reads on a kept
        /// connection, not its verified opens.
        fn count_kept_reads(path: &std::path::Path) -> Arc<AtomicU64> {
            let reads = Arc::new(AtomicU64::new(0));
            hooks::before_kept_read_guard(path, {
                let reads = Arc::clone(&reads);
                move || {
                    reads.fetch_add(1, Ordering::SeqCst);
                }
            });
            reads
        }

        /// Whether maintenance gets the database at `path` now: no kept
        /// connection holds its presence lock. The fence is released at once.
        fn maintenance_gets_the_database(path: &std::path::Path) -> bool {
            meerkat_sqlite::ExclusiveFence::try_acquire(path)
                .expect("io")
                .is_some()
        }

        /// A quiet watched store ticks its owner every sweep, and
        /// every tick reads the delivery generation. Through the store's kept
        /// reader the owner opens the database once and reads on the kept
        /// connection at its ticks; through the fresh-open path (the one
        /// every tick took before) it opens it on every tick.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_quiet_store_ticks_on_one_kept_open_instead_of_one_open_per_tick() {
            const SWEEP: Duration = Duration::from_millis(5);

            let (fresh, fresh_path, capability) = watched_store();
            hooks::force_fresh_reads(&fresh_path);
            let handle = fresh
                .owner()
                .with_store_sweep(SWEEP)
                .arm(ClaimHost::new(&capability))
                .expect("arm the fresh owner");
            let started = std::time::Instant::now();
            while hooks::verified_opens(&fresh_path) < 100 {
                assert!(
                    started.elapsed() < Duration::from_secs(60),
                    "100 store ticks never came"
                );
                tokio::time::sleep(SWEEP).await;
            }
            let hundred_ticks = started.elapsed();
            drop(handle);

            let (kept, kept_path, capability) = watched_store();
            let kept_reads = count_kept_reads(&kept_path);
            let handle = kept
                .owner()
                .with_store_sweep(SWEEP)
                .arm(ClaimHost::new(&capability))
                .expect("arm the kept owner");
            tokio::time::sleep(hundred_ticks).await;
            assert_eq!(
                hooks::verified_opens(&kept_path),
                1,
                "as many ticks as the fresh owner took for 100 opens cost one open"
            );
            let reads = kept_reads.load(Ordering::SeqCst);
            assert!(
                reads >= 50,
                "the kept owner reads at its ticks: {reads} kept reads in the time the fresh \
                 owner opened 100 times"
            );
            drop(handle);
        }

        /// The kept reader's connection closes once the store has been
        /// quiet for `KEPT_DELIVERY_READER_IDLE`, and with it the presence
        /// lock the maintenance fence waits for (paused time).
        #[tokio::test(start_paused = true)]
        async fn a_quiet_kept_reader_closes_at_its_idle_bound() {
            let (fixture, path, capability) = watched_store();
            // No sweep inside the test: the arming pass is the only read.
            let handle = fixture
                .owner()
                .with_store_sweep(Duration::from_secs(3600))
                .arm(ClaimHost::new(&capability))
                .expect("arm");
            let mut passes = handle.subscribe_passes();
            first_pass(&mut passes).await;
            assert!(
                !maintenance_gets_the_database(&path),
                "the arming read keeps its connection, so maintenance waits"
            );

            tokio::time::advance(
                meerkat_runtime::KEPT_DELIVERY_READER_IDLE
                    .saturating_sub(Duration::from_millis(50)),
            )
            .await;
            tokio::task::yield_now().await;
            assert!(
                !maintenance_gets_the_database(&path),
                "kept until the idle bound"
            );

            tokio::time::advance(Duration::from_millis(100)).await;
            let mut released = false;
            for _ in 0..1000 {
                tokio::task::yield_now().await;
                released = maintenance_gets_the_database(&path);
                if released {
                    break;
                }
            }
            assert!(
                released,
                "the quiet reader closed at its idle bound, and maintenance gets the database"
            );
            drop(handle);
        }

        /// Holds the host calls made through it, once armed, until the test
        /// opens it, and says when it holds one.
        struct HostGate {
            armed: AtomicBool,
            held: watch::Sender<bool>,
            open: watch::Sender<bool>,
        }

        impl HostGate {
            fn new() -> Arc<Self> {
                Arc::new(Self {
                    armed: AtomicBool::new(false),
                    held: watch::Sender::new(false),
                    open: watch::Sender::new(false),
                })
            }

            /// Hold the calls from now on.
            fn arm(&self) {
                self.armed.store(true, Ordering::SeqCst);
            }

            /// A host call through the gate: held while it is armed and
            /// closed.
            async fn pass(&self) {
                if !self.armed.load(Ordering::SeqCst) {
                    return;
                }
                self.held.send_replace(true);
                // The gate owns the sender, so this wait ends when it opens.
                let mut open = self.open.subscribe();
                let _ = open.wait_for(|open| *open).await;
            }

            /// Wait until the gate holds a call.
            async fn wait_held(&self, what: &str) {
                let mut held = self.held.subscribe();
                tokio::time::timeout(EVENT_GUARD, held.wait_for(|held| *held))
                    .await
                    .unwrap_or_else(|_| panic!("never held: {what}"))
                    .expect("the gate owns the sender");
            }

            fn open(&self) {
                self.open.send_replace(true);
            }
        }

        /// Admits through [`ObservingSink`] once its gate lets the admission
        /// on.
        struct GatedSink {
            inner: Arc<ObservingSink>,
            gate: Arc<HostGate>,
        }

        #[async_trait::async_trait]
        impl ContinuationDeliverySink for GatedSink {
            async fn admit(
                &self,
                session: &SessionId,
                input: meerkat_runtime::Input,
            ) -> Result<InputId, String> {
                self.gate.pass().await;
                self.inner.admit(session, input).await
            }

            async fn admitted_input(
                &self,
                session: &SessionId,
                admission_key: &str,
            ) -> Result<Option<StoredInputState>, String> {
                self.inner.admitted_input(session, admission_key).await
            }
        }

        /// Routes and claims as [`ClaimHost`] does, and puts a gate in front
        /// of each host call that can wait: an admission, a page of owed
        /// continuations and an address resolution.
        struct GatedHost {
            claims: Arc<ClaimHost>,
            sink: Arc<GatedSink>,
            owed: Arc<HostGate>,
            resolution: Arc<HostGate>,
            /// Changes of the owing source; `None`: the host owes nothing.
            owed_changes: Option<watch::Receiver<u64>>,
        }

        impl GatedHost {
            fn new(
                capability: &HostingCapability,
                owed_changes: Option<watch::Receiver<u64>>,
            ) -> Arc<Self> {
                let claims = ClaimHost::new(capability);
                Arc::new(Self {
                    sink: Arc::new(GatedSink {
                        inner: claims.sink.clone(),
                        gate: HostGate::new(),
                    }),
                    claims,
                    owed: HostGate::new(),
                    resolution: HostGate::new(),
                    owed_changes,
                })
            }
        }

        #[async_trait::async_trait]
        impl RuntimeDeliveryHost for GatedHost {
            async fn delivery_route(
                &self,
                session_id: &SessionId,
            ) -> Option<meerkat::DeliveryRoute> {
                self.claims.delivery_route(session_id).await
            }

            async fn claim_cold_delivery(
                &self,
                session_id: &SessionId,
            ) -> Option<Result<HostingClaim, HostingRefused>> {
                self.claims.claim_cold_delivery(session_id).await
            }

            async fn continuation_sink(
                &self,
                _session_id: &SessionId,
            ) -> Option<Arc<dyn ContinuationDeliverySink>> {
                Some(self.sink.clone())
            }

            async fn resolve_address(&self, address: &LogicalRuntimeId) -> AddressResolution {
                self.resolution.pass().await;
                self.claims.resolve_address(address).await
            }

            fn projects_owed_continuations(&self) -> bool {
                self.owed_changes.is_some()
            }

            fn subscribe_owed_continuation_changes(&self) -> Option<watch::Receiver<u64>> {
                self.owed_changes.clone()
            }

            async fn project_owed_continuations(
                &self,
                _after: Option<&meerkat::OwedContinuationCursor>,
                _limit: usize,
            ) -> meerkat::OwedContinuationProjection {
                self.owed.pass().await;
                meerkat::OwedContinuationProjection::default()
            }
        }

        /// Delivering calls into the host, which can wait, so an owner
        /// closes its kept reader's connection before it delivers, and
        /// maintenance never waits on a delivery. The arming pass reads the
        /// delivery generation through the kept reader, then delivers a
        /// continuation whose admission the test holds: while it is held,
        /// the fence gets the database.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_delivering_owner_holds_no_kept_connection_while_the_host_admits() {
            let (fixture, path, capability) = watched_store();
            let session = SessionId::new();
            let owner = ContinuationOwner::Session {
                session_id: session.clone(),
            };
            fixture
                .continuations
                .submit(&owner, delivery("task-1", host_producer()), 1)
                .await
                .expect("commit");

            let host = GatedHost::new(&capability, None);
            host.sink.gate.arm();
            let handle = fixture
                .owner()
                .with_store_sweep(Duration::from_secs(3600))
                .arm(host.clone())
                .expect("arm");
            let mut passes = handle.subscribe_passes();

            host.sink
                .gate
                .wait_held("the arming pass reaches the admission")
                .await;
            assert!(
                maintenance_gets_the_database(&path),
                "a delivering owner keeps no connection, so maintenance gets the database"
            );

            host.sink.gate.open();
            wait_for_status(&fixture, &owner, "task-1", &mut passes, |status| {
                matches!(status, ContinuationStatus::Applied { .. })
            })
            .await;
            drop(handle);
        }

        /// Projecting a page of owed continuations calls into the
        /// host, which can wait, so an owner closes its kept reader's
        /// connection before it projects. The store sweep reads on the kept
        /// connection until a change of the owing source starts a round,
        /// whose page the test holds: while it is held, the fence gets the
        /// database.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn an_owner_projecting_owed_continuations_holds_no_kept_connection() {
            let (fixture, path, capability) = watched_store();
            let (owed_source, owed_changes) = watch::channel(0_u64);
            let host = GatedHost::new(&capability, Some(owed_changes));
            let kept_reads = count_kept_reads(&path);
            let handle = fixture
                .owner()
                .with_store_sweep(TEST_SWEEP)
                .arm(host.clone())
                .expect("arm");
            let mut passes = handle.subscribe_passes();
            let armed = first_pass(&mut passes).await;
            let started = std::time::Instant::now();
            while kept_reads.load(Ordering::SeqCst) == 0 {
                assert!(
                    started.elapsed() < EVENT_GUARD,
                    "the sweep never read on the kept connection"
                );
                tokio::time::sleep(TEST_SWEEP).await;
            }

            host.owed.arm();
            owed_source.send_modify(|revision| *revision += 1);
            host.owed
                .wait_held("a source change starts a round of owed continuations")
                .await;
            assert!(
                maintenance_gets_the_database(&path),
                "an owner projecting owed continuations keeps no connection, so maintenance \
                 gets the database"
            );

            host.owed.open();
            wait_for_pass(&mut passes, "the pass of the held round", |pass| {
                pass.generation > armed.generation
            })
            .await;
            drop(handle);
        }

        /// Resolving a continuation address calls into the host, which
        /// can wait (it can revive a member), so an owner closes its kept
        /// reader's connection before it resolves the address of a member
        /// continuation waiting on another host. The test holds the
        /// resolution a store tick makes: while it is held, the fence gets
        /// the database. Then the member repoints to a session served here,
        /// and the row applies through it.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn an_owner_resolving_a_waiting_address_holds_no_kept_connection() {
            let (fixture, path, capability) = watched_store();
            let host = GatedHost::new(&capability, None);
            let (address, a, b, _elsewhere, _a_claim) =
                repointed_member(&fixture, &capability, &host.claims).await;
            let handle = fixture
                .owner()
                .with_store_sweep(TEST_SWEEP)
                .arm(host.clone())
                .expect("arm");
            let mut passes = handle.subscribe_passes();
            let pass = first_pass(&mut passes).await;
            assert_eq!(
                pass.awaiting_other_hosts, 1,
                "A is hosted elsewhere: {pass:?}"
            );

            host.resolution.arm();
            host.resolution
                .wait_held("a store tick resolves the waiting address again")
                .await;
            assert!(
                maintenance_gets_the_database(&path),
                "an owner resolving an address keeps no connection, so maintenance gets the \
                 database"
            );

            // B is served here and the member repoints to it while the
            // resolution is held, so the held call resolves B.
            let _b_claim = host.claims.authority.grant(&b).expect("B is served here");
            host.claims
                .resolve(&address, AddressResolution::Session(b.clone()));
            host.resolution.open();
            wait_for_pass(&mut passes, "the row applied through B", |pass| {
                pass.applied > 0 || pass.awaiting_other_hosts == 0
            })
            .await;
            assert_eq!(
                host.claims
                    .sink
                    .inner
                    .admissions()
                    .iter()
                    .map(|(session, ..)| session.clone())
                    .collect::<Vec<_>>(),
                vec![b.clone()],
                "admitted once, into B, never into A ({a})"
            );
            drop(handle);
        }
    }
}

/// Invariant behind continuation routing (#1813): the applier routes and
/// admits a continuation into the runtime's page session, the serving session
/// of its FIRST pending row. A job row and a continuation row share a
/// runtime only on a session address `rt:session:{id}`, where the job's
/// provenance session and `resolve_address` are the same session; member
/// addresses carry no job rows. Here a projected job row (origin session S)
/// heads S's address and the continuation behind it is admitted into S.
#[tokio::test]
async fn a_continuation_behind_a_job_row_on_a_session_address_is_admitted_into_that_session() {
    let fixture = Fixture::new();
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let address = LogicalRuntimeId::for_session(&session);
    assert_eq!(
        meerkat::default_address_resolution(&address),
        AddressResolution::Session(session.clone())
    );

    // A completed job whose origin is S and whose notification S subscribed.
    let job_store = Arc::new(MemoryDetachedJobStore::new());
    let jobs = meerkat::DetachedJobService::new(job_store.clone());
    let receipt = jobs
        .submit(meerkat::JobSpec::new(
            "default",
            session.clone(),
            meerkat::ExecutionIntentId::new(),
            meerkat::InteractionLineageId::new(),
            meerkat::ToolIdentity::new("shell", "1").expect("tool"),
            meerkat::RunnerIdentity::new("durable-shell", "1").expect("runner"),
            meerkat::RestartClass::Adoptable,
            meerkat::CanonicalArgumentsHash::new("hash-head").expect("hash"),
            meerkat::JobSubmissionKey::new("head").expect("submission key"),
        ))
        .await
        .expect("submit");
    jobs.subscribe(
        &receipt.job_id,
        meerkat::JobSubscription::new(
            meerkat::JobSubscriptionId::new("to-origin").expect("subscription id"),
            session.clone(),
            meerkat::JobDeliveryKind::Notification,
        ),
    )
    .await
    .expect("subscribe");
    let claim = jobs
        .claim_attempt(
            &receipt.job_id,
            meerkat::AttemptClaim::new(
                meerkat::WorkerId::new("worker").expect("worker"),
                1,
                100,
                meerkat::RunnerHandleRef::new("runner-handle").expect("handle"),
            ),
        )
        .await
        .expect("claim");
    jobs.complete_attempt(
        &receipt.job_id,
        (&claim).into(),
        2,
        Some(meerkat::JobResultRef::new("result").expect("result")),
    )
    .await
    .expect("complete");
    // The job row is projected first, so it heads S's runtime.
    let projected = meerkat::JobOutboxProjector::new(job_store.clone(), fixture.inbox.clone())
        .project_pending(16)
        .await
        .expect("project");
    assert_eq!(projected.projected.len(), 1, "{projected:?}");
    let continuation = fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    assert_eq!(
        continuation.address, address.0,
        "one runtime carries both rows"
    );

    let handle = RuntimeDeliveryOwner::new(job_store, fixture.inbox.clone())
        .arm(Host::new(Some(sink.clone())))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    wait_for_status(&fixture, &owner, "task-1", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;
    let admissions = sink.admissions();
    assert_eq!(admissions.len(), 1, "{admissions:?}");
    assert_eq!(
        admissions[0].0, session,
        "the page session (from the job row) is the address's session"
    );
    assert_eq!(
        fixture
            .inbox
            .pending_delivery_total()
            .await
            .expect("backlog"),
        0,
        "the job row ahead of it was applied too"
    );
}

/// The retained admission keeps a typed readiness cause: not admitted yet and
/// retried, with the cause the runtime named. A terminal refusal stays a
/// refusal.
#[test]
fn a_retained_admission_keeps_a_typed_readiness_cause() {
    use meerkat_core::authorization::ControllerReadinessFailure;
    assert_eq!(
        meerkat::retained_admission_outcome(Err(
            meerkat_runtime::RuntimeDriverError::ControllerReadinessUnavailable {
                reason: ControllerReadinessFailure::OriginalAdmissionPending,
            }
        )),
        Err(ContinuationAdmitError::NotReady(
            ControllerReadinessFailure::OriginalAdmissionPending
        ))
    );
    assert_eq!(
        meerkat::retained_admission_outcome(Err(
            meerkat_runtime::RuntimeDriverError::RetainedResumeRefused {
                reason: meerkat_runtime::retained_work::RetainedResumeRefusal::AuthorityDenied,
            }
        )),
        Err(ContinuationAdmitError::Refused(
            meerkat_runtime::RuntimeDeliveryRefusalReason::AuthorityDenied
        ))
    );
}

async fn status_of(fixture: &Fixture, owner: &ContinuationOwner, key: &str) -> ContinuationStatus {
    fixture
        .continuations
        .continuation_status(owner, &ContinuationKey::new(key).expect("key"))
        .await
        .expect("status")
}

/// CONTROL-stuck (row part): a retained completion whose original's admission
/// its source owner has not settled is not ready, never refused. Its row stays
/// pending and is reported blocked with the typed cause, and the row behind it
/// in the same runtime waits too. Once the owner withdraws the original (the
/// sink then refuses the resume for good) and the host announces the
/// settlement, the row settles refused and the row behind it proceeds.
#[tokio::test]
async fn a_pending_admission_blocks_its_row_with_the_typed_cause_until_announced() {
    use meerkat_core::authorization::ControllerReadinessFailure;
    let fixture = Fixture::new();
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let identity = work_identity("pending");
    let jobs = FakeJobs::holding("job-1", committed_job(&session, &identity));
    retained_fork_completion(&fixture, &session, &jobs).await;
    fixture
        .continuations
        .submit(&owner, delivery("after", host_producer()), 1)
        .await
        .expect("commit the row behind it");
    let sink = Arc::new(RecordingSink::governed());
    *sink.resume_verdict.lock().expect("lock") = Some(ContinuationAdmitError::NotReady(
        ControllerReadinessFailure::OriginalAdmissionPending,
    ));
    let bindings = Arc::new(meerkat::ContinuationHostBindings::default());
    let _binding = bindings
        .bind(Arc::new(SessionAddressResolver), jobs.clone())
        .expect("bind");
    let handle = fixture
        .owner()
        .with_source_changes(bindings.subscribe_source_changes())
        .arm(Arc::new(BindingsHost {
            bindings: Arc::clone(&bindings),
            sink: sink.clone(),
        }))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let blocked = wait_for_pass(&mut passes, "blocked with its typed cause", |pass| {
        pass.failures
            .iter()
            .any(|line| line.contains("NotReady(OriginalAdmissionPending)"))
    })
    .await;
    assert_eq!(blocked.refused, 0, "never refused while pending");
    assert!(matches!(
        status_of(&fixture, &owner, "fork_off:job-1").await,
        ContinuationStatus::Pending { .. }
    ));
    assert!(
        matches!(
            status_of(&fixture, &owner, "after").await,
            ContinuationStatus::Pending { .. }
        ),
        "the row behind it waits"
    );
    assert!(sink.resumes().is_empty());

    *sink.resume_verdict.lock().expect("lock") = Some(ContinuationAdmitError::Refused(
        meerkat_runtime::RuntimeDeliveryRefusalReason::AuthorityDenied,
    ));
    bindings.source_custody_changed();
    wait_for_pass(&mut passes, "settled after the announcement", |pass| {
        pass.refused >= 2
    })
    .await;
    assert!(matches!(
        status_of(&fixture, &owner, "fork_off:job-1").await,
        ContinuationStatus::Refused {
            reason: meerkat_runtime::RuntimeDeliveryRefusalReason::AuthorityDenied,
            ..
        }
    ));
    assert!(
        matches!(
            status_of(&fixture, &owner, "after").await,
            ContinuationStatus::Refused { .. }
        ),
        "the row behind it proceeds"
    );
}

/// The incarnation of the mob `team` these tests address members of.
fn team_incarnation() -> meerkat_core::service::MobIncarnationId {
    "6a1d3c58-2f0e-4b7a-9c41-8e5d2b0f7a13"
        .parse()
        .expect("canonical mob incarnation id")
}

// ---------------------------------------------------------------------------
// Retired addresses: a reservation left on an address that retired is
// settled from the reserved session's ledger, and a stranded answer is
// final: no admission follows it.
// ---------------------------------------------------------------------------

/// Retirements the test records, read by the delivery host and by the status
/// read's resolver alike, as one host's records are.
#[derive(Default)]
struct Retirements(Mutex<std::collections::HashSet<LogicalRuntimeId>>);

impl Retirements {
    fn retire(&self, address: &LogicalRuntimeId) {
        self.0.lock().expect("lock").insert(address.clone());
    }

    fn retired(&self, address: &LogicalRuntimeId) -> bool {
        self.0.lock().expect("lock").contains(address)
    }
}

/// The status read's resolver: session addresses, retired once recorded.
/// `before_observe` runs inside the next observation, before it answers.
struct RetiringResolver {
    retirements: Arc<Retirements>,
    before_observe: Mutex<Option<futures::future::BoxFuture<'static, ()>>>,
}

#[async_trait::async_trait]
impl meerkat::ContinuationAddressResolver for RetiringResolver {
    async fn current_address(
        &self,
        owner: &ContinuationOwner,
    ) -> Result<Option<LogicalRuntimeId>, String> {
        meerkat::ContinuationAddressResolver::current_address(&SessionAddressResolver, owner).await
    }

    async fn resolve_address(
        &self,
        address: &LogicalRuntimeId,
    ) -> Result<AddressResolution, String> {
        if self.retirements.retired(address) {
            return Ok(AddressResolution::Retired);
        }
        meerkat::ContinuationAddressResolver::resolve_address(&SessionAddressResolver, address)
            .await
    }

    async fn observe_address(
        &self,
        address: &LogicalRuntimeId,
    ) -> Result<meerkat::AddressObservation, String> {
        let hook = self.before_observe.lock().expect("lock").take();
        if let Some(hook) = hook {
            hook.await;
        }
        Ok(if self.retirements.retired(address) {
            meerkat::AddressObservation::Retired
        } else {
            meerkat::AddressObservation::MayServe
        })
    }
}

/// A delivery host over the same retirements: a retired address resolves
/// Retired. A session in `elsewhere` is another runtime owner's. The next
/// route asked can be parked: the pass has resolved its address then, and
/// has reserved nothing yet.
struct RetiringHost {
    sink: Arc<RecordingSink>,
    retirements: Arc<Retirements>,
    /// Addresses served by another session than their own (a repoint).
    served_by: Mutex<HashMap<LogicalRuntimeId, SessionId>>,
    elsewhere: Mutex<std::collections::HashSet<SessionId>>,
    held_route: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
}

impl RetiringHost {
    fn new(sink: Arc<RecordingSink>, retirements: Arc<Retirements>) -> Arc<Self> {
        Arc::new(Self {
            sink,
            retirements,
            served_by: Mutex::new(HashMap::new()),
            elsewhere: Mutex::new(std::collections::HashSet::new()),
            held_route: Mutex::new(None),
        })
    }

    fn serve(&self, address: &LogicalRuntimeId, session: SessionId) {
        self.served_by
            .lock()
            .expect("lock")
            .insert(address.clone(), session);
    }

    /// Park the next route asked. The first receiver fires when it parks;
    /// the route answers once the sender fires.
    fn hold_next_route(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        *self.held_route.lock().expect("lock") = Some((entered_tx, release_rx));
        (entered_rx, release_tx)
    }
}

#[async_trait::async_trait]
impl RuntimeDeliveryHost for RetiringHost {
    async fn delivery_route(&self, session_id: &SessionId) -> Option<meerkat::DeliveryRoute> {
        let held = self.held_route.lock().expect("lock").take();
        if let Some((entered, release)) = held {
            let _ = entered.send(());
            let _ = release.await;
        }
        if self.elsewhere.lock().expect("lock").contains(session_id) {
            return Some(meerkat::DeliveryRoute::ServedElsewhere);
        }
        Some(meerkat::DeliveryRoute::ServedHere(Arc::new(NoJobs)))
    }

    async fn claim_cold_delivery(
        &self,
        session_id: &SessionId,
    ) -> Option<Result<meerkat_runtime::HostingClaim, meerkat_runtime::HostingRefused>> {
        Some(meerkat_runtime::grant_session_hosting(
            &meerkat_runtime::HostingCapability::ProcessLocal,
            &meerkat_runtime::HostingOwner::mint(),
            session_id,
        ))
    }

    async fn resolve_address(&self, address: &LogicalRuntimeId) -> AddressResolution {
        if self.retirements.retired(address) {
            return AddressResolution::Retired;
        }
        if let Some(session) = self.served_by.lock().expect("lock").get(address) {
            return AddressResolution::Session(session.clone());
        }
        meerkat::default_address_resolution(address)
    }

    async fn continuation_sink(
        &self,
        _session_id: &SessionId,
    ) -> Option<Arc<dyn ContinuationDeliverySink>> {
        Some(self.sink.clone() as Arc<dyn ContinuationDeliverySink>)
    }
}

/// A fixture whose status read observes `retirements` through `resolver`.
struct RetiringFixture {
    fixture: Fixture,
    retirements: Arc<Retirements>,
    resolver: Arc<RetiringResolver>,
}

impl RetiringFixture {
    fn on(store: Arc<dyn RuntimeStore>, dir: Option<tempfile::TempDir>) -> Self {
        let retirements = Arc::new(Retirements::default());
        let resolver = Arc::new(RetiringResolver {
            retirements: Arc::clone(&retirements),
            before_observe: Mutex::new(None),
        });
        let inbox = RuntimeDeliveryInbox::new(store);
        Self {
            fixture: Fixture {
                continuations: ContinuationOwnerService::new(
                    inbox.clone(),
                    resolver.clone(),
                    Arc::new(meerkat_runtime::MeerkatMachine::ephemeral()),
                ),
                inbox,
                _dir: dir,
            },
            retirements,
            resolver,
        }
    }

    fn memory() -> Self {
        Self::on(Arc::new(InMemoryRuntimeStore::new()), None)
    }

    #[cfg(feature = "sqlite-store")]
    fn sqlite() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = meerkat_runtime::SqliteRuntimeStore::new(dir.path().join("runtime.sqlite3"))
            .expect("open sqlite runtime store");
        Self::on(Arc::new(store), Some(dir))
    }

    async fn status(&self, owner: &ContinuationOwner, key: &str) -> ContinuationStatus {
        self.fixture
            .continuations
            .continuation_status(owner, &ContinuationKey::new(key).expect("key"))
            .await
            .expect("status")
    }

    async fn admission_entry(
        &self,
        address: &LogicalRuntimeId,
        delivery_id: &str,
    ) -> Option<ContinuationAdmission> {
        self.fixture
            .inbox
            .continuation_admission(
                address,
                &RuntimeDeliveryId::new(delivery_id.to_string()).expect("id"),
            )
            .await
            .expect("admission index read")
    }
}

/// Wait for the owner's first pass, its reconcile, which visits every
/// runtime with a pending row.
async fn wait_for_reconcile(passes: &mut watch::Receiver<RuntimeDeliveryPass>) {
    wait_for_pass(passes, "the reconcile pass", |pass| pass.generation >= 1).await;
}

/// A drain resolved the address, then the address retired and a status read
/// answered stranded. The drain, released, must admit nothing, so a resend
/// under a new key is the only delivery.
#[tokio::test]
async fn a_drain_that_resolved_before_the_retire_admits_nothing_after_a_stranded_answer() {
    let cut = RetiringFixture::memory();
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let receipt = cut
        .fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());

    let host = RetiringHost::new(sink.clone(), Arc::clone(&cut.retirements));
    let (entered, release) = host.hold_next_route();
    let handle = cut.fixture.owner().arm(host).expect("arm");
    let mut passes = handle.subscribe_passes();
    tokio::time::timeout(EVENT_GUARD, entered)
        .await
        .expect("the drain reached the session's route")
        .expect("route hold");

    cut.retirements.retire(&address);
    let stranded = cut.status(&owner, "task-1").await;
    assert!(
        matches!(stranded, ContinuationStatus::Stranded { .. }),
        "never admitted, and the address retired: {stranded:?}"
    );

    release.send(()).expect("release the drain");
    wait_for_reconcile(&mut passes).await;
    assert!(
        sink.admissions().is_empty(),
        "nothing is admitted after a stranded answer: {:?}",
        sink.admissions()
    );
    let still = cut.status(&owner, "task-1").await;
    assert!(
        matches!(still, ContinuationStatus::Stranded { .. }),
        "a stranded continuation stays stranded: {still:?}"
    );
}

/// The same through a repoint: a crash left a reservation in s1, and the
/// pass resolved the address to s2. The address retires while the drain is
/// held. Released, the drain repoints to s2, observes the retire and
/// releases the repointed reservation: nothing is admitted.
#[tokio::test]
async fn a_drain_that_repoints_after_the_retire_releases_the_repointed_reservation() {
    let cut = RetiringFixture::memory();
    let sink = Arc::new(RecordingSink::default());
    let (s1, s2) = (SessionId::new(), SessionId::new());
    let owner = ContinuationOwner::Session {
        session_id: s1.clone(),
    };
    let receipt = cut
        .fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    cut.fixture
        .admission(
            &address,
            &receipt.delivery_id,
            ContinuationAdmissionTransition::Reserve {
                session_id: s1.clone(),
                input_id: InputId::new(),
            },
        )
        .await;

    let host = RetiringHost::new(sink.clone(), Arc::clone(&cut.retirements));
    host.serve(&address, s2.clone());
    let (entered, release) = host.hold_next_route();
    let handle = cut.fixture.owner().arm(host).expect("arm");
    let mut passes = handle.subscribe_passes();
    tokio::time::timeout(EVENT_GUARD, entered)
        .await
        .expect("the drain reached the serving session's route")
        .expect("route hold");

    cut.retirements.retire(&address);
    let reserved = cut.status(&owner, "task-1").await;
    assert!(
        matches!(reserved, ContinuationStatus::Pending { admitted: None, .. }),
        "a reservation exists, so the read is not stranded: {reserved:?}"
    );

    release.send(()).expect("release the drain");
    wait_for_reconcile(&mut passes).await;
    assert!(
        sink.admissions().is_empty(),
        "nothing is admitted after the retire: {:?}",
        sink.admissions()
    );
    assert_eq!(
        cut.admission_entry(&address, &receipt.delivery_id).await,
        None,
        "the repointed reservation is released"
    );
    let stranded = cut.status(&owner, "task-1").await;
    assert!(
        matches!(stranded, ContinuationStatus::Stranded { .. }),
        "never admitted, and the address retired: {stranded:?}"
    );
}

/// A reservation the drain left before a crash, whose input never reached
/// the reserved session, on an address that retired since: the owner settles
/// it from that session's ledger, and the continuation reads stranded.
async fn a_reservation_left_on_a_retired_address_without_its_input_reads_stranded(
    cut: RetiringFixture,
) {
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let receipt = cut
        .fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    cut.fixture
        .admission(
            &address,
            &receipt.delivery_id,
            ContinuationAdmissionTransition::Reserve {
                session_id: session.clone(),
                input_id: InputId::new(),
            },
        )
        .await;
    cut.retirements.retire(&address);

    let handle = cut
        .fixture
        .owner()
        .arm(RetiringHost::new(
            sink.clone(),
            Arc::clone(&cut.retirements),
        ))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    wait_for_reconcile(&mut passes).await;

    let status = cut.status(&owner, "task-1").await;
    assert!(
        matches!(status, ContinuationStatus::Stranded { .. }),
        "the reserved session never held the input: {status:?}"
    );
    assert!(sink.admissions().is_empty(), "nothing is admitted");
    assert_eq!(
        cut.admission_entry(&address, &receipt.delivery_id).await,
        None,
        "the reservation is released"
    );
}

/// As above, but the input reached the reserved session before the crash:
/// the owner records that admission, and the continuation reads admitted.
async fn a_reservation_left_on_a_retired_address_with_its_input_reads_admitted(
    cut: RetiringFixture,
) {
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let receipt = cut
        .fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    let reserved = InputId::new();
    cut.fixture
        .admission(
            &address,
            &receipt.delivery_id,
            ContinuationAdmissionTransition::Reserve {
                session_id: session.clone(),
                input_id: reserved.clone(),
            },
        )
        .await;
    sink.already_holds(
        &session,
        &continuation_input(reserved.clone(), &receipt.delivery_id, "result"),
    );
    cut.retirements.retire(&address);

    let handle = cut
        .fixture
        .owner()
        .arm(RetiringHost::new(
            sink.clone(),
            Arc::clone(&cut.retirements),
        ))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    wait_for_reconcile(&mut passes).await;

    let status = cut.status(&owner, "task-1").await;
    assert_eq!(
        status,
        ContinuationStatus::Pending {
            receipt,
            sequence: 1,
            admitted: Some((session, reserved)),
        },
        "the reserved session holds the input"
    );
    assert!(sink.admissions().is_empty(), "nothing is admitted again");
}

/// As above, but the reserved session holds ANOTHER input under the
/// admission key: never this delivery by key alone. The owner keeps the
/// reservation, reports the conflict in its pass, and the continuation stays
/// pending; nothing is applied.
async fn a_reservation_left_on_a_retired_address_beside_another_input_stays_pending(
    cut: RetiringFixture,
) {
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let receipt = cut
        .fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    let reserved = InputId::new();
    cut.fixture
        .admission(
            &address,
            &receipt.delivery_id,
            ContinuationAdmissionTransition::Reserve {
                session_id: session.clone(),
                input_id: reserved.clone(),
            },
        )
        .await;
    sink.holds_legacy_row(&session, &receipt.delivery_id);
    cut.retirements.retire(&address);

    let handle = cut
        .fixture
        .owner()
        .arm(RetiringHost::new(
            sink.clone(),
            Arc::clone(&cut.retirements),
        ))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let pass = wait_for_pass(&mut passes, "the reconcile pass", |pass| {
        pass.generation >= 1
    })
    .await;
    assert!(
        pass.failures
            .iter()
            .any(|failure| failure.contains(&receipt.delivery_id)),
        "the conflict is reported: {:?}",
        pass.failures
    );

    let status = cut.status(&owner, "task-1").await;
    assert!(
        matches!(status, ContinuationStatus::Pending { admitted: None, .. }),
        "another input is never this delivery: {status:?}"
    );
    assert_eq!(
        cut.admission_entry(&address, &receipt.delivery_id).await,
        Some(ContinuationAdmission::Reserved {
            session_id: session,
            input_id: reserved,
        }),
        "the reservation stays"
    );
    assert!(sink.admissions().is_empty(), "nothing is admitted");
}

macro_rules! retired_cut {
    ($case:ident) => {
        mod $case {
            #[tokio::test]
            async fn memory() {
                super::$case(super::RetiringFixture::memory()).await;
            }

            #[cfg(feature = "sqlite-store")]
            #[tokio::test]
            async fn sqlite() {
                super::$case(super::RetiringFixture::sqlite()).await;
            }
        }
    };
}

retired_cut!(a_reservation_left_on_a_retired_address_without_its_input_reads_stranded);
retired_cut!(a_reservation_left_on_a_retired_address_with_its_input_reads_admitted);
retired_cut!(a_reservation_left_on_a_retired_address_beside_another_input_stays_pending);

/// A reservation that lands between a status read's entry read and its
/// observation of the retirement is seen: the read answers pending, never
/// stranded. The owner then settles it from the reserved session's ledger.
#[tokio::test]
async fn a_reservation_made_while_the_status_observes_the_retire_reads_pending() {
    let cut = RetiringFixture::memory();
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let receipt = cut
        .fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    cut.retirements.retire(&address);

    let inbox = cut.fixture.inbox.clone();
    let hook_address = address.clone();
    let delivery_id = RuntimeDeliveryId::new(receipt.delivery_id.clone()).expect("id");
    let hook_session = session.clone();
    *cut.resolver.before_observe.lock().expect("lock") = Some(Box::pin(async move {
        let outcome = inbox
            .transition_continuation_admission(
                &hook_address,
                &delivery_id,
                ContinuationAdmissionTransition::Reserve {
                    session_id: hook_session,
                    input_id: InputId::new(),
                },
            )
            .await
            .expect("admission index write");
        assert!(
            matches!(outcome, ContinuationAdmissionOutcome::Transitioned(_)),
            "{outcome:?}"
        );
    }));
    let status = cut.status(&owner, "task-1").await;
    assert!(
        matches!(status, ContinuationStatus::Pending { admitted: None, .. }),
        "a reservation exists, so the read is not stranded: {status:?}"
    );

    let handle = cut
        .fixture
        .owner()
        .arm(RetiringHost::new(
            sink.clone(),
            Arc::clone(&cut.retirements),
        ))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    wait_for_reconcile(&mut passes).await;
    let settled = cut.status(&owner, "task-1").await;
    assert!(
        matches!(settled, ContinuationStatus::Stranded { .. }),
        "the reserved session never held the input: {settled:?}"
    );
    assert!(sink.admissions().is_empty(), "nothing is admitted");
}

/// The owner settles a reservation only under the reserved session's
/// custody: a session another runtime owner serves keeps its reservation
/// for that owner, and the continuation stays pending.
#[tokio::test]
async fn a_reservation_on_a_retired_address_is_left_to_the_host_that_serves_its_session() {
    let cut = RetiringFixture::memory();
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let receipt = cut
        .fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let address = LogicalRuntimeId::new(receipt.address.clone());
    let reserved = InputId::new();
    cut.fixture
        .admission(
            &address,
            &receipt.delivery_id,
            ContinuationAdmissionTransition::Reserve {
                session_id: session.clone(),
                input_id: reserved.clone(),
            },
        )
        .await;
    cut.retirements.retire(&address);

    let host = RetiringHost::new(sink.clone(), Arc::clone(&cut.retirements));
    host.elsewhere.lock().expect("lock").insert(session.clone());
    let handle = cut.fixture.owner().arm(host).expect("arm");
    let mut passes = handle.subscribe_passes();
    wait_for_reconcile(&mut passes).await;

    let status = cut.status(&owner, "task-1").await;
    assert!(
        matches!(status, ContinuationStatus::Pending { admitted: None, .. }),
        "another host serves the reserved session: {status:?}"
    );
    assert_eq!(
        cut.admission_entry(&address, &receipt.delivery_id).await,
        Some(ContinuationAdmission::Reserved {
            session_id: session,
            input_id: reserved,
        }),
        "the reservation is left to that host"
    );
    assert!(sink.admissions().is_empty(), "nothing is admitted");
}

/// A live address still drains, once, through the observation that follows
/// its reservation.
#[tokio::test]
async fn a_live_address_still_drains_once_through_the_post_reserve_observation() {
    let cut = RetiringFixture::memory();
    let sink = Arc::new(RecordingSink::default());
    let session = SessionId::new();
    let owner = ContinuationOwner::Session {
        session_id: session.clone(),
    };
    let handle = cut
        .fixture
        .owner()
        .arm(RetiringHost::new(
            sink.clone(),
            Arc::clone(&cut.retirements),
        ))
        .expect("arm");
    let mut passes = handle.subscribe_passes();
    let receipt = cut
        .fixture
        .continuations
        .submit(&owner, delivery("task-1", host_producer()), 1)
        .await
        .expect("commit");
    let status = wait_for_status(&cut.fixture, &owner, "task-1", &mut passes, |status| {
        matches!(status, ContinuationStatus::Applied { .. })
    })
    .await;
    let admissions = sink.admissions();
    assert_eq!(admissions.len(), 1, "admitted exactly once: {admissions:?}");
    assert_eq!(
        status,
        ContinuationStatus::Applied {
            receipt,
            session,
            input: admissions[0].2.clone(),
        }
    );
}
