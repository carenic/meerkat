//! The route report of an actual pinned controller client.
//!
//! [`controller_route_report`] reads a pinned [`ControllerModelClient`]'s own
//! request-free route facts and reports its route key with a bounded,
//! nonsecret explanation of what that key names. It resolves, fetches and
//! calls nothing, and the report grants nothing: ordinary admission and use
//! checks stay authoritative. The stock governed host
//! (`governed_realm::stock::controller`) re-exports it unchanged, and a mob's
//! held member reports it from the exact client its actor pinned.

use meerkat_core::{ControllerModelClient, ControllerRouteKey};

/// A trusted report of `controller`'s actual lowered route, from the pinned
/// client's own request-free route facts: its key and a bounded, nonsecret
/// explanation of what that key names. `None` when those facts are
/// unavailable or describe another selection. Nothing is resolved, fetched
/// or called, and the report grants nothing.
pub fn controller_route_report(
    controller: &ControllerModelClient,
) -> Option<ControllerRouteReport> {
    // The pin's own native read: the client must still report the pinned
    // selection, and its facts must describe it.
    let facts = controller.plain_facts().ok()?;
    let selection = facts.selection();
    Some(ControllerRouteReport {
        key: facts.route_key(),
        provider: bounded(selection.provider().as_str()),
        model: bounded(selection.model()),
        wire_model: bounded(facts.wire_model()),
        endpoint: bounded(&endpoint_without_query(facts.endpoint())),
        credential_owner: bounded(&credential_owner(selection.credential())),
    })
}

/// The key of `controller`'s actual lowered route
/// ([`controller_route_report`]).
pub fn controller_route_key(controller: &ControllerModelClient) -> Option<ControllerRouteKey> {
    controller_route_report(controller).map(|report| report.key)
}

/// What one controller route key names, for an operator deciding whether to
/// grant it: provider, model, wire model, endpoint (scheme, host, port and
/// path only) and the stable credential owner (never a token). Each value
/// is bounded and stripped of control characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerRouteReport {
    pub key: ControllerRouteKey,
    pub provider: String,
    pub model: String,
    pub wire_model: String,
    pub endpoint: String,
    pub credential_owner: String,
}

impl std::fmt::Display for ControllerRouteReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "provider {}, model {} (wire model {}), endpoint {}, credential owner {}; key {}",
            self.provider,
            self.model,
            self.wire_model,
            self.endpoint,
            self.credential_owner,
            self.key
        )
    }
}

const REPORT_FIELD_LIMIT: usize = 160;

fn bounded(value: &str) -> String {
    let mut out: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(REPORT_FIELD_LIMIT)
        .collect();
    if value.chars().filter(|c| !c.is_control()).count() > REPORT_FIELD_LIMIT {
        out.push_str("...");
    }
    out
}

/// The endpoint for display only, without userinfo, query or fragment,
/// any of which may carry a secret. The route facts and key keep the actual
/// endpoint.
fn endpoint_without_query(endpoint: &str) -> String {
    match reqwest::Url::parse(endpoint) {
        Ok(mut url) => {
            // Setting these fails only for URLs that cannot carry them.
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => "<unparseable endpoint>".to_string(),
    }
}

fn credential_owner(credential: &meerkat_core::AuthCredentialIdentity) -> String {
    match credential {
        meerkat_core::AuthCredentialIdentity::Binding(binding) => match &binding.profile {
            Some(profile) => format!(
                "binding {}/{}/{}",
                binding.realm.as_str(),
                binding.binding.as_str(),
                profile.as_str()
            ),
            None => format!(
                "binding {}/{}",
                binding.realm.as_str(),
                binding.binding.as_str()
            ),
        },
        meerkat_core::AuthCredentialIdentity::Account(account) => format!(
            "account {}/{}",
            account.realm.as_str(),
            account.account.as_str()
        ),
    }
}

#[cfg(test)]
mod report_tests {
    use super::*;

    #[test]
    fn a_report_drops_the_endpoint_query_and_bounds_every_field() {
        assert_eq!(
            endpoint_without_query("https://models.invalid/v1/x?key=secret-canary#frag"),
            "https://models.invalid/v1/x"
        );
        let long = "m".repeat(400);
        let shown = bounded(&format!("{long}\u{7}"));
        assert_eq!(shown.chars().count(), REPORT_FIELD_LIMIT + 3);
        assert!(!shown.contains('\u{7}'));
    }
}
