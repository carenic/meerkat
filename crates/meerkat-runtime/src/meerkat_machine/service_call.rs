//! Governed service calls: provider calls that no runtime input carries.
//!
//! A scheduled host runnable (for example a collector that reads a provider
//! through a connector-bound MCP server) runs host code with no agent turn,
//! so no admitted run supplies its work authorization. A connector-bound
//! call without one is refused (`meerkat::ConnectorMcpCallContextProvider`).
//! This module is the native seam for that work: the trusted host presents
//! a [`ServiceCallRequest`] naming the service original it enrolled at
//! operator-reviewed setup, and the machine's installed native host admits
//! it (see [`NativeWorkAuthorizationHost::service_call_authorization`]).
//! Every call made through the returned [`ServiceCall`] then runs the
//! ordinary fenced dispatch: prepared from the work's current owners (the
//! work owner, the operator's grant selected for exactly the operation's
//! MCP server, the operation owner), checked again at the call's entry, and
//! audited. Nothing here grants anything, and no run's authority is
//! borrowed.
//!
//! The audit is Core's. No input row exists whose next commit could hold a
//! service call's observations, so the machine builds the request's sink
//! itself: it only stages observations in process (its `append` does no
//! I/O), and the machine store's service-call audit log
//! ([`RuntimeStore::append_service_call_audit`]) makes them durable. Each
//! call's decision and entry are appended through the fenced dispatch's
//! observation barrier ([`OperationObservationBarrier`]), after the call's
//! root entry is staged and before any dispatcher code runs, so a call whose
//! append fails is refused before it is sent; what the call staged after
//! that (its outcome) is appended once the call returns.
//!
//! [`NativeWorkAuthorizationHost::service_call_authorization`]:
//! crate::input_authority::NativeWorkAuthorizationHost::service_call_authorization
//! [`RuntimeStore::append_service_call_audit`]:
//! crate::store::RuntimeStore::append_service_call_audit

use std::sync::{Arc, Mutex};

use meerkat_authorization_contracts::audit::{
    AuthorizationAuditObservation, AuthorizationAuditSink, NativeAuditContributor,
    StoredAuthorizationAuditObservation,
};
use meerkat_core::authorization::{OperationObservationBarrier, OperationObservationError};
use meerkat_core::exact_operation::OperationExecutionScope;
use meerkat_core::types::ToolCallView;
use meerkat_core::{
    AgentToolDispatcher, OperationAuthorizationError, ToolDispatchContext, ToolDispatchOutcome,
    ToolError, ToolExecutionResolutionContext,
};

use super::MeerkatMachine;
use crate::input_authority::ServiceCallRequest;
use crate::store::{RuntimeStore, RuntimeStoreError, ServiceCallAuditBatch, ServiceCallAuditKey};

impl MeerkatMachine {
    /// Admit `request`, one schedule occurrence's governed service calls,
    /// through this machine's installed native host.
    ///
    /// The machine builds the request's audit sink and keeps its
    /// observations in its store's service-call audit log, under the
    /// request's [`ServiceCallRequest::audit_key`]: a machine without a
    /// store admits no service call. A store without the log refuses each
    /// call at its first append, before anything is sent.
    ///
    /// # Errors
    /// `Unavailable` on an ungoverned machine, on a machine without a store,
    /// on a host or owner without service call support, or when an owner
    /// cannot say now; `Refused` (`Denied`) for an original that is not
    /// enrolled or is cancelled, and (`MalformedFacts`) for a request whose
    /// association is not the enrolled record.
    pub fn prepare_service_call(
        &self,
        request: Arc<ServiceCallRequest>,
    ) -> Result<ServiceCall, OperationAuthorizationError> {
        // An ungoverned machine has no native host, and so no service call:
        // no ungoverned context is ever handed out for one.
        let attachment = self
            .native_work_authorization_host
            .get()
            .ok_or(OperationAuthorizationError::Unavailable)?;
        self.shared
            .require_governed_execution_custody()
            .map_err(|_| OperationAuthorizationError::Unavailable)?;
        // The audit log is the store's: without one, nothing could hold the
        // record of a call, so none is admitted.
        let store = self
            .shared
            .store
            .clone()
            .ok_or(OperationAuthorizationError::Unavailable)?;
        let staged = Arc::new(NativeServiceCallAudit::default());
        let audit = Arc::new(ServiceCallAuditLog {
            store,
            key: request.audit_key().clone(),
            staged: Arc::clone(&staged),
            flush: crate::tokio::sync::Mutex::new(FlushState::Open),
        });
        let work = attachment
            .host()
            .service_call_authorization(request, staged as Arc<dyn AuthorizationAuditSink>)?;
        Ok(ServiceCall {
            context: ToolDispatchContext::default().with_work_authorization(Some(work)),
            audit,
        })
    }
}

/// One admitted schedule occurrence's governed service calls.
///
/// Not `Clone`: the runnable that prepared it owns it for its one
/// occurrence and drops it when the occurrence's run returns. Core builds
/// and keeps the dispatch context; the host's dispatcher receives it during
/// each dispatch, as every dispatcher does, and a context it keeps
/// authorizes no other call, because each entry needs a prepared check
/// current for that exact call.
pub struct ServiceCall {
    context: ToolDispatchContext,
    audit: Arc<ServiceCallAuditLog>,
}

impl ServiceCall {
    /// Make one call through `dispatcher` (for example the
    /// `McpRouterAdapter` the connector-bound server is staged on) under
    /// this occurrence's work authorization: the fenced plan resolution and
    /// dispatch every governed tool call takes. The governed decision is
    /// prepared and checked before any dispatcher code runs, so a refused
    /// operation never reaches the connector credential owner.
    ///
    /// The record is durable before the call is sent: once the call's
    /// decision is prepared and its root entry staged, the fenced dispatch
    /// awaits this call's observation barrier, which appends them to the
    /// store's service-call audit log, then checks the decision again (a
    /// decision prepared again meanwhile is appended too before the call
    /// enters with it). Only then does any dispatcher code run. When that
    /// append fails the call is refused
    /// ([`ToolError::OperationObservationUnavailable`]) and nothing is sent.
    /// What the call stages after that (its outcome, and the entries of the
    /// dispatcher's own boundaries) is appended once the call returns; a
    /// failure there cannot undo the call, so it is reported beside the
    /// call's result ([`ServiceCallOutcome::audit`]). After any failed
    /// append this `ServiceCall` refuses every later call the same way: its
    /// log would have a gap. The occurrence can prepare a new request.
    ///
    /// Calls on one `ServiceCall` append one at a time: a call waits for an
    /// append in progress, so that its own decision is durable before it
    /// enters. Calls on different `ServiceCall`s share no lock.
    ///
    /// The outcome's `result` is `AuthorizationRefused` for a governed
    /// refusal, and the dispatcher's own errors (a connector call-context
    /// refusal included); nothing is sent on any refusal.
    pub async fn dispatch<T: AgentToolDispatcher + ?Sized + 'static>(
        &self,
        dispatcher: &Arc<T>,
        call: ToolCallView<'_>,
        resolution: ToolExecutionResolutionContext,
    ) -> ServiceCallOutcome {
        // A barrier of this call's own, so an append that refuses it is
        // answered with this call.
        let barrier = Arc::new(ServiceCallBarrier {
            log: Arc::clone(&self.audit),
            failure: Mutex::new(None),
        });
        let context = self
            .context
            .clone()
            .with_observation_barrier(Arc::clone(&barrier) as Arc<dyn OperationObservationBarrier>);
        let result = match meerkat_core::resolve_tool_execution_plan_fenced(
            dispatcher,
            call,
            &context,
            &resolution,
        ) {
            Ok(plan) => {
                meerkat_core::dispatch_tool_execution_plan_fenced(dispatcher, call, &context, &plan)
                    .await
            }
            Err(error) => Err(error.into()),
        };
        if let Some(failure) = barrier.take_failure() {
            // The log refused an append of this call: nothing more of it is
            // appended (the log would have a gap).
            return ServiceCallOutcome {
                result,
                audit: Err(failure),
            };
        }
        // What the call staged after its barrier (its outcome), or its
        // refusal when it never reached one. The call already happened, so
        // a failure is reported, never turned into the call's own result.
        let audit = self.audit.append_staged().await;
        ServiceCallOutcome { result, audit }
    }

    /// The key of this occurrence's rows in the store's service-call audit
    /// log ([`ServiceCallRequest::audit_key`]).
    #[must_use]
    pub fn audit_key(&self) -> &ServiceCallAuditKey {
        &self.audit.key
    }
}

impl std::fmt::Debug for ServiceCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServiceCall([REDACTED])")
    }
}

/// What one [`ServiceCall::dispatch`] answers: the call's own result, and
/// whether every observation of the call reached the audit log.
#[derive(Debug)]
#[must_use]
#[non_exhaustive]
pub struct ServiceCallOutcome {
    /// The call's result. A call whose decision could not be appended
    /// before it was sent is `Err(OperationObservationUnavailable)` and was
    /// not sent.
    pub result: Result<ToolDispatchOutcome, ToolError>,
    /// `Err` when an append of this call's observations failed: before the
    /// call entered its dispatcher (then `result` says it was not sent), or
    /// after it returned (then `result` is the call's own, and its outcome
    /// is not in the log).
    pub audit: Result<(), ServiceCallAuditError>,
}

/// Why a service call's observations did not reach the audit log.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum ServiceCallAuditError {
    /// The store refused or failed the append; `Unsupported` for a store
    /// without the service-call audit log.
    #[error("service call audit append failed: {0}")]
    Store(#[from] RuntimeStoreError),
    /// The staged observations could not be read.
    #[error("service call audit staging failed")]
    Staging,
    /// An earlier append of this service call failed, so it appends nothing
    /// more: its log would have a gap.
    #[error("an earlier service call audit append failed")]
    Spent,
}

/// The audit sink Core builds for one governed service call request: it
/// stages each observation in process and does no I/O, so a call's
/// preparation is never slowed by the store; [`ServiceCallAuditLog`] makes
/// the staged observations durable. Every observation of a service call is
/// Domain-scoped and runless: anything else is refused.
#[derive(Default)]
struct NativeServiceCallAudit {
    staged: Mutex<Vec<StoredAuthorizationAuditObservation>>,
}

impl AuthorizationAuditSink for NativeServiceCallAudit {
    fn append(
        &self,
        observation: AuthorizationAuditObservation,
    ) -> Result<(), OperationObservationError> {
        if observation.execution_scope != OperationExecutionScope::Domain
            || observation.run_id.is_some()
        {
            return Err(OperationObservationError);
        }
        let mut staged = self.staged.lock().map_err(|_| OperationObservationError)?;
        staged
            .try_reserve(1)
            .map_err(|_| OperationObservationError)?;
        // No input contributed to a service call: the row's key names its
        // service original and occurrence.
        staged.push(StoredAuthorizationAuditObservation {
            contributors: Arc::<[NativeAuditContributor]>::from([]),
            observation,
        });
        Ok(())
    }
}

impl NativeServiceCallAudit {
    fn take(&self) -> Result<Vec<StoredAuthorizationAuditObservation>, ServiceCallAuditError> {
        let mut staged = self
            .staged
            .lock()
            .map_err(|_| ServiceCallAuditError::Staging)?;
        Ok(std::mem::take(&mut *staged))
    }
}

/// Whether a service call may still append to its audit log.
enum FlushState {
    Open,
    /// An append failed: the log would have a gap, so nothing more is
    /// appended and every later call is refused before it is sent.
    Spent,
}

/// One service call request's way into its store's audit log.
struct ServiceCallAuditLog {
    store: Arc<dyn RuntimeStore>,
    key: ServiceCallAuditKey,
    staged: Arc<NativeServiceCallAudit>,
    /// Appends are made one at a time, so a call that continues after an
    /// append knows that every observation staged before it, a concurrent
    /// call's included, is durable. This request's own lock: no other
    /// request, run or store operation takes it.
    flush: crate::tokio::sync::Mutex<FlushState>,
}

impl ServiceCallAuditLog {
    /// Append everything staged so far, in order, in one store append.
    async fn append_staged(&self) -> Result<(), ServiceCallAuditError> {
        let mut state = self.flush.lock().await;
        if matches!(*state, FlushState::Spent) {
            // Nothing more is appended: drop what was staged, so a host that
            // keeps calling holds no growing staging.
            let _ = self.staged.take();
            return Err(ServiceCallAuditError::Spent);
        }
        let observations = match self.staged.take() {
            Ok(observations) => observations,
            Err(error) => {
                *state = FlushState::Spent;
                return Err(error);
            }
        };
        if observations.is_empty() {
            return Ok(());
        }
        match self
            .store
            .append_service_call_audit(ServiceCallAuditBatch::new(self.key.clone(), observations))
            .await
        {
            Ok(()) => Ok(()),
            Err(error) => {
                *state = FlushState::Spent;
                Err(error.into())
            }
        }
    }
}

/// One call's observation barrier: the fenced dispatch awaits it before any
/// dispatcher code runs, and it appends everything staged so far (the
/// call's decision and root entry). It keeps the first append failure, for
/// the call's [`ServiceCallOutcome::audit`].
struct ServiceCallBarrier {
    log: Arc<ServiceCallAuditLog>,
    failure: Mutex<Option<ServiceCallAuditError>>,
}

impl ServiceCallBarrier {
    fn take_failure(&self) -> Option<ServiceCallAuditError> {
        self.failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl OperationObservationBarrier for ServiceCallBarrier {
    async fn before_entry(&self) -> Result<(), OperationObservationError> {
        match self.log.append_staged().await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get_or_insert(error);
                Err(OperationObservationError)
            }
        }
    }
}
