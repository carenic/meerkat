//! Host store adapters: Core's realm state kept in a host's own database
//! through the host store SPI ([`meerkat_core::host_store`]).
//!
//! - [`SpiRealm`] is the one per-process handle of a realm: the fence and
//!   its lease, the per-scope journal state and the commit engine. Every
//!   adapter of the process commits through it.
//! - [`SpiRuntimeStore`] is the [`crate::store::RuntimeStore`] over the
//!   realm: it keeps the in-memory store's semantics
//!   ([`crate::store::InMemoryRuntimeStore`], WholeBlobV1) and writes every
//!   mutation through to the host as one batch before it answers.
//! - [`SpiSessionStore`] is the realm's session slot: reads of the committed
//!   documents and the catalog through the runtime store, writes refused.
//!
//! # Layout
//!
//! Each runtime is one session scope ([`RUNTIME_FAMILY`]): its head pins
//! the facts a boot needs (the catalog entry, the machine lifecycle, the
//! delivery authority and the boot facts), and its rows carry everything
//! else as keyed entries (input states, receipts, the WholeBlob body and
//! its provisional tail, ...). Realm-wide maps are record scopes, one per
//! entry: continuation key bindings, direct member high waters, mob host
//! bindings and their revocations. Values larger than
//! [`realm::INLINE_VALUE_LIMIT`] live in chunk scopes, written before the
//! batch that references them. See [`codec`] for the durable formats.
//!
//! # Boot and memory (D2)
//!
//! A boot lists every head once. The pinned entries of every runtime are
//! imported, so listings, the delivery authority listing and the controller
//! inventory see every runtime. A runtime's body is loaded at boot only
//! when recovery must act on it: it has nonterminal inputs, pending terminal
//! owners, a provisional tail (an open run cursor), a current run in its
//! lifecycle, or input rows without a lifecycle. Archived runtimes are
//! never loaded at boot. Every other body loads on the first call that
//! names the runtime and stays resident until the runtime is archived.
//! Resident memory is therefore bounded by the bodies of the runtimes
//! opened since the process started and not archived since, plus every
//! runtime's pinned head entries.
//!
//! # Limits of this adapter
//!
//! - OAuth login flow snapshots are process-local: the three snapshot
//!   methods of [`crate::store::RuntimeStore`] are synchronous and the SPI
//!   is not, so an interactive login in progress does not survive a
//!   restart (the user starts it again). Completed credentials are kept by
//!   the credential stores and are not affected.
//! - The WholeBlob candidate document is written whole at every per-round
//!   commit, so the bytes a run writes grow with the square of its rounds
//!   (S1). Its chunks are replaced, not accumulated.

#![cfg(not(target_arch = "wasm32"))]

pub mod codec;
pub mod realm;
mod runtime;
mod session;
#[cfg(test)]
mod test_host;
#[cfg(test)]
mod tests;

use std::time::Duration;

use meerkat_core::host_store::{FenceToken, HostStoreError};

pub use realm::{
    ScopeCell, ScopeDelta, ScopeFamily, ScopeGuard, ScopeId, ScopeState, SpiRealm, SpiRealmConfig,
};
pub use runtime::{RUNTIME_FAMILY, SpiRuntimeStore};
pub use session::SpiSessionStore;

use super::RuntimeStoreError;

/// Why a host store adapter refused or failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SpiError {
    /// The realm lost its fence: every call refuses until it is reopened.
    #[error("host store realm is poisoned: {0}")]
    RealmPoisoned(String),
    /// A batch touching this scope failed after memory changed: the scope
    /// refuses every call until the realm is reopened.
    #[error("host store scope {scope} is poisoned: {cause}")]
    ScopePoisoned { scope: String, cause: String },
    /// An exclusive acquire met a live holder.
    #[error("host store fence is held by token {holder} for {remaining:?} more")]
    Held {
        holder: FenceToken,
        remaining: Duration,
    },
    /// The local lease is not live and a renew could not restore it; the
    /// call changed nothing.
    #[error("host store lease is not live: {0}")]
    LeaseLapsed(String),
    /// The host store failed.
    #[error("host store: {0}")]
    Host(#[from] HostStoreError),
    /// Host data does not decode or is inconsistent.
    #[error("host store data is invalid: {0}")]
    Corrupt(String),
    /// The adapter was used against its rules (a bug).
    #[error("host store adapter misuse: {0}")]
    Misuse(String),
}

impl SpiError {
    /// The error a mutating store call answers.
    #[must_use]
    pub fn into_write(self) -> RuntimeStoreError {
        RuntimeStoreError::WriteFailed(self.to_string())
    }

    /// The error a reading store call answers.
    #[must_use]
    pub fn into_read(self) -> RuntimeStoreError {
        RuntimeStoreError::ReadFailed(self.to_string())
    }
}
