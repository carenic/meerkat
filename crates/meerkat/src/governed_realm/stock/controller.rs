//! Stock rkat's controller pin: the actual selected controller client the
//! stock factory built for the session, handed to the scoped original
//! composition as its [`RetainedControllerOwner`].
//!
//! The pin is the client itself, never rebuilt from a selection: a request
//! for any other selection gets no client. The CLI ingress pins the same
//! client on the input it admits (`NativeIngressContext::with_controller_client`).

use std::sync::Arc;

use meerkat_core::authorization::ControllerReadinessFailure;
use meerkat_core::{ControllerModelClient, ControllerModelSelection};
use meerkat_runtime::meerkat_machine::RetainedControllerOwner;

/// The route report of a pinned controller client, defined in
/// [`crate::controller_route`] and re-exported here unchanged.
pub use crate::controller_route::{
    ControllerRouteReport, controller_route_key, controller_route_report,
};

pub struct StockControllerPin {
    client: ControllerModelClient,
}

impl StockControllerPin {
    pub fn new(client: ControllerModelClient) -> Arc<Self> {
        Arc::new(Self { client })
    }

    /// The pinned client, for the ingress pin of each admitted input.
    pub fn client(&self) -> &ControllerModelClient {
        &self.client
    }
}

impl RetainedControllerOwner for StockControllerPin {
    fn reacquire_controller(
        &self,
        selection: &ControllerModelSelection,
    ) -> Result<Option<ControllerModelClient>, ControllerReadinessFailure> {
        Ok((self.client.selection() == selection).then(|| self.client.clone()))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    struct Client(ControllerModelSelection);

    /// A client that reports route facts, for the selection it carries.
    struct Routed {
        reported: Option<ControllerModelSelection>,
        facts_for: ControllerModelSelection,
        endpoint: &'static str,
    }

    #[async_trait::async_trait]
    impl meerkat_core::AgentLlmClient for Routed {
        fn controller_model_selection(&self) -> Option<ControllerModelSelection> {
            self.reported.clone()
        }
        fn controller_model_facts(
            &self,
        ) -> Result<meerkat_core::ControllerModelFacts, meerkat_core::ControllerFactsUnavailable>
        {
            Ok(meerkat_core::ControllerModelFacts::new(
                self.facts_for.clone(),
                Arc::from(self.endpoint),
                Arc::from("wire-model"),
            ))
        }
        async fn stream_response(
            &self,
            _: &[meerkat_core::Message],
            _: &[Arc<meerkat_core::ToolDef>],
            _: u32,
            _: Option<f32>,
            _: Option<&meerkat_core::ProviderParamsOverride>,
        ) -> Result<meerkat_core::LlmStreamResult, meerkat_core::AgentError> {
            Err(meerkat_core::AgentError::ConfigError(
                "route fixture".into(),
            ))
        }
        fn provider(&self) -> meerkat_core::Provider {
            self.facts_for.provider()
        }
        fn model(&self) -> &str {
            self.facts_for.model()
        }
    }

    #[test]
    fn the_route_key_comes_from_the_pinned_clients_own_facts() {
        const ENDPOINT: &str = "https://models.invalid/v1/messages";
        let pinned = selection("operator");
        let routed = |reported: Option<ControllerModelSelection>,
                      facts_for: ControllerModelSelection,
                      endpoint: &'static str| {
            ControllerModelClient::new(
                pinned.clone(),
                Arc::new(Routed {
                    reported,
                    facts_for,
                    endpoint,
                }),
            )
        };
        let actual = routed(Some(pinned.clone()), pinned.clone(), ENDPOINT);
        let expected = meerkat_core::ControllerModelFacts::new(
            pinned.clone(),
            Arc::from(ENDPOINT),
            Arc::from("wire-model"),
        )
        .route_key();
        assert_eq!(controller_route_key(&actual), Some(expected.clone()));
        let report = controller_route_report(&actual).expect("report");
        assert_eq!(report.provider, "anthropic");
        assert_eq!(report.model, "claude-sonnet-4-5");
        assert_eq!(report.wire_model, "wire-model");
        assert_eq!(report.endpoint, ENDPOINT);
        assert_eq!(report.credential_owner, "account stock/operator");
        let shown = report.to_string();
        assert!(shown.contains(expected.as_str()), "{shown}");

        // The pin's native read refuses, so there is no report: facts for
        // another selection, a client that independently reports another
        // selection or none, or a client without facts.
        let other = selection("someone-else");
        for refused in [
            routed(Some(pinned.clone()), other.clone(), ENDPOINT),
            routed(Some(other), pinned.clone(), ENDPOINT),
            routed(None, pinned.clone(), ENDPOINT),
            ControllerModelClient::new(pinned.clone(), Arc::new(Client(pinned.clone()))),
        ] {
            assert_eq!(controller_route_report(&refused), None);
        }

        // Userinfo is never shown; the key still names the actual endpoint.
        const WITH_USERINFO: &str =
            "https://user:secret-canary@models.invalid/v1/messages?key=query-canary";
        let with_userinfo = routed(Some(pinned.clone()), pinned.clone(), WITH_USERINFO);
        let report = controller_route_report(&with_userinfo).expect("report");
        assert_eq!(report.endpoint, ENDPOINT);
        let shown = report.to_string();
        assert!(
            !shown.contains("secret-canary") && !shown.contains("query-canary"),
            "{shown}"
        );
        assert_eq!(
            report.key,
            meerkat_core::ControllerModelFacts::new(
                pinned.clone(),
                Arc::from(WITH_USERINFO),
                Arc::from("wire-model"),
            )
            .route_key()
        );
        assert_ne!(report.key, expected);
    }

    #[async_trait::async_trait]
    impl meerkat_core::AgentLlmClient for Client {
        fn controller_model_selection(&self) -> Option<ControllerModelSelection> {
            Some(self.0.clone())
        }
        async fn stream_response(
            &self,
            _: &[meerkat_core::Message],
            _: &[Arc<meerkat_core::ToolDef>],
            _: u32,
            _: Option<f32>,
            _: Option<&meerkat_core::ProviderParamsOverride>,
        ) -> Result<meerkat_core::LlmStreamResult, meerkat_core::AgentError> {
            Err(meerkat_core::AgentError::ConfigError("pin fixture".into()))
        }
        fn provider(&self) -> meerkat_core::Provider {
            self.0.provider()
        }
        fn model(&self) -> &str {
            self.0.model()
        }
    }

    fn selection(account: &str) -> ControllerModelSelection {
        ControllerModelSelection::new(
            meerkat_core::SessionLlmIdentity {
                model: "claude-sonnet-4-5".into(),
                provider: meerkat_core::Provider::Anthropic,
                self_hosted_server_id: None,
                provider_params: None,
                auth_binding: None,
            },
            serde_json::from_value(serde_json::json!({"realm": "stock", "account": account}))
                .expect("credential identity"),
            "profile".into(),
            "transport".into(),
        )
    }

    #[test]
    fn the_pin_answers_only_its_own_selection_with_the_actual_client() {
        let pinned = selection("operator");
        let client = ControllerModelClient::new(pinned.clone(), Arc::new(Client(pinned.clone())));
        let pin = StockControllerPin::new(client.clone());
        let reacquired = pin
            .reacquire_controller(&pinned)
            .expect("answerable")
            .expect("the pinned selection");
        assert!(Arc::ptr_eq(reacquired.client(), client.client()));
        assert!(Arc::ptr_eq(pin.client().client(), client.client()));
        assert!(
            pin.reacquire_controller(&selection("someone-else"))
                .expect("answerable")
                .is_none(),
            "no client is built for another selection"
        );
    }
}
