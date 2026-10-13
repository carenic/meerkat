//! A host's own committed-result owner composed into the job source a mob
//! state binds: the host's exact namespaces are answered by it, its fork_off
//! and council jobs by the state, and any other host namespace by no one.

#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod support;

use std::sync::Arc;

use meerkat::{ContinuationProducer, RetainedJobFacts, RetainedJobLookup, RetainedJobSource};
use meerkat_core::SessionId;
use meerkat_mob_mcp::{HostJobSource, HostJobSourceError, MobMcpState};
use support::{CouncilFixture, ScriptedTurn};

/// A host's setup store: every job it is asked for is its committed one.
struct SetupStore {
    facts: RetainedJobFacts,
}

#[async_trait::async_trait]
impl RetainedJobSource for SetupStore {
    async fn retained_job(
        &self,
        _producer: &ContinuationProducer,
        _job_id: &str,
    ) -> RetainedJobLookup {
        RetainedJobLookup::Found(self.facts.clone())
    }
}

fn host(namespace: &str) -> ContinuationProducer {
    ContinuationProducer::Host {
        namespace: namespace.into(),
    }
}

#[test]
fn a_host_job_source_must_own_a_named_namespace() {
    let store: Arc<dyn RetainedJobSource> = Arc::new(SetupStore {
        facts: RetainedJobFacts {
            owner_session_id: SessionId::new(),
            retained_work: None,
            result_digest: "sha256:setup".into(),
        },
    });
    assert_eq!(
        HostJobSource::new(Vec::<String>::new(), Arc::clone(&store)).err(),
        Some(HostJobSourceError::NoNamespaces)
    );
    assert_eq!(
        HostJobSource::new([""], store).err(),
        Some(HostJobSourceError::EmptyNamespace)
    );
}

#[tokio::test]
async fn the_bound_job_source_composes_the_hosts_namespaces_with_the_states_jobs() {
    let fixture = CouncilFixture::new_runtime_backed(|_| ScriptedTurn::Text("ok".into()));
    let facts = RetainedJobFacts {
        owner_session_id: SessionId::new(),
        retained_work: None,
        result_digest: "sha256:setup".into(),
    };
    let setup = HostJobSource::new(
        ["app.setup"],
        Arc::new(SetupStore {
            facts: facts.clone(),
        }),
    )
    .expect("a named namespace");
    let state = MobMcpState::new_with_runtime_adapter(
        fixture.service.clone(),
        fixture.runtime_adapter.clone(),
        meerkat_mob::MobControlPrincipal::Owner,
    )
    .expect("construct runtime authority")
    .with_host_job_source(setup)
    .into_shared();
    assert!(state.continuation_service().is_none(), "not bound yet");

    let inbox = meerkat_runtime::RuntimeDeliveryInbox::new(Arc::new(
        meerkat_runtime::InMemoryRuntimeStore::new(),
    ));
    let bindings = meerkat::ContinuationHostBindings::default();
    state
        .bind_continuations(inbox, &bindings)
        .expect("bind the state's continuations");
    assert!(
        state.continuation_service().is_some(),
        "the bound service is the host's to submit through"
    );
    let bound = bindings.job_source().expect("a live binding");

    assert_eq!(
        bound.retained_job(&host("app.setup"), "setup-1").await,
        RetainedJobLookup::Found(facts),
        "the host's namespace is answered by the host's owner"
    );
    assert_eq!(
        bound.retained_job(&host("app.other"), "setup-1").await,
        RetainedJobLookup::Absent,
        "a namespace the host did not declare is owned by no one"
    );
    assert!(
        !matches!(
            bound
                .retained_job(&ContinuationProducer::ForkOff, "setup-1")
                .await,
            RetainedJobLookup::Found(_)
        ),
        "fork_off jobs stay with the state's own owner, which holds none here"
    );
}
