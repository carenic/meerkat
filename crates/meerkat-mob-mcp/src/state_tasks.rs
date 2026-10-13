//! Background work a [`crate::MobMcpState`] starts on its own, and the
//! admissions that may still add to what it manages.
//!
//! Every task the state spawns outside a caller's await goes through
//! [`StateTasks`], and every caller that may add a mob or start detached work
//! holds a [`StateHold`] from its admission check until its writes are done,
//! so a host can close the state and wait for both before it releases what
//! they write to (a durable child-mob root). Each task has a
//! [`StateTaskKind`] that decides what a stand-down does with it.
//!
//! Custody stays here, never in a stand-down caller's frame: a task leaves
//! the set only when its future is dropped (it finished, was aborted, or
//! panicked), and a hold only when it is dropped. A stand-down waits for the
//! set to empty, so a concurrent or cancelled stand-down cannot take tasks
//! out of view and report a clean settlement while they run.

use std::future::Future;

/// The work a state task runs: `Send` on native targets, where it is spawned
/// onto the multi-threaded runtime; wasm32 futures cannot be `Send` and run
/// on the single-threaded local executor.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait StateTaskFuture<T>: Future<Output = T> + Send + 'static {}
#[cfg(not(target_arch = "wasm32"))]
impl<F, T> StateTaskFuture<T> for F where F: Future<Output = T> + Send + 'static {}
#[cfg(target_arch = "wasm32")]
pub(crate) trait StateTaskFuture<T>: Future<Output = T> + 'static {}
#[cfg(target_arch = "wasm32")]
impl<F, T> StateTaskFuture<T> for F where F: Future<Output = T> + 'static {}

/// A state task's output, with the same per-target `Send` rule.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait StateTaskOutput: Send + 'static {}
#[cfg(not(target_arch = "wasm32"))]
impl<T> StateTaskOutput for T where T: Send + 'static {}
#[cfg(target_arch = "wasm32")]
pub(crate) trait StateTaskOutput: 'static {}
#[cfg(target_arch = "wasm32")]
impl<T> StateTaskOutput for T where T: 'static {}

#[cfg(target_arch = "wasm32")]
use crate::tokio;

/// How a stand-down treats one state-owned task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateTaskKind {
    /// Loops for the life of the state (a sweep, a reconciler). A
    /// stand-down aborts it; closing refuses a new one.
    Periodic,
    /// Idempotent convergence over durable records (a re-link, a recovery
    /// sweep) that the next state over the same storage redoes. A
    /// stand-down aborts it; closing refuses a new one.
    Convergent,
    /// Carries an outcome a caller or a durable record waits for (a council
    /// run, a completion submission, a teardown handoff). A stand-down joins
    /// it and never aborts it; it is admitted until the state is sealed.
    Owned,
}

/// The state's background tasks, its admission holds, and its admission.
///
/// Open: everything is admitted. Closed (a stand-down started): new
/// admissions ([`Self::admit`]) and periodic or convergent work are refused;
/// owned work and [`Self::hold`] are still admitted, because they may be the
/// second half of work admitted before the close, and they are waited for
/// like the rest. Sealed (a stand-down saw everything settle): nothing is
/// admitted.
pub(crate) struct StateTasks {
    #[cfg(not(target_arch = "wasm32"))]
    shared: std::sync::Arc<Shared>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Shared {
    inner: std::sync::Mutex<Inner>,
    /// Tasks plus holds still live, republished on every change.
    live: tokio::sync::watch::Sender<usize>,
    /// Admissions ([`StateTasks::admit`]) still live.
    admitted: tokio::sync::watch::Sender<usize>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Inner {
    closed: bool,
    sealed: bool,
    next_id: u64,
    tasks: std::collections::BTreeMap<u64, Tracked>,
    /// Each hold's name, and whether it is an admission.
    holds: std::collections::BTreeMap<u64, (&'static str, bool)>,
    /// Owned work that ended without finishing (it panicked or something
    /// else aborted it): its outcome is lost, so no later stand-down is
    /// clean.
    failures: Vec<String>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Tracked {
    kind: StateTaskKind,
    name: &'static str,
    /// Set once the task is spawned.
    abort: Option<tokio::task::AbortHandle>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Shared {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn publish(&self, inner: &Inner) {
        self.live
            .send_replace(inner.tasks.len() + inner.holds.len());
        self.admitted.send_replace(
            inner
                .holds
                .values()
                .filter(|(_, admission)| *admission)
                .count(),
        );
    }
}

/// Removes its task from the state's custody when the task's future is
/// dropped, however it ends.
#[cfg(not(target_arch = "wasm32"))]
struct Settle {
    shared: std::sync::Arc<Shared>,
    id: u64,
    finished: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Settle {
    fn drop(&mut self) {
        let mut inner = self.shared.inner();
        if let Some(tracked) = inner.tasks.remove(&self.id)
            && !self.finished
            && tracked.kind == StateTaskKind::Owned
        {
            inner.failures.push(format!(
                "{}: ended without finishing (it panicked or was aborted)",
                tracked.name
            ));
        }
        self.shared.publish(&inner);
    }
}

/// One admitted caller or detached task that may still write to what the
/// state manages. A stand-down is not clean while any hold is alive.
pub(crate) struct StateHold {
    #[cfg(not(target_arch = "wasm32"))]
    shared: std::sync::Arc<Shared>,
    #[cfg(not(target_arch = "wasm32"))]
    id: u64,
}

impl Drop for StateHold {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut inner = self.shared.inner();
            inner.holds.remove(&self.id);
            self.shared.publish(&inner);
        }
    }
}

impl StateTasks {
    pub(crate) fn new() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            shared: std::sync::Arc::new(Shared {
                inner: std::sync::Mutex::new(Inner::default()),
                live: tokio::sync::watch::Sender::new(0),
                admitted: tokio::sync::watch::Sender::new(0),
            }),
        }
    }

    /// Whether new admissions are accepted: false once a stand-down has
    /// started.
    #[cfg(test)]
    pub(crate) fn admits_new_work(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            !self.shared.inner().closed
        }
        #[cfg(target_arch = "wasm32")]
        {
            true
        }
    }

    /// Admit a caller that may add a mob or start detached work, named
    /// `name` in a stand-down report. `None` once a stand-down has started.
    /// Hold the result until the caller's writes are done (through the
    /// insertion of what it created): checking admission before the writes
    /// is not enough, because a stand-down could otherwise settle in between.
    pub(crate) fn admit(&self, name: &'static str) -> Option<StateHold> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut inner = self.shared.inner();
            if inner.closed {
                return None;
            }
            Some(self.hold_locked(&mut inner, name, true))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = name;
            Some(StateHold {})
        }
    }

    /// Hold the state for the second half of admitted work (a detached
    /// delivery, a cleanup), named `name` in a stand-down report. Unlike
    /// [`Self::admit`] this is accepted while a stand-down runs; `None` only
    /// once the state is sealed.
    #[cfg_attr(
        not(any(test, all(feature = "openai-live", not(target_arch = "wasm32")))),
        allow(dead_code)
    )]
    pub(crate) fn hold(&self, name: &'static str) -> Option<StateHold> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut inner = self.shared.inner();
            if inner.sealed {
                return None;
            }
            Some(self.hold_locked(&mut inner, name, false))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = name;
            Some(StateHold {})
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn hold_locked(&self, inner: &mut Inner, name: &'static str, admission: bool) -> StateHold {
        let id = inner.next_id;
        inner.next_id += 1;
        inner.holds.insert(id, (name, admission));
        self.shared.publish(inner);
        StateHold {
            shared: std::sync::Arc::clone(&self.shared),
            id,
        }
    }

    /// Start `work` as a task this state owns, named `name` in a stand-down
    /// report. `None` when the state no longer admits work of this kind;
    /// nothing was started then. The receiver yields the work's output (and
    /// closes without one if the task was aborted or panicked); dropping it
    /// does not affect the task.
    pub(crate) fn spawn<F, T>(
        &self,
        kind: StateTaskKind,
        name: &'static str,
        work: F,
    ) -> Option<tokio::sync::oneshot::Receiver<T>>
    where
        F: StateTaskFuture<T>,
        T: StateTaskOutput,
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let id = {
                let mut inner = self.shared.inner();
                if inner.sealed || (inner.closed && kind != StateTaskKind::Owned) {
                    return None;
                }
                let id = inner.next_id;
                inner.next_id += 1;
                inner.tasks.insert(
                    id,
                    Tracked {
                        kind,
                        name,
                        abort: None,
                    },
                );
                self.shared.publish(&inner);
                id
            };
            // Tracked before it is spawned; the lock is not held across the
            // spawn, because a runtime that is shutting down drops the
            // future (and its settle guard) right there.
            let mut settle = Settle {
                shared: std::sync::Arc::clone(&self.shared),
                id,
                finished: false,
            };
            let handle = tokio::spawn(async move {
                let output = work.await;
                settle.finished = true;
                let _ = tx.send(output);
                drop(settle);
            });
            let mut inner = self.shared.inner();
            let closed = inner.closed;
            if let Some(tracked) = inner.tasks.get_mut(&id) {
                if closed && kind != StateTaskKind::Owned {
                    // A stand-down started between the admission and the
                    // spawn: it could not abort this task yet.
                    handle.abort();
                } else {
                    tracked.abort = Some(handle.abort_handle());
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (kind, name);
            tokio::spawn(async move {
                let _ = tx.send(work.await);
            });
        }
        Some(rx)
    }

    /// Close admission and abort the periodic and convergent tasks.
    /// Idempotent; owned tasks and holds keep running. Native only: wasm32
    /// cannot fence state admission.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn close(&self) {
        let mut inner = self.shared.inner();
        inner.closed = true;
        for tracked in inner.tasks.values() {
            if tracked.kind != StateTaskKind::Owned
                && let Some(abort) = &tracked.abort
            {
                abort.abort();
            }
        }
    }

    /// Close (see [`Self::close`]) and wait until every admitted caller (a
    /// create, insert, restore or council admission already past its check)
    /// is done or `deadline` passes. Returns the admissions still running:
    /// empty means nothing admitted before the close can add a mob any more.
    /// Owned work and second-half holds may still run.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn close_admissions(&self, deadline: std::time::Instant) -> Vec<String> {
        self.close();
        let deadline = tokio::time::Instant::from_std(deadline);
        let mut admitted = self.shared.admitted.subscribe();
        let _ = tokio::time::timeout_at(deadline, admitted.wait_for(|count| *count == 0)).await;
        self.shared
            .inner()
            .holds
            .values()
            .filter(|(_, admission)| *admission)
            .map(|(name, _)| format!("{name}: still admitted"))
            .collect()
    }

    /// Close (see [`Self::close`]) and wait until every task and hold has
    /// settled or `deadline` passes. Returns what did not settle: empty
    /// means everything this state owns has finished, and the state is then
    /// sealed (it admits nothing more). Work still running at the deadline
    /// keeps running and stays in the state's custody; a later call waits
    /// for it again. Owned work that ended without finishing is reported by
    /// every later call too.
    pub(crate) async fn stand_down(&self, deadline: std::time::Instant) -> Vec<String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.close();
            let deadline = tokio::time::Instant::from_std(deadline);
            let mut live = self.shared.live.subscribe();
            loop {
                let settled = tokio::time::timeout_at(deadline, live.wait_for(|count| *count == 0))
                    .await
                    .is_ok();
                let mut inner = self.shared.inner();
                if inner.tasks.is_empty() && inner.holds.is_empty() {
                    if inner.failures.is_empty() {
                        inner.sealed = true;
                    }
                    return inner.failures.clone();
                }
                if !settled {
                    let mut unsettled = inner.failures.clone();
                    unsettled.extend(
                        inner
                            .tasks
                            .values()
                            .map(|tracked| format!("{}: still running", tracked.name)),
                    );
                    unsettled.extend(
                        inner
                            .holds
                            .values()
                            .map(|(name, _)| format!("{name}: still admitted")),
                    );
                    return unsettled;
                }
                // Owned work or a hold arrived after the count read zero:
                // wait for it too.
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = deadline;
            vec!["state-owned tasks cannot be joined on wasm32".to_string()]
        }
    }
}
