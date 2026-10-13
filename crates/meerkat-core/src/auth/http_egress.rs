//! Typed refusals of a host-owned HTTP egress boundary.
//!
//! The boundary itself (profiles, admission and the clients' own resolver)
//! lives in `meerkat-auth-core`. Its refusal vocabulary lives here so that a
//! coordinated credential refresh carries it in the one result it shares
//! with every reader ([`super::RefreshError::EgressRefused`]).

/// Why a required profile refused a destination. Local and typed; it carries
/// no URL text, so it never renders a credential-bearing query or path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum HttpEgressRefusal {
    #[error("the destination is not a valid HTTP URL")]
    InvalidDestination,
    #[error("the destination scheme is outside the host egress profile")]
    SchemeNotAdmitted,
    #[error("the destination host is outside the host egress profile")]
    HostNotAdmitted,
    #[error("the destination port is outside the host egress profile")]
    PortNotAdmitted,
    #[error("the destination address is outside the host egress profile")]
    AddressNotAdmitted,
}
