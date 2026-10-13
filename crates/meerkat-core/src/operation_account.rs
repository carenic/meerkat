//! The account an operation acts with, as the operation's own owner states it.
//!
//! An execution owner states the account with the exact operation it
//! resolves a call to ([`crate::ToolOperationTarget`]): the connector binding
//! or OAuth selection of the MCP connection the call runs on, the owner
//! configuration's declaration that the server acts with no selected
//! account, or a bundle registration's declaration. It is bounded, nonsecret
//! owner text for review and consent presentation, read only after native
//! authorization and only for reviewed (R2/R3) operations. It never
//! authorizes anything, is never a credential or a credential reference, and
//! is never inferred from a name, a read-only declaration or an empty
//! credential reference. The credential that actually satisfies a selected
//! account is checked where the operation physically enters its sink.

use std::fmt;

/// Upper bound, in bytes, for each part of a [`QualifiedAccount`].
pub const MAX_QUALIFIED_ACCOUNT_PART_BYTES: usize = 1024;

/// The account an operation's owner states the operation acts with.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OperationAccount {
    /// The owner selected exactly this account; the operation's credential
    /// must prove it at the operation's physical entry.
    Selected(QualifiedAccount),
    /// The owner's own configuration declares that the operation acts with
    /// no selected account. Nothing else establishes it: a missing connector
    /// binding, an empty credential reference or a read-only declaration
    /// never does.
    DeclaredNone,
    /// The owner cannot state the account: no declaration, or unknown,
    /// legacy or unverified account evidence. Review material for the
    /// operation is unavailable, never "no account".
    Unavailable,
}

/// One account, qualified by the authority whose namespace its subject is
/// in: the issuer that verifies it, or the protected resource whose OAuth
/// provider does.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct QualifiedAccount {
    authority: String,
    subject: String,
}

/// A refused [`QualifiedAccount`] part: blank, over
/// [`MAX_QUALIFIED_ACCOUNT_PART_BYTES`] or carrying a control character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a qualified account needs a nonblank authority and subject within their bound")]
pub struct InvalidQualifiedAccount;

impl QualifiedAccount {
    /// The account `subject` in `authority`'s namespace. Nothing is
    /// truncated.
    ///
    /// # Errors
    /// [`InvalidQualifiedAccount`] for a blank part, a part over its bound or
    /// a control character.
    pub fn new(
        authority: impl Into<String>,
        subject: impl Into<String>,
    ) -> Result<Self, InvalidQualifiedAccount> {
        fn bounded(part: String) -> Result<String, InvalidQualifiedAccount> {
            if part.trim().is_empty()
                || part.len() > MAX_QUALIFIED_ACCOUNT_PART_BYTES
                || part.chars().any(char::is_control)
            {
                return Err(InvalidQualifiedAccount);
            }
            Ok(part)
        }
        Ok(Self {
            authority: bounded(authority.into())?,
            subject: bounded(subject.into())?,
        })
    }

    #[must_use]
    pub fn authority(&self) -> &str {
        &self.authority
    }

    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }
}

/// `<subject> at <authority>`: the presentation text of the account.
impl fmt::Display for QualifiedAccount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at {}", self.subject, self.authority)
    }
}

/// The subject is not rendered.
impl fmt::Debug for QualifiedAccount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QualifiedAccount")
            .field("authority", &self.authority)
            .finish_non_exhaustive()
    }
}

impl OperationAccount {
    /// The account `subject` of `authority`, or [`Self::Unavailable`] when
    /// the owner's facts do not make a valid qualified account.
    #[must_use]
    pub fn selected(authority: impl Into<String>, subject: impl Into<String>) -> Self {
        QualifiedAccount::new(authority, subject).map_or(Self::Unavailable, Self::Selected)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_qualified_account_is_bounded_and_never_rendered_by_debug() {
        let account = QualifiedAccount::new("https://issuer.example", "alice@example.com").unwrap();
        assert_eq!(
            account.to_string(),
            "alice@example.com at https://issuer.example"
        );
        assert!(!format!("{account:?}").contains("alice"));
        for (authority, subject) in [
            ("", "alice"),
            ("https://issuer.example", " "),
            ("https://issuer.example", "alice\nbob"),
        ] {
            assert_eq!(
                QualifiedAccount::new(authority, subject),
                Err(InvalidQualifiedAccount)
            );
        }
        let long = "a".repeat(MAX_QUALIFIED_ACCOUNT_PART_BYTES + 1);
        assert_eq!(
            QualifiedAccount::new("https://issuer.example", long.as_str()),
            Err(InvalidQualifiedAccount)
        );
        assert!(
            QualifiedAccount::new("https://issuer.example", &long[1..]).is_ok(),
            "the bound itself is accepted"
        );
    }

    #[test]
    fn invalid_owner_facts_make_no_account_only_an_unavailable_one() {
        assert_eq!(
            OperationAccount::selected("", "alice"),
            OperationAccount::Unavailable
        );
        assert!(matches!(
            OperationAccount::selected("https://issuer.example", "alice"),
            OperationAccount::Selected(_)
        ));
    }
}
