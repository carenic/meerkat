//! A trusted host's own stores governed by a stock realm (host-supplied
//! persistence).
//!
//! A host that keeps its own session, runtime and subsystem stores (its own
//! runtime database and the decorator in front of it) runs its sessions under
//! the stock composition of a governed realm it names explicitly
//! ([`StockHostCustody`]): the realm whose posture, grant owner, grant
//! selection and installed MCP configuration govern the host's stores.
//! Nothing about that association is inferred from the host's own storage
//! layout or realm name.
//!
//! The host selects its posture before anything is prepared
//! ([`RealmGovernance`]). Only the stock composition is prepared here; every
//! other selection is refused typed, and a refused or failed preparation is
//! an error, never an ordinary or unavailable composition in its place.
//!
//! [`prepare_stock_host_composition`] takes custody before it reads any
//! governed owner, with the ownership semantics of a staged realm
//! ([`super::staged`]). The host's one staged machine retains two governed
//! claims together: the realm's administrative custody (the governed
//! execution custody of the realm's own runtime database, which every opener
//! of the realm's stores and every administrative command of the realm
//! holds while it acts) and the governed execution custody of the host's own
//! runtime store. The posture is then reread under that custody, the grant
//! owner restored on the composition's publication, the agent's selection
//! snapshotted, and the composition installed on that same machine exactly
//! once. For as long as the machine, or any handle sharing its custody,
//! lives, no administrative command, stock opener or other host of the realm
//! acts on it, so the composition the host prepared stays the realm's
//! current one. A competing owner refuses the preparation promptly; nothing
//! waits for it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use meerkat_core::connection::RealmId;
use meerkat_runtime::MeerkatMachine;
use meerkat_runtime::store::RuntimeStoreExecutionCustodyError;
use meerkat_store::{RealmManifestPin, RealmPaths};

use super::{GovernedComposition, RealmGovernance, stock::StockGovernance};
use crate::persistence::{PersistenceError, StagedRealmStores, realm_runtime_database};
use crate::storage_provider::RealmStoreSet;
use crate::{ApprovalIssuerSelection, PersistenceBundle};

/// The explicit association of a trusted host's own stores with the stock
/// governed realm `realm_id` under `realms_root`. It names the association
/// only: the custody it implies is acquired by
/// [`prepare_stock_host_composition`] and retained by the host's machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockHostCustody {
    realms_root: PathBuf,
    realm_id: RealmId,
}

impl StockHostCustody {
    /// The stock realm `realm_id` under `realms_root`.
    pub fn new(realms_root: impl Into<PathBuf>, realm_id: &str) -> Result<Self, StockHostError> {
        let realm_id = RealmId::parse(realm_id).map_err(|_| StockHostError::InvalidRealm {
            realm_id: realm_id.to_string(),
        })?;
        Ok(Self {
            realms_root: realms_root.into(),
            realm_id,
        })
    }

    pub fn realms_root(&self) -> &Path {
        &self.realms_root
    }

    pub fn realm_id(&self) -> &str {
        self.realm_id.as_str()
    }

    fn paths(&self) -> RealmPaths {
        meerkat_store::realm_paths_in(&self.realms_root, self.realm_id())
    }

    /// The realm's durable posture, which must be governed.
    async fn require_governed(&self) -> Result<(), StockHostError> {
        match meerkat_store::read_realm_posture_in(&self.realms_root, self.realm_id())
            .await
            .map_err(PersistenceError::from)?
        {
            meerkat_store::RealmPosture::Governed => Ok(()),
            meerkat_store::RealmPosture::Ungoverned
            | meerkat_store::RealmPosture::NotMaterialized => Err(StockHostError::NotGoverned {
                realm_id: self.realm_id().to_string(),
            }),
        }
    }

    /// The realm's manifest, read without creating or changing anything.
    fn manifest(&self, paths: &RealmPaths) -> Result<RealmManifestPin, StockHostError> {
        let manifest = meerkat_store::read_realm_manifest_pin(&paths.manifest_path)
            .map_err(PersistenceError::from)?;
        if manifest.realm().as_str() != self.realm_id() {
            return Err(
                PersistenceError::from(meerkat_store::StoreError::RealmIdentityMismatch {
                    requested: self.realm_id().to_string(),
                    existing: manifest.realm().as_str().to_string(),
                })
                .into(),
            );
        }
        Ok(manifest)
    }
}

/// Why a trusted host's stock composition was not prepared. Each refusal
/// ends this preparation only: no custody it took is kept, and nothing is
/// opened in place of the composition.
#[derive(Debug, thiserror::Error)]
pub enum StockHostError {
    /// The association does not name a valid realm.
    #[error("'{realm_id}' is not a valid realm id")]
    InvalidRealm { realm_id: String },
    /// The host selected no governance; an ordinary host prepares no stock
    /// composition.
    #[error("the host selected no governance for realm '{realm_id}'")]
    Ungoverned { realm_id: String },
    /// The host selected governance without the owners it needs (its
    /// operator, agent or ingress actor is not available).
    #[error("the host selected governance for realm '{realm_id}' without its owners")]
    GovernedWithoutOwners { realm_id: String },
    /// The host selected its own governed owners, which are not the stock
    /// composition prepared here.
    #[error("the host's own governed owners for realm '{realm_id}' are not a stock composition")]
    NotStock { realm_id: String },
    /// The stock composition governs another realm than the association.
    #[error(
        "the stock composition governs realm '{composition}', not the associated realm \
         '{realm_id}'"
    )]
    RealmMismatch {
        realm_id: String,
        composition: String,
    },
    /// The stock composition was already composed for a realm machine; each
    /// preparation takes a fresh one, so no earlier selection is reused.
    #[error("the stock composition for realm '{realm_id}' was already composed")]
    CompositionReused { realm_id: String },
    /// The associated realm's durable posture is not governed.
    #[error("realm '{realm_id}' is not governed")]
    NotGoverned { realm_id: String },
    /// The associated realm has no realm-wide execution custody a host can
    /// hold.
    #[error("realm '{realm_id}' has no realm-wide execution custody for a host: {reason}")]
    CustodyUnsupported { realm_id: String, reason: String },
    /// The realm's administrative custody is held by another owner (an
    /// administrative command, a stock opener or another host), or cannot
    /// be taken.
    #[error("the administrative custody of realm '{realm_id}' is not available: {source}")]
    RealmCustody {
        realm_id: String,
        #[source]
        source: RuntimeStoreExecutionCustodyError,
    },
    /// The realm's governed owners (its grant owner, its installations or
    /// the stock owners) are unavailable.
    #[error("the governed owners of realm '{realm_id}' are unavailable")]
    GovernanceUnavailable { realm_id: String },
    /// The host's runtime store is the realm's own runtime database (the
    /// same file, by any path): its machine would admit itself.
    #[error("the host's runtime store for realm '{realm_id}' is the realm's own runtime database")]
    HostStoreIsRealmStore { realm_id: String },
    /// A persistent host selected process-local approvals: its approvals
    /// are never downgraded to ephemeral.
    #[error(
        "the persistent host of realm '{realm_id}' needs a durable approval selection, not an \
         ephemeral one"
    )]
    EphemeralApprovals { realm_id: String },
    /// The host's stores, its machine or the realm's records were refused.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

/// A trusted host's prepared stock composition: its one machine, built over
/// the host's own runtime store with the realm's composition installed, and
/// the host's admitted stores for the bundle around it
/// ([`Self::construct`]). The machine holds the realm's administrative
/// custody and the host store's governed custody from
/// [`prepare_stock_host_composition`] on; dropping this releases both, with
/// nothing published.
pub struct PreparedStockHost {
    custody: StockHostCustody,
    stock: Arc<StockGovernance>,
    machine: MeerkatMachine,
    stores: StagedRealmStores,
}

impl std::fmt::Debug for PreparedStockHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedStockHost")
            .field("custody", &self.custody)
            .finish_non_exhaustive()
    }
}

impl PreparedStockHost {
    /// The association this composition was prepared for.
    pub fn custody(&self) -> &StockHostCustody {
        &self.custody
    }

    /// The prepared stock composition; the host binds its callers' inputs
    /// through it ([`StockGovernance::bind_caller_prompt`]).
    pub fn stock_governance(&self) -> &Arc<StockGovernance> {
        &self.stock
    }

    /// Assemble the host's bundle around its prepared machine: the host's
    /// own stores (its job and artifact stores included), one delivery inbox
    /// and one set of continuation bindings, the stock composition, and the
    /// host's one approval issuer, opened here from `approvals` exactly once
    /// ([`PersistenceBundle::approval_issuer`]). The host takes its single
    /// machine from [`PersistenceBundle::runtime_adapter`].
    ///
    /// A persistent host (its session or runtime store is persistent) is
    /// refused an ephemeral selection: its approvals are never downgraded.
    /// Refused, the prepared composition is dropped and its custody
    /// released.
    pub fn construct(
        self,
        approvals: ApprovalIssuerSelection,
    ) -> Result<(RealmManifestPin, PersistenceBundle), StockHostError> {
        if self.stores.is_persistent() && approvals == ApprovalIssuerSelection::Ephemeral {
            return Err(StockHostError::EphemeralApprovals {
                realm_id: self.custody.realm_id().to_string(),
            });
        }
        let (pin, mut bundle) = crate::persistence::assemble_persistence_bundle(
            self.stores,
            Arc::new(self.machine),
            &approvals,
        )?;
        bundle.set_stock_governance(Some(self.stock));
        Ok((pin, bundle))
    }
}

/// Prepare the stock composition `governance` (the host's posture selection)
/// for the host's own `stores`, under the realm `custody` names. See the
/// module docs for the custody it takes and keeps.
///
/// Refused typed, before any custody is taken, when the host selected no
/// stock composition, a composition of another realm or one already
/// composed, when the realm is not governed, or when the host's stores fail
/// the realm's durability or session-profile checks (the same checks as an
/// open of the realm's own stores). Refused promptly when the realm's
/// administrative custody or the host store's governed custody is held.
/// Refused under the custody it took, which the refusal releases, when the
/// realm's posture or runtime database changed before that custody was
/// taken, when another preparation composed the same composition meanwhile,
/// or when the governed owners do not restore. No ordinary or unavailable
/// composition is ever installed in place of a refused one.
pub async fn prepare_stock_host_composition(
    custody: StockHostCustody,
    governance: RealmGovernance,
    stores: RealmStoreSet,
) -> Result<PreparedStockHost, StockHostError> {
    let realm_id = || custody.realm_id().to_string();
    let stock = match governance {
        RealmGovernance::Stock(stock) => stock,
        RealmGovernance::Ungoverned => {
            return Err(StockHostError::Ungoverned {
                realm_id: realm_id(),
            });
        }
        RealmGovernance::GovernedWithoutOwners => {
            return Err(StockHostError::GovernedWithoutOwners {
                realm_id: realm_id(),
            });
        }
        RealmGovernance::Governed(_) => {
            return Err(StockHostError::NotStock {
                realm_id: realm_id(),
            });
        }
    };
    if stock.realm() != &custody.realm_id {
        return Err(StockHostError::RealmMismatch {
            realm_id: realm_id(),
            composition: stock.realm().as_str().to_string(),
        });
    }
    if stock.is_composed() {
        return Err(StockHostError::CompositionReused {
            realm_id: realm_id(),
        });
    }
    custody.require_governed().await?;
    let paths = custody.paths();
    let manifest = custody.manifest(&paths)?;
    let database = realm_runtime_database(&manifest, &paths).map_err(|reason| {
        StockHostError::CustodyUnsupported {
            realm_id: realm_id(),
            reason: reason.to_string(),
        }
    })?;
    let stores = StagedRealmStores::host_supplied(manifest, stores, paths.clone())?;
    #[cfg(test)]
    tests::before_custody(custody.realms_root());
    let realm_custody =
        meerkat_runtime::store::SqliteRuntimeStore::try_acquire_governed_execution_custody(
            &database,
        )
        .map_err(|source| StockHostError::RealmCustody {
            realm_id: realm_id(),
            source,
        })?;
    let host_store = stores.runtime_store();
    if host_store
        .execution_custody()
        .is_some_and(|host| host.binds_same_store(&realm_custody))
    {
        return Err(StockHostError::HostStoreIsRealmStore {
            realm_id: realm_id(),
        });
    }
    let staged = MeerkatMachine::persistent_governed_staged_with_associated_custody(
        stores.runtime_store(),
        Some(stores.blob_store()),
        realm_custody,
    )
    .map_err(PersistenceError::from)?;
    #[cfg(test)]
    tests::after_custody(custody.realms_root());
    // Reread under the custody now held: the posture and the database the
    // host proceeds with hold for as long as its machine lives.
    custody.require_governed().await?;
    if realm_runtime_database(&custody.manifest(&paths)?, &paths).ok() != Some(database) {
        // The custody taken is no longer the realm's.
        return Err(StockHostError::RealmCustody {
            realm_id: realm_id(),
            source: RuntimeStoreExecutionCustodyError::Unavailable,
        });
    }
    // A composition is composed only under the realm's custody, so one
    // another preparation composed (and released) while this one waited for
    // the custody is seen here, and never composed again.
    if stock.is_composed() {
        return Err(StockHostError::CompositionReused {
            realm_id: realm_id(),
        });
    }
    // No opener read the review rules for these host-supplied stores: the
    // composition reads them itself.
    let composition =
        super::governed_composition(RealmGovernance::Stock(Arc::clone(&stock)), None, &paths).await;
    if matches!(composition, GovernedComposition::Unavailable) {
        return Err(StockHostError::GovernanceUnavailable {
            realm_id: realm_id(),
        });
    }
    let machine =
        super::install_governed_composition(staged, composition).map_err(PersistenceError::from)?;
    Ok(PreparedStockHost {
        custody,
        stock,
        machine,
        stores,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "stock_host_tests.rs"]
mod tests;
