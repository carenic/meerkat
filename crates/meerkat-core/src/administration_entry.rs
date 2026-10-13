//! A host's admission of one administrative mutation, checked by the
//! mutation's owner at the mutation's entry.
//!
//! An owner that writes a realm's configuration or credentials for an
//! authenticated administrator waits for custody first (the config
//! document's locks and generation, the credential mutation owner's
//! exclusion). The administrator can lose that authority while the write
//! waits, so a check made before or around those waits admits nothing. The
//! owner runs the host's [`AdministrationEntry`] after it holds that custody,
//! immediately before it writes anything, and holds the returned
//! [`EnteredAdministration`] until the mutation has settled.

/// A host's admission check for one administrative mutation of a realm's
/// configuration or credentials.
///
/// The mutation's owner calls [`Self::enter`] at the mutation's entry: after
/// it holds every custody it waits for, and immediately before anything is
/// written. A refusal writes nothing. The check runs while the owner holds
/// that custody, so it decides from state it can read at once and never
/// waits; it is the host's own authentication and authorization, which the
/// owner neither performs nor replaces.
pub trait AdministrationEntry: Send + Sync {
    /// Admit the mutation now, or refuse it.
    fn enter(&self) -> Result<EnteredAdministration, AdministrationEntryRefusal>;
}

/// An administrative mutation its host admitted at the mutation's entry.
///
/// The mutation's owner holds it until the mutation has settled (written, or
/// failed with its outcome known or unknown) and only then drops it, so a
/// host that records its entered operations sees the mutation as entered
/// until it settles. It authorizes nothing by itself and is never stored.
pub struct EnteredAdministration {
    _held: Option<Box<dyn std::any::Any + Send>>,
}

impl EnteredAdministration {
    /// An admission that holds nothing.
    pub fn admitted() -> Self {
        Self { _held: None }
    }

    /// An admission holding `guard`, the host's own record of the entered
    /// operation, until the mutation settles.
    pub fn holding(guard: impl Send + 'static) -> Self {
        Self {
            _held: Some(Box::new(guard)),
        }
    }
}

impl std::fmt::Debug for EnteredAdministration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnteredAdministration")
            .finish_non_exhaustive()
    }
}

/// Why an administrative mutation was not admitted at its entry. Nothing was
/// written.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdministrationEntryRefusal {
    /// The caller is no longer authenticated.
    #[error("the caller is no longer authenticated")]
    NotAuthenticated,
    /// The caller is authenticated but does not administer now.
    #[error("the caller does not administer this installation")]
    NotAdministrator,
    /// The caller's authority could not be established now.
    #[error("the caller's administration could not be established: {0}")]
    Unavailable(String),
}
