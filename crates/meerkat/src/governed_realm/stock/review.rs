//! The stock host's owner-selected review: the rules that raise an exact
//! native operation target's review tier above the stock rules' own R1
//! ([`StockReviewRules`], validated from the realm's
//! [`StockReviewConfig`]), and the composition of the review that serves
//! them ([`stock_operation_review`]).
//!
//! A rule only raises a tier. It never allows an operation the stock rules
//! refuse or leave unmapped, and the review it requires is decided by the
//! existing native owners: [`BoundOperationReview`] never selects a tier.
//! An R3 rule also states the weakest approver assurance its human consent
//! accepts, which the stock consent route enforces ([`super::consent`]).
//! With no rule every stock operation stays R1, the review is not composed
//! and no reviewer is built or called.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use meerkat_authorization::publication::LocalAuthorizationPublication;
use meerkat_core::approval::review::{
    BoundOperationReview, OperationReviewer, ReviewCandidate, ReviewVerdict, ReviewerFailure,
};
use meerkat_core::authorization::OperationReviewTier;
use meerkat_core::{
    ApprovalService, Config, ConfigError, StockConsentAssurance, StockReviewConfig,
    StockReviewTarget, StockReviewTier, ToolOperationTarget,
};

use super::consent::{StockConsentSurface, StockHumanConsentRoute};
use crate::operation_reviewer::{
    ModelOperationReviewer, ModelReviewerConfig, ModelReviewerConfigError,
    NativeOperationReviewContextSource,
};

/// The stock rules' owner-selected review requirements: the tier each exact
/// native operation target requires above R1, and for R3 the weakest
/// approver assurance its consent accepts, validated from the realm's
/// [`StockReviewConfig`]. Data only; it grants nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StockReviewRules {
    rules: BTreeMap<StockReviewTarget, Requirement>,
}

/// One rule's requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Requirement {
    tier: OperationReviewTier,
    consent_assurance: Option<StockConsentAssurance>,
}

impl StockReviewRules {
    /// The validated rules of `config`.
    ///
    /// # Errors
    /// [`ConfigError::Validation`] for a malformed review section.
    pub fn from_config(config: &StockReviewConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(Self {
            rules: config
                .rules
                .iter()
                .map(|rule| {
                    (
                        rule.target.clone(),
                        Requirement {
                            tier: tier(rule.tier),
                            consent_assurance: rule.consent_assurance,
                        },
                    )
                })
                .collect(),
        })
    }

    /// No rule: every stock operation stays R1.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The tier the operation of `target` requires: the strictest rule that
    /// names its MCP server, its exact MCP operation or its exact bundle
    /// operation, and R1 when none does (or the operation has no owner
    /// target).
    #[must_use]
    pub fn tier_for(&self, target: Option<&ToolOperationTarget>) -> OperationReviewTier {
        if self.rules.is_empty() {
            return OperationReviewTier::R1;
        }
        self.matching(target)
            .map(|requirement| requirement.tier)
            .fold(OperationReviewTier::R1, std::cmp::max)
    }

    /// The weakest approver assurance the human consent of the operation of
    /// `target` accepts: the strongest that a matching R3 rule states, and
    /// `None` when no R3 rule names the operation.
    #[must_use]
    pub fn consent_assurance_for(
        &self,
        target: Option<&ToolOperationTarget>,
    ) -> Option<StockConsentAssurance> {
        if self.rules.is_empty() {
            return None;
        }
        self.matching(target)
            .filter_map(|requirement| requirement.consent_assurance)
            .max()
    }

    /// The requirements of every rule naming the operation of `target`: its
    /// MCP server, its exact MCP operation or its exact bundle operation. An
    /// MCP operation reached through a checked route is named by the same
    /// server and operation: the route never escapes a rule that names them.
    fn matching(
        &self,
        target: Option<&ToolOperationTarget>,
    ) -> impl Iterator<Item = &Requirement> + '_ {
        let selectors: Vec<StockReviewTarget> = match target {
            Some(
                ToolOperationTarget::McpServerOperation {
                    server,
                    raw_operation,
                    ..
                }
                | ToolOperationTarget::CheckedMcpServerOperation {
                    server,
                    raw_operation,
                    ..
                },
            ) => vec![
                StockReviewTarget::McpServer {
                    server: server.clone(),
                },
                StockReviewTarget::McpOperation {
                    server: server.clone(),
                    operation: raw_operation.clone(),
                },
            ],
            Some(ToolOperationTarget::RegisteredToolBundleOperation {
                bundle, operation, ..
            }) => {
                vec![StockReviewTarget::ToolBundleOperation {
                    bundle: bundle.clone(),
                    operation: operation.clone(),
                }]
            }
            _ => Vec::new(),
        };
        selectors
            .into_iter()
            .filter_map(move |selector| self.rules.get(&selector))
    }
}

/// The review rules of `realm`'s effective configuration: its own document
/// over its parent chain, folded root-first exactly as every host reads it,
/// under the realm root `realms_root` and its `layout`'s global document. The
/// realm's trusted host administers them in those documents; an ordinary
/// config write cannot change them.
///
/// # Errors
/// A configuration that cannot be read, or a malformed review section: the
/// realm then has no rules to compose, and its governed work is refused
/// rather than served under fewer.
pub(crate) async fn read_realm_review_rules(
    realms_root: &Path,
    realm: &meerkat_core::connection::RealmId,
    layout: &meerkat_core::StorageLayout,
) -> Result<StockReviewRules, ConfigError> {
    use meerkat_core::ConfigStore as _;
    let global_doc = layout
        .global_config_path()
        .unwrap_or_else(|| realms_root.join("__no_global__").join("config.toml"));
    let source = meerkat_store::FilesystemRealmConfigSource::new(
        realms_root.to_path_buf(),
        global_doc,
        meerkat_models::canonical(),
    );
    // A read of the realm's own document; nothing is written through this
    // store, so it composes no realm chain.
    let head = meerkat_core::FileConfigStore::new(
        source.config_doc_path(realm),
        meerkat_models::canonical(),
    )
    .get()
    .await?;
    let effective = meerkat_core::EffectiveConfigReader::new(Arc::new(source))
        .effective_config_over_head(realm, head)
        .await?;
    match &effective.governance {
        Some(governance) => StockReviewRules::from_config(&governance.review),
        None => Ok(StockReviewRules::default()),
    }
}

fn tier(tier: StockReviewTier) -> OperationReviewTier {
    match tier {
        StockReviewTier::R2 => OperationReviewTier::R2,
        StockReviewTier::R3 => OperationReviewTier::R3,
    }
}

/// A stock host's human consent for R3: its surface, the composition's
/// publication that surface's login owner publishes on, and the finite
/// validity of each consent request.
pub struct StockConsent {
    surface: Arc<dyn StockConsentSurface>,
    publication: LocalAuthorizationPublication,
    validity: Duration,
}

impl StockConsent {
    /// The consent of `surface` for `governance`'s composition, each request
    /// valid for `validity`. The surface's login owner joins the
    /// composition's publication
    /// ([`super::StockGovernance::authorization_publication`]).
    #[must_use]
    pub fn new(
        governance: &super::StockGovernance,
        surface: Arc<dyn StockConsentSurface>,
        validity: Duration,
    ) -> Self {
        Self {
            surface,
            publication: governance.authorization_publication(),
            validity,
        }
    }
}

/// Why the stock review could not be composed.
#[derive(Debug, thiserror::Error)]
pub enum StockReviewError {
    #[error("the realm's review configuration is invalid: {0}")]
    Config(#[from] ConfigError),
    #[error("the configured reviewer is invalid: {0}")]
    Reviewer(#[from] ModelReviewerConfigError),
    #[error("the configured reviewer could not be built: {0}")]
    Factory(#[from] crate::FactoryError),
}

/// The stock host's operation review, deciding through `approvals`, the
/// realm's one issuing approval service: the configured model reviewer for
/// R2 and the stock consent route over `consent`'s surface for R3, each
/// attempt bounded by `review_deadline`. `None` when the realm's configuration selects no rule,
/// so every stock operation stays R1 and nothing is built or called.
///
/// An R2 operation without a configured reviewer, and an R3 operation
/// without a route, are refused locally with typed feedback; nothing falls
/// back to R1. Install the result on the factory
/// ([`crate::AgentFactory::with_operation_review`]) before it is cloned.
///
/// # Errors
/// An invalid review section, or a configured reviewer the factory cannot
/// build exactly as selected.
pub async fn stock_operation_review(
    factory: &crate::AgentFactory,
    config: &Config,
    approvals: ApprovalService,
    review_deadline: Duration,
    consent: Option<StockConsent>,
) -> Result<Option<Arc<BoundOperationReview>>, StockReviewError> {
    let Some(governance) = &config.governance else {
        return Ok(None);
    };
    let review = &governance.review;
    let rules = StockReviewRules::from_config(review)?;
    if rules.is_empty() {
        return Ok(None);
    }
    let reviewer: Arc<dyn OperationReviewer> = match &review.reviewer {
        Some(selection) => Arc::new(
            ModelOperationReviewer::build(
                factory,
                config,
                ModelReviewerConfig::new(selection.identity.clone(), selection.max_output_tokens)?,
                Arc::new(NativeOperationReviewContextSource),
            )
            .await?,
        ),
        None => Arc::new(UnconfiguredReviewer),
    };
    let mut bound = BoundOperationReview::new(reviewer, approvals, review_deadline);
    if let Some(consent) = consent {
        let route = StockHumanConsentRoute::new(consent.surface, consent.publication, rules);
        bound = bound.with_human_consent(Arc::new(route), consent.validity);
    }
    Ok(Some(Arc::new(bound)))
}

/// The reviewer of a realm whose configuration selects no reviewer model: an
/// R2 operation's review is unavailable, a typed local refusal.
struct UnconfiguredReviewer;

#[async_trait]
impl OperationReviewer for UnconfiguredReviewer {
    async fn review(
        &self,
        _candidate: &ReviewCandidate<'_>,
    ) -> Result<ReviewVerdict, ReviewerFailure> {
        Err(ReviewerFailure::Unavailable)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use meerkat_core::{CatalogOperation, McpConnectorApproval, StockReviewRule};

    use super::*;

    use OperationReviewTier::{R1, R2, R3};
    use StockConsentAssurance::{DeviceTrusted, PersonAuthenticated};

    fn rule(
        target: StockReviewTarget,
        tier: StockReviewTier,
        consent_assurance: Option<StockConsentAssurance>,
    ) -> StockReviewRule {
        StockReviewRule {
            target,
            tier,
            consent_assurance,
        }
    }

    fn rules(rules: Vec<StockReviewRule>) -> StockReviewRules {
        StockReviewRules::from_config(&StockReviewConfig {
            rules,
            reviewer: None,
        })
        .expect("valid rules")
    }

    fn server(server: &str) -> StockReviewTarget {
        StockReviewTarget::McpServer {
            server: server.into(),
        }
    }

    fn operation(server: &str, operation: &str) -> StockReviewTarget {
        StockReviewTarget::McpOperation {
            server: server.into(),
            operation: operation.into(),
        }
    }

    fn mcp(server: &str, raw: &str) -> ToolOperationTarget {
        ToolOperationTarget::McpServerOperation {
            server: server.into(),
            connection_generation: 3,
            raw_operation: raw.into(),
            connector: McpConnectorApproval::NoConnector,
            account: std::sync::Arc::new(meerkat_core::OperationAccount::Unavailable),
        }
    }

    fn bundle(bundle: &str, operation: &str) -> ToolOperationTarget {
        ToolOperationTarget::RegisteredToolBundleOperation {
            bundle: bundle.into(),
            operation: operation.into(),
            account: std::sync::Arc::new(meerkat_core::OperationAccount::Unavailable),
        }
    }

    #[test]
    fn a_rule_raises_the_tier_of_exactly_the_operations_it_names() {
        let rules = rules(vec![
            rule(server("alpha"), StockReviewTier::R2, None),
            rule(
                operation("alpha", "send"),
                StockReviewTier::R3,
                Some(DeviceTrusted),
            ),
            rule(
                StockReviewTarget::ToolBundleOperation {
                    bundle: "files".into(),
                    operation: "delete".into(),
                },
                StockReviewTier::R3,
                Some(PersonAuthenticated),
            ),
        ]);
        // Every operation of the named server, and the strictest rule where
        // the server and the exact operation are both named.
        assert_eq!(rules.tier_for(Some(&mcp("alpha", "read"))), R2);
        assert_eq!(rules.tier_for(Some(&mcp("alpha", "send"))), R3);
        assert_eq!(rules.tier_for(Some(&mcp("beta", "send"))), R1);
        // The same server and operation reached through a checked route
        // never escape a rule that names them.
        let checked = |server: &str, raw: &str| ToolOperationTarget::CheckedMcpServerOperation {
            route: "checked".into(),
            server: server.into(),
            connection_generation: 3,
            raw_operation: raw.into(),
            connector: McpConnectorApproval::NoConnector,
            account: std::sync::Arc::new(meerkat_core::OperationAccount::Unavailable),
        };
        assert_eq!(rules.tier_for(Some(&checked("alpha", "read"))), R2);
        assert_eq!(rules.tier_for(Some(&checked("alpha", "send"))), R3);
        assert_eq!(rules.tier_for(Some(&checked("beta", "send"))), R1);
        assert_eq!(rules.tier_for(Some(&bundle("files", "delete"))), R3);
        assert_eq!(rules.tier_for(Some(&bundle("files", "read"))), R1);
        // A bundle named like a server is another owner's registration.
        assert_eq!(rules.tier_for(Some(&bundle("alpha", "send"))), R1);
        let catalog = ToolOperationTarget::CatalogControl {
            operation: CatalogOperation::Search,
        };
        assert_eq!(rules.tier_for(Some(&catalog)), R1);
        assert_eq!(rules.tier_for(None), R1);
        assert!(StockReviewRules::default().is_empty());
        assert_eq!(
            StockReviewRules::default().tier_for(Some(&mcp("alpha", "send"))),
            R1
        );
    }

    #[test]
    fn the_strongest_consent_assurance_of_the_matching_r3_rules_applies() {
        let rules = rules(vec![
            rule(server("alpha"), StockReviewTier::R3, Some(DeviceTrusted)),
            rule(
                operation("alpha", "send"),
                StockReviewTier::R3,
                Some(PersonAuthenticated),
            ),
            rule(server("beta"), StockReviewTier::R2, None),
        ]);
        assert_eq!(
            rules.consent_assurance_for(Some(&mcp("alpha", "send"))),
            Some(PersonAuthenticated)
        );
        assert_eq!(
            rules.consent_assurance_for(Some(&mcp("alpha", "read"))),
            Some(DeviceTrusted)
        );
        // R2 states none, and an unnamed operation has none to state.
        assert_eq!(
            rules.consent_assurance_for(Some(&mcp("beta", "send"))),
            None
        );
        assert_eq!(rules.consent_assurance_for(None), None);
    }

    /// The rules come from the realm's own document over its parent chain,
    /// as every host reads them, and a malformed section is an error, never
    /// fewer rules.
    #[tokio::test]
    async fn the_rules_are_read_from_the_realms_effective_configuration() {
        let root = tempfile::tempdir().expect("root");
        let realms_root = root.path().join("realms");
        let layout = meerkat_core::StorageLayout::resolve(
            meerkat_core::StorageLayoutInputs {
                invocation_context: root.path().to_path_buf(),
                user_config_root: Some(root.path().join("user")),
                ..meerkat_core::StorageLayoutInputs::default()
            },
            &meerkat_core::RealmConfig {
                selection: meerkat_core::RealmSelection::Explicit {
                    realm_id: "governed".to_string(),
                },
                state_root: Some(realms_root.clone()),
                ..meerkat_core::RealmConfig::default()
            },
        )
        .expect("layout")
        .layout;
        let realm = meerkat_core::connection::RealmId::parse("governed").expect("realm");
        let document = meerkat_store::realm_paths_in(&realms_root, "governed").config_path;
        tokio::fs::create_dir_all(document.parent().expect("realm directory"))
            .await
            .expect("realm directory");
        // No document: no rules.
        assert!(
            read_realm_review_rules(&realms_root, &realm, &layout)
                .await
                .expect("no rules")
                .is_empty()
        );
        tokio::fs::write(
            &document,
            "[[governance.review.rules]]\n\
             target = { kind = \"mcp_operation\", server = \"mail\", operation = \"send\" }\n\
             tier = \"r3\"\n\
             consent_assurance = \"device_trusted\"\n",
        )
        .await
        .expect("document");
        let rules = read_realm_review_rules(&realms_root, &realm, &layout)
            .await
            .expect("rules");
        assert_eq!(rules.tier_for(Some(&mcp("mail", "send"))), R3);
        assert_eq!(
            rules.consent_assurance_for(Some(&mcp("mail", "send"))),
            Some(DeviceTrusted)
        );
        tokio::fs::write(
            &document,
            "[[governance.review.rules]]\n\
             target = { kind = \"mcp_server\", server = \"mail\" }\n\
             tier = \"r3\"\n",
        )
        .await
        .expect("document");
        assert!(
            read_realm_review_rules(&realms_root, &realm, &layout)
                .await
                .is_err()
        );
    }

    #[test]
    fn malformed_rules_are_refused() {
        let config = StockReviewConfig {
            rules: vec![rule(operation("alpha", "send"), StockReviewTier::R3, None)],
            reviewer: None,
        };
        assert!(matches!(
            StockReviewRules::from_config(&config),
            Err(ConfigError::Validation(_))
        ));
    }
}
