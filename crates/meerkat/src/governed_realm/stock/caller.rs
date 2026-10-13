//! Who may be the requester of a stock composition's governed inputs.
//!
//! A stock composition admits requesters in exactly one posture
//! ([`StockCallerPosture`]):
//!
//! - [`StockCallerPosture::LocalOperator`], stock rkat's explicit trusted
//!   local-operator stdio posture: the process's own operating-system user
//!   ([`super::StockPrincipals::operator`]) is the only requester, at its own
//!   session destinations. It authenticates no delegated or remote user.
//! - [`StockCallerPosture::HostAuthenticated`]: a trusted host authenticates
//!   each caller itself and binds each input with that caller
//!   ([`StockAuthenticatedCaller`],
//!   [`super::StockGovernance::bind_caller_prompt`]). The canonical session
//!   owner stays the composition's operator, and so do the executor, the
//!   destination and the native grant checks: a caller never becomes the
//!   owner of the session it invokes. The operator is no requester in this
//!   posture. Invocation admission and every admitted operation ask the
//!   host the same exact-target question ([`StockCallerAccess`]), so the two
//!   policy faces agree.
//!
//! Each posture refuses the other's binding locally: no caller is relabeled
//! as the operator, and no operator prompt is bound as a caller's.
//!
//! The host's decision covers invocation only. It is never consent
//! eligibility: who may decide an operation's review is the consent route's
//! own question.

use std::sync::Arc;

use meerkat_authorization_contracts::evidence::HistoricalEvidenceRef;
use meerkat_core::{OperationAuthorizationError, PrincipalRef, SessionId};

use super::StockGovernanceError;

/// The posture in which a stock composition admits requesters; see the
/// module documentation.
#[derive(Clone)]
pub enum StockCallerPosture {
    /// The process's operating-system operator is the only requester.
    LocalOperator,
    /// Each requester is a caller the trusted host authenticated, admitted
    /// only by the host's exact-target decision.
    HostAuthenticated(Arc<dyn StockCallerAccess>),
}

impl std::fmt::Debug for StockCallerPosture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LocalOperator => f.write_str("LocalOperator"),
            Self::HostAuthenticated(_) => f.write_str("HostAuthenticated"),
        }
    }
}

/// One exact-target invocation question a stock composition asks its host:
/// may this authenticated caller, in the exact login its input was
/// authenticated in, now invoke this executor in this session of this owner?
pub struct StockCallerRequest<'a> {
    pub(super) caller: &'a PrincipalRef,
    pub(super) authentication: Option<&'a HistoricalEvidenceRef>,
    pub(super) executor: &'a PrincipalRef,
    pub(super) owner: &'a PrincipalRef,
    pub(super) session: &'a SessionId,
}

impl<'a> StockCallerRequest<'a> {
    /// The authenticated caller: the requester of the work.
    #[must_use]
    pub fn caller(&self) -> &'a PrincipalRef {
        self.caller
    }

    /// The host's own evidence of the exact authentication (login) the
    /// caller's input was admitted with, as the native owner retained it
    /// with the work ([`StockAuthenticatedCaller::authentication`]). The host
    /// answers for that exact login, never for any current login of the same
    /// principal: a rotated, logged-out or expired login stays refused while
    /// another login of the same person is current. `None` for a control
    /// whose ingress retains no login evidence (a configured MCP attachment
    /// control); a host that binds its decisions to exact logins refuses it.
    #[must_use]
    pub fn authentication(&self) -> Option<&'a HistoricalEvidenceRef> {
        self.authentication
    }

    /// The realm's agent executor the caller invokes.
    #[must_use]
    pub fn executor(&self) -> &'a PrincipalRef {
        self.executor
    }

    /// The canonical owner of the session: the composition's operator.
    #[must_use]
    pub fn owner(&self) -> &'a PrincipalRef {
        self.owner
    }

    /// The exact session invoked.
    #[must_use]
    pub fn session(&self) -> &'a SessionId {
        self.session
    }
}

/// A trusted host's current, exact-target invocation decision for its
/// authenticated callers.
///
/// It is asked inside the native admission and operation decisions, so it
/// answers from the host's own owned facts, synchronously and without I/O:
/// an admission returns the admitted login's lifetime; a refusal
/// ([`meerkat_core::OperationRefusalKind::Denied`]) is a local verdict;
/// `Unavailable` is local unavailable feedback, never an allowance. A host
/// whose facts can change joins their owner to the composition's publication
/// ([`super::StockGovernance::authorization_publication`]) at quiescent
/// startup, before it exposes any writer of those facts, so that a change
/// invalidates the decisions observed before it.
pub trait StockCallerAccess: Send + Sync {
    /// The host's decision for `request`: on admission, the lifetime of the
    /// exact login it admitted. A decision the composition takes for that
    /// login stands no longer than the login does, whether or not the host's
    /// own expiry timer has fired.
    ///
    /// # Errors
    /// A typed refusal or unavailability; see the trait documentation.
    fn authorize_caller(
        &self,
        request: &StockCallerRequest<'_>,
    ) -> Result<StockLoginLifetime, OperationAuthorizationError>;
}

/// How long the exact login a requester or approver was authenticated in
/// stays current. A decision admitted for that login stands no longer, and
/// the deadline is checked where the decision is used: a host's expiry timer
/// retires the login and notifies, it is never the proof that the login is
/// unexpired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockLoginLifetime {
    /// No expiry of its own: current until the host retires it (the local
    /// operator's process, or one stdio pipe's lifetime). The host publishes
    /// the retirement on the composition's publication.
    UntilRetired,
    /// Current until `unix_ms` (milliseconds since the Unix epoch) at the
    /// latest, and until the host retires it earlier.
    ExpiresAt { unix_ms: u64 },
}

impl StockLoginLifetime {
    /// Whether the login is still current at `now_ms`, by its deadline.
    #[must_use]
    pub fn is_current_at(self, now_ms: u64) -> bool {
        match self {
            Self::UntilRetired => true,
            Self::ExpiresAt { unix_ms } => now_ms < unix_ms,
        }
    }

    /// Whether the login is still current now, by its deadline and this
    /// host's clock. A clock before the Unix epoch proves no deadline unmet.
    #[must_use]
    pub fn is_current_now(self) -> bool {
        match self {
            Self::UntilRetired => true,
            Self::ExpiresAt { .. } => {
                unix_now_ms().is_some_and(|now_ms| self.is_current_at(now_ms))
            }
        }
    }

    /// `deadline_ms` bounded by the login's own deadline.
    #[must_use]
    pub fn bound(self, deadline_ms: u64) -> u64 {
        match self {
            Self::UntilRetired => deadline_ms,
            Self::ExpiresAt { unix_ms } => deadline_ms.min(unix_ms),
        }
    }
}

/// Milliseconds since the Unix epoch, or `None` for a clock before it.
fn unix_now_ms() -> Option<u64> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    u64::try_from(elapsed.as_millis()).ok()
}

/// A caller the trusted host authenticated for one governed input: the
/// requester and the host's own evidence of that authentication. Building it
/// authenticates nobody; the host builds it only after it has authenticated
/// the caller.
#[derive(Clone)]
pub struct StockAuthenticatedCaller {
    requester: PrincipalRef,
    authentication: HistoricalEvidenceRef,
}

impl std::fmt::Debug for StockAuthenticatedCaller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StockAuthenticatedCaller")
            .finish_non_exhaustive()
    }
}

impl StockAuthenticatedCaller {
    /// The caller `requester`, authenticated by the host as `authentication`
    /// records.
    ///
    /// # Errors
    /// [`StockGovernanceError::Principal`] when `requester` is not a
    /// qualified principal.
    pub fn new(
        requester: PrincipalRef,
        authentication: HistoricalEvidenceRef,
    ) -> Result<Self, StockGovernanceError> {
        requester.validate_qualified().map_err(|_| {
            StockGovernanceError::Principal("the caller is not a qualified principal".to_string())
        })?;
        Ok(Self {
            requester,
            authentication,
        })
    }

    /// The authenticated requester.
    #[must_use]
    pub fn requester(&self) -> &PrincipalRef {
        &self.requester
    }

    /// The host's evidence of the caller's authentication.
    #[must_use]
    pub fn authentication(&self) -> &HistoricalEvidenceRef {
        &self.authentication
    }
}
