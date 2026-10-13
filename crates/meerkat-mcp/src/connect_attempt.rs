//! The caller-held lifetime of one MCP connect or inspection attempt.
//!
//! Dropping a connect future, a running service or an rmcp worker transport
//! does not stop its work: the transport's worker finishes any request it
//! already started (it observes cancellation only between requests), a
//! response stream task can outlive the worker that started it, and rmcp's
//! bounded close detaches the worker it stops waiting for. An
//! [`McpConnectAttempt`] is created by its caller before the attempt's first
//! await and outlives it. Cancelling it, or dropping an attempt-scoped connect
//! before it returns, aborts the attempt's in-flight credential reads and HTTP
//! requests (Streamable HTTP), so nothing further is sent or published.
//!
//! Every part of the attempt's work holds a lease while it exists: the
//! attempt-scoped connect from before its first await until it returns or is
//! dropped, every clone of its transport client, every response stream the
//! client returned, and its connection's close. [`McpConnectAttempt::settle`]
//! reports what was actually observed: whether every lease was released (its
//! local termination joined), and, separately, whether the server confirmed
//! the deletion of the session it opened. A cancellation request is not a
//! termination, and a bound is a responsiveness limit, never a settlement
//! receipt: a termination not observed within it is reported as such, and a
//! later settlement still observes it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

/// The lifetime of one connect or inspection attempt (see the module docs).
/// Cloning shares the same attempt.
#[derive(Clone, Default)]
pub struct McpConnectAttempt(Arc<AttemptShared>);

#[derive(Default)]
struct AttemptShared {
    abort: CancellationToken,
    /// Leases on the attempt's work still held.
    live: tokio::sync::watch::Sender<usize>,
    session: Mutex<SessionRecord>,
}

/// What the attempt's transport saw of its server session.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum SessionRecord {
    /// No request was handed to the server, or the server answered without
    /// a session.
    #[default]
    NoSession,
    /// A request was handed to the server and no answer about a session was
    /// observed: the server may have opened one.
    RequestSent,
    Established,
    DeleteStarted,
    Deleted,
    DeleteUnsupported,
    DeleteFailed,
}

/// Whether the attempt's local work was observed to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpLocalTermination {
    /// Every part of the attempt's work ended: its connect, every task
    /// holding its transport client or a response stream, and its
    /// connection's close (when one was started).
    Joined,
    /// Not observed within the bound: the work may still be running.
    NotObserved,
}

/// What the server confirmed about the session the attempt opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpRemoteSession {
    /// No server session exists: no request reached the server, or the
    /// server answered without one.
    NoSession,
    /// The server confirmed the session's deletion with a success status.
    Deleted,
    /// The server does not support deleting sessions; it ends the session on
    /// its own terms.
    DeletionUnsupported,
    /// A server session may exist and its deletion was not confirmed: a
    /// request reached the server with no answer about a session observed
    /// (cancelled or failed before its response head), or a session was
    /// opened and its deletion was never sent, answered without success,
    /// failed or abandoned.
    Unconfirmed,
}

/// What one settlement of an attempt observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpAttemptSettlement {
    pub local: McpLocalTermination,
    pub remote_session: McpRemoteSession,
}

impl std::fmt::Debug for McpConnectAttempt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpConnectAttempt")
            .field("cancelled", &self.0.abort.is_cancelled())
            .field("live_work", &*self.0.live.borrow())
            .finish()
    }
}

impl McpConnectAttempt {
    /// A new attempt, created before the attempt's first await.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Abort the attempt's in-flight credential reads and HTTP requests; no
    /// new one starts. Work already handed to the server may have reached
    /// it: cancelling never proves an effect did not happen, and the work
    /// has not ended until a settlement observes it.
    pub fn cancel(&self) {
        self.0.abort.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.abort.is_cancelled()
    }

    /// Whether `other` is this same attempt.
    pub(crate) fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Wait up to `bound` for the attempt's local work to end, then report
    /// what was observed. Settlements may run concurrently, and dropping one
    /// changes nothing another observes.
    pub async fn settle(&self, bound: Duration) -> McpAttemptSettlement {
        self.settle_until(tokio::time::Instant::now() + bound).await
    }

    pub(crate) async fn settle_until(
        &self,
        deadline: tokio::time::Instant,
    ) -> McpAttemptSettlement {
        let local = if self.work_ended(deadline).await {
            McpLocalTermination::Joined
        } else {
            McpLocalTermination::NotObserved
        };
        let remote_session = match *lock(&self.0.session) {
            SessionRecord::NoSession => McpRemoteSession::NoSession,
            SessionRecord::Deleted => McpRemoteSession::Deleted,
            SessionRecord::DeleteUnsupported => McpRemoteSession::DeletionUnsupported,
            SessionRecord::RequestSent
            | SessionRecord::Established
            | SessionRecord::DeleteStarted
            | SessionRecord::DeleteFailed => McpRemoteSession::Unconfirmed,
        };
        McpAttemptSettlement {
            local,
            remote_session,
        }
    }

    /// Whether every lease on the attempt's work was released by `deadline`.
    async fn work_ended(&self, deadline: tokio::time::Instant) -> bool {
        let mut live = self.0.live.subscribe();
        tokio::time::timeout_at(deadline, live.wait_for(|count| *count == 0))
            .await
            .is_ok_and(|ended| ended.is_ok())
    }

    /// A lease on one part of the attempt's work, live until dropped.
    pub(crate) fn lease(&self) -> AttemptLease {
        self.0
            .live
            .send_modify(|count| *count = count.saturating_add(1));
        AttemptLease(self.clone())
    }

    /// Run `work` on its own task, leased by the attempt until the task
    /// ends: the lease is taken before the task exists, so no settlement can
    /// observe the work as ended before it starts.
    pub(crate) fn spawn_retained(
        &self,
        work: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        let lease = self.lease();
        tokio::spawn(async move {
            let _lease = lease;
            work.await;
        });
    }

    /// Resolves once the attempt is cancelled.
    pub(crate) async fn cancelled(&self) {
        self.0.abort.cancelled().await;
    }

    /// Run `work` unless the attempt is or becomes cancelled first; `None`
    /// when cancelled (the work is dropped at its current await).
    pub(crate) async fn abortable<F: std::future::Future>(&self, work: F) -> Option<F::Output> {
        tokio::select! {
            biased;
            () = self.0.abort.cancelled() => None,
            output = work => Some(output),
        }
    }

    /// Cancels the attempt when dropped before [`AbortUnlessCompleted::completed`].
    pub(crate) fn abort_unless_completed(&self) -> AbortUnlessCompleted {
        AbortUnlessCompleted(Some(self.0.abort.clone()))
    }

    /// A request is being handed to the server: until its response head is
    /// observed, the server may have opened a session.
    pub(crate) fn record_request_sent(&self) {
        let mut session = lock(&self.0.session);
        if *session == SessionRecord::NoSession {
            *session = SessionRecord::RequestSent;
        }
    }

    /// A response head was observed, carrying a session header or not: the
    /// server's own answer about a session.
    pub(crate) fn record_response_head(&self, carries_session: bool) {
        let mut session = lock(&self.0.session);
        match (*session, carries_session) {
            (SessionRecord::NoSession | SessionRecord::RequestSent, true) => {
                *session = SessionRecord::Established;
            }
            (SessionRecord::RequestSent, false) => *session = SessionRecord::NoSession,
            _ => {}
        }
    }

    pub(crate) fn record_session_delete_started(&self) {
        *lock(&self.0.session) = SessionRecord::DeleteStarted;
    }

    pub(crate) fn record_session_deleted(&self, supported: bool) {
        *lock(&self.0.session) = if supported {
            SessionRecord::Deleted
        } else {
            SessionRecord::DeleteUnsupported
        };
    }

    pub(crate) fn record_session_delete_failed(&self) {
        *lock(&self.0.session) = SessionRecord::DeleteFailed;
    }
}

/// A lease on one part of an attempt's work, live until dropped.
pub(crate) struct AttemptLease(McpConnectAttempt);

impl Drop for AttemptLease {
    fn drop(&mut self) {
        (self.0)
            .0
            .live
            .send_modify(|count| *count = count.saturating_sub(1));
    }
}

/// A response stream leased by its attempt until the stream itself is
/// dropped, wherever its holder lives.
pub(crate) struct LeasedStream<S> {
    stream: S,
    _lease: AttemptLease,
}

impl<S> LeasedStream<S> {
    pub(crate) fn new(stream: S, lease: AttemptLease) -> Self {
        Self {
            stream,
            _lease: lease,
        }
    }
}

impl<S: futures::Stream + Unpin> futures::Stream for LeasedStream<S> {
    type Item = S::Item;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.stream).poll_next(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

/// Cancels its attempt when dropped, unless the attempt completed first.
pub(crate) struct AbortUnlessCompleted(Option<CancellationToken>);

impl AbortUnlessCompleted {
    pub(crate) fn completed(mut self) {
        self.0 = None;
    }
}

impl Drop for AbortUnlessCompleted {
    fn drop(&mut self) {
        if let Some(abort) = self.0.take() {
            abort.cancel();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
