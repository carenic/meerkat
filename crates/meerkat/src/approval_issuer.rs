//! The one approval issuer of a host's persistence.
//!
//! Every surface, the operation review, decision ingress, recovery and
//! delivery over one persistence share one [`ApprovalService`]: the host
//! states whether its persistence is durable, selects the issuer once,
//! before the factory is cloned, and passes that owner on. It is never
//! replaced while it issues. Two services over one approval file are two
//! in-process owners, so a host opens its selection exactly once.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use meerkat_core::ApprovalService;

/// How a host's persistence selects its one approval issuer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalIssuerSelection {
    /// Durable approvals in `approvals.json` under the host's store path.
    Persistent { store_path: PathBuf },
    /// Process-local approvals, for a host that is explicitly ephemeral.
    Ephemeral,
}

/// A host's approval issuer cannot be selected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApprovalIssuerSelectionError {
    #[error("a persistent host must name its approval store path")]
    PersistentWithoutStorePath,
}

impl ApprovalIssuerSelection {
    /// The selection of a host that states whether it is `persistent`: a
    /// persistent host names its `store_path`, and an absent path never
    /// downgrades it to ephemeral approvals.
    ///
    /// # Errors
    /// [`ApprovalIssuerSelectionError::PersistentWithoutStorePath`].
    pub fn for_host(
        persistent: bool,
        store_path: Option<&Path>,
    ) -> Result<Self, ApprovalIssuerSelectionError> {
        match (persistent, store_path) {
            (true, Some(store_path)) => Ok(Self::Persistent {
                store_path: store_path.to_path_buf(),
            }),
            (true, None) => Err(ApprovalIssuerSelectionError::PersistentWithoutStorePath),
            (false, _) => Ok(Self::Ephemeral),
        }
    }

    /// Open the one issuer of this selection; a host calls it once. A
    /// persistent selection whose store cannot be opened or loaded yields an
    /// unavailable service that refuses every approval operation, never an
    /// ephemeral one.
    #[must_use]
    pub fn open(&self) -> ApprovalService {
        let Self::Persistent { store_path } = self else {
            return ApprovalService::new();
        };
        let path = store_path.join("approvals.json");
        match meerkat_store::FileApprovalStore::open(&path) {
            Ok(store) => match ApprovalService::with_store(Arc::new(store)) {
                Ok(service) => service,
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        path = %path.display(),
                        "failed to load approval records; approval service unavailable and failing closed"
                    );
                    ApprovalService::unavailable(format!(
                        "failed to load approval records from {}: {err}",
                        path.display()
                    ))
                }
            },
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    path = %path.display(),
                    "failed to open approval store; approval service unavailable and failing closed"
                );
                ApprovalService::unavailable(format!(
                    "failed to open approval store {}: {err}",
                    path.display()
                ))
            }
        }
    }
}
