use crate::constraints::*;

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use meerkat_core::auth::{PrincipalKind, PrincipalRef};
    use serde::Serialize;
    use std::collections::BTreeSet;

    #[test]
    fn unit_bounds_reject_reserved_null_payloads() {
        assert!(
            serde_json::from_str::<SetBound<String>>(r#"{"bound":"unrestricted","values":null}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<DepthBound>(r#"{"bound":"unrestricted","edges":null}"#).is_err()
        );
        for bound in ["unrestricted", "empty"] {
            let wire = serde_json::json!({"bound":bound,"window":null});
            assert!(serde_json::from_value::<LifetimeBound>(wire).is_err());
        }
    }

    fn principal(id: &str) -> PrincipalRef {
        PrincipalRef::new(PrincipalKind::ServiceAccount, id).expect("test principal")
    }

    #[test]
    fn empty_unrestricted_and_all_unresolved_states_remain_distinct() {
        let empty = ExactRestriction::<String>::exact([]);
        let unrestricted = ExactRestriction::<String>::unrestricted();
        assert_eq!(empty.check(&"read".into()), BoundMatch::OutsideBounds);
        assert_eq!(unrestricted.check(&"read".into()), BoundMatch::Matches);
        for fact in [
            UnresolvedConstraint::Absent,
            UnresolvedConstraint::Unknown,
            UnresolvedConstraint::Unavailable,
        ] {
            let unresolved = ExactRestriction::unresolved(fact);
            let combined = empty.conjoin(&unresolved);
            assert_eq!(combined.bound(), &SetBound::Exact(BTreeSet::new()));
            assert_eq!(
                combined.check(&"read".into()),
                BoundMatch::Unresolved(BTreeSet::from([fact]))
            );
            let wire = serde_json::to_vec(&combined).expect("serialize restriction");
            assert_eq!(
                serde_json::from_slice::<ExactRestriction<String>>(&wire)
                    .expect("deserialize restriction"),
                combined
            );
        }
        assert!(
            serde_json::from_str::<ExactRestriction<String>>(
                r#"{"bound":{"bound":"unrestricted"}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn conjunction_preserves_every_unresolved_contributor_and_known_bound() {
        let known = ExactRestriction::exact(["alice", "bob"]);
        let unknown = ExactRestriction::unresolved(UnresolvedConstraint::Unknown);
        let unavailable = ExactRestriction::unresolved(UnresolvedConstraint::Unavailable);
        let result = known.conjoin(&unknown).conjoin(&unavailable);
        assert_eq!(result.bound(), known.bound());
        assert_eq!(
            result.unresolved_facts(),
            &BTreeSet::from([
                UnresolvedConstraint::Unknown,
                UnresolvedConstraint::Unavailable
            ])
        );
    }

    #[test]
    fn exact_domain_authority_and_action_values_do_not_infer_wildcards() {
        let first = ResourceDomain {
            authority: principal("first"),
            namespace: "private".into(),
        };
        let second = ResourceDomain {
            authority: principal("second"),
            namespace: "private".into(),
        };
        let joined =
            ExactRestriction::exact([first.clone()]).conjoin(&ExactRestriction::exact([second]));
        assert_eq!(joined.check(&first), BoundMatch::OutsideBounds);
        let wildcard_text = ActionRef {
            feature: "records".into(),
            action: "*".into(),
        };
        let read = ActionRef {
            feature: "records".into(),
            action: "read".into(),
        };
        assert_eq!(
            ExactRestriction::exact([wildcard_text]).check(&read),
            BoundMatch::OutsideBounds
        );
    }

    #[test]
    fn exact_conjunction_is_associative_commutative_idempotent_and_never_widens() {
        // Exhaustive finite domains, rather than examples that mirror intersection.
        let mut bounds = vec![ExactRestriction::unrestricted()];
        for mask in 0_u8..8 {
            bounds.push(ExactRestriction::exact(
                (0_u8..3).filter(|n| mask & (1 << n) != 0),
            ));
        }
        for fact in [
            UnresolvedConstraint::Absent,
            UnresolvedConstraint::Unknown,
            UnresolvedConstraint::Unavailable,
        ] {
            bounds.push(ExactRestriction::unresolved(fact));
        }
        for a in &bounds {
            assert_eq!(a.conjoin(a), *a);
            for b in &bounds {
                let child = a.conjoin(b);
                assert_eq!(child, b.conjoin(a));
                for value in 0..4 {
                    if child.check(&value) == BoundMatch::Matches {
                        assert_eq!(a.check(&value), BoundMatch::Matches);
                        assert_eq!(b.check(&value), BoundMatch::Matches);
                    }
                }
                for c in &bounds {
                    assert_eq!(a.conjoin(b).conjoin(c), a.conjoin(&b.conjoin(c)));
                }
            }
        }
    }

    #[test]
    fn lifetime_conjunction_never_extends_parent_and_expiry_is_exclusive() {
        let parent = LifetimeRestriction::window(10, 20);
        let child = parent.conjoin(&LifetimeRestriction::window(0, u64::MAX));
        assert_eq!(child, parent);
        assert_eq!(child.check(9), BoundMatch::OutsideBounds);
        assert_eq!(child.check(10), BoundMatch::Matches);
        assert_eq!(child.check(19), BoundMatch::Matches);
        assert_eq!(child.check(20), BoundMatch::OutsideBounds);
        assert_eq!(
            parent
                .conjoin(&LifetimeRestriction::window(20, 30))
                .check(20),
            BoundMatch::OutsideBounds
        );
        assert_eq!(
            LifetimeRestriction::window(20, 10).check(15),
            BoundMatch::OutsideBounds
        );
        assert!(matches!(
            parent
                .conjoin(&LifetimeRestriction::unresolved(
                    UnresolvedConstraint::Unknown
                ))
                .check(15),
            BoundMatch::Unresolved(_)
        ));
    }

    #[test]
    fn delegated_child_cannot_replace_parent_bounds_or_reset_depth() {
        let read = ActionRef {
            feature: "records".into(),
            action: "read".into(),
        };
        let write = ActionRef {
            feature: "records".into(),
            action: "write".into(),
        };
        let mut parent = ExecutionRestrictions::unrestricted();
        parent.actions = ExactRestriction::exact([read.clone()]);
        parent.lifetime = LifetimeRestriction::window(100, 200);
        parent.delegation_depth = DelegationDepth::remaining(1);
        let mut request = ExecutionRestrictions::unrestricted();
        request.actions = ExactRestriction::exact([read.clone(), write.clone()]);
        request.delegation_depth = DelegationDepth::remaining(u32::MAX);
        let child = parent.for_child(&request).expect("one child edge");
        assert_eq!(child.actions.check(&read), BoundMatch::Matches);
        assert_eq!(child.actions.check(&write), BoundMatch::OutsideBounds);
        assert_eq!(child.lifetime.check(200), BoundMatch::OutsideBounds);
        assert_eq!(child.for_child(&request), Err(DelegationFailure::Exhausted));
        parent.delegation_depth = DelegationDepth::unresolved(UnresolvedConstraint::Unavailable);
        assert!(matches!(
            parent.for_child(&request),
            Err(DelegationFailure::Unresolved(_))
        ));
    }

    #[test]
    fn current_use_requires_every_dimension_to_be_known_even_when_depth_is_zero() {
        let action = ActionRef {
            feature: "records".into(),
            action: "read".into(),
        };
        let domain = ResourceDomain {
            authority: principal("owner"),
            namespace: "records".into(),
        };
        let processor = ProcessorRef::Principal {
            principal: principal("processor"),
        };
        let audience = AudienceRef::Principal {
            principal: principal("recipient"),
        };
        let values = OperationRestrictionValues {
            action: &action,
            resource_domain: &domain,
            processor: &processor,
            audience: &audience,
            now_ms: 100,
        };
        let mut known = ExecutionRestrictions::unrestricted();
        known.delegation_depth = DelegationDepth::remaining(0);
        assert_eq!(known.check_bounds(values), Ok(()));
        for dimension in [
            RestrictionDimension::Action,
            RestrictionDimension::ResourceDomain,
            RestrictionDimension::Processor,
            RestrictionDimension::Audience,
            RestrictionDimension::Lifetime,
            RestrictionDimension::DelegationDepth,
        ] {
            let mut unknown = known.clone();
            match dimension {
                RestrictionDimension::Action => {
                    unknown.actions = ExactRestriction::unresolved(UnresolvedConstraint::Unknown);
                }
                RestrictionDimension::ResourceDomain => {
                    unknown.resource_domains =
                        DomainRestriction::unresolved(UnresolvedConstraint::Unknown);
                }
                RestrictionDimension::Processor => {
                    unknown.processors =
                        ExactRestriction::unresolved(UnresolvedConstraint::Unknown);
                }
                RestrictionDimension::Audience => {
                    unknown.audiences = ExactRestriction::unresolved(UnresolvedConstraint::Unknown);
                }
                RestrictionDimension::Lifetime => {
                    unknown.lifetime =
                        LifetimeRestriction::unresolved(UnresolvedConstraint::Unknown);
                }
                RestrictionDimension::DelegationDepth => {
                    unknown.delegation_depth =
                        DelegationDepth::unresolved(UnresolvedConstraint::Unknown);
                }
            }
            assert_eq!(
                unknown.check_bounds(values),
                Err(RestrictionFailure::Unresolved {
                    dimension,
                    facts: BTreeSet::from([UnresolvedConstraint::Unknown])
                })
            );
        }
    }

    #[test]
    fn security_sensitive_wire_enums_reject_unknown_fields() {
        fn reject_extra<T: Serialize + serde::de::DeserializeOwned>(value: &T) {
            let mut wire = serde_json::to_value(value).expect("wire enum");
            wire.as_object_mut()
                .expect("tagged object")
                .insert("future_broadening".into(), serde_json::json!(true));
            assert!(
                serde_json::from_value::<T>(wire.clone()).is_err(),
                "{} accepted {wire}",
                std::any::type_name::<T>()
            );
        }
        reject_extra(&SetBound::<String>::Unrestricted);
        reject_extra(&SetBound::Exact(BTreeSet::from(["value".to_owned()])));
        reject_extra(&LifetimeBound::Unrestricted);
        reject_extra(&LifetimeBound::Empty);
        reject_extra(&LifetimeBound::Window {
            not_before_ms: 1,
            expires_at_ms: 2,
        });
        assert!(
            serde_json::from_str::<LifetimeBound>(
                r#"{"bound":"unrestricted","window":{"expires_at_ms":20}}"#
            )
            .is_err()
        );
        assert!(serde_json::from_str::<LifetimeBound>(r#"{"bound":"window","window":{"not_before_ms":1,"expires_at_ms":20,"ignore_expiry":true}}"#).is_err());
        reject_extra(&DepthBound::Unrestricted);
        reject_extra(&DepthBound::Remaining(1));
        reject_extra(&ProcessorRef::Principal {
            principal: principal("p"),
        });
        reject_extra(&ProcessorRef::Route {
            authority: principal("p"),
            route_id: "r".into(),
        });
        reject_extra(&AudienceRef::Principal {
            principal: principal("p"),
        });
        reject_extra(&AudienceRef::Destination {
            authority: principal("p"),
            destination_id: "d".into(),
        });
    }

    #[test]
    fn audience_and_processor_are_independently_exact() {
        let alice = AudienceRef::Principal {
            principal: principal("alice"),
        };
        let bob = AudienceRef::Principal {
            principal: principal("bob"),
        };
        let route = ProcessorRef::Route {
            authority: principal("provider"),
            route_id: "bound-route".into(),
        };
        let another = ProcessorRef::Route {
            authority: principal("provider"),
            route_id: "other-route".into(),
        };
        assert_eq!(
            ExactRestriction::exact([alice.clone()])
                .conjoin(&ExactRestriction::exact([bob]))
                .check(&alice),
            BoundMatch::OutsideBounds
        );
        assert_eq!(
            ExactRestriction::exact([route]).check(&another),
            BoundMatch::OutsideBounds
        );
    }
    #[test]
    fn shared_json_set_and_lifetime_conformance_vectors() {
        use crate::conformance_tests as fixture;
        use serde::de::DeserializeOwned;
        use std::fmt::Debug;

        fn outcome(result: &BoundMatch) -> &'static str {
            match result {
                BoundMatch::Matches => "matches",
                BoundMatch::OutsideBounds => "outside_bounds",
                BoundMatch::Unresolved(_) => "unresolved",
            }
        }
        fn check_set<T: Ord + Clone + Debug + DeserializeOwned>(
            case: &serde_json::Value,
            atoms: &serde_json::Value,
        ) {
            let mut actual = ExactRestriction::<T>::unrestricted();
            for input in fixture::entries(&case["inputs"]) {
                actual = actual.conjoin(&fixture::restriction(input, atoms));
            }
            assert_eq!(
                actual,
                fixture::restriction(&case["expected"], atoms),
                "{}",
                case["id"]
            );
            for probe in fixture::entries(&case["probes"]) {
                let value: T = fixture::decode(&atoms[fixture::name(&probe["value"])]);
                assert_eq!(
                    outcome(&actual.check(&value)),
                    fixture::name(&probe["expected"]),
                    "{}",
                    case["id"]
                );
            }
        }
        // The resource-domain dimension is a `DomainRestriction`; its exact
        // vectors keep the same wire and expectations.
        fn check_domain_set(case: &serde_json::Value, corpus: &serde_json::Value) {
            let atoms = &corpus["atoms"]["resource_domains"];
            let mut actual = DomainRestriction::unrestricted();
            for input in fixture::entries(&case["inputs"]) {
                actual = actual.conjoin(&fixture::domain_restriction(input, corpus));
            }
            assert_eq!(
                actual,
                fixture::domain_restriction(&case["expected"], corpus),
                "{}",
                case["id"]
            );
            for probe in fixture::entries(&case["probes"]) {
                let value: ResourceDomain = fixture::decode(&atoms[fixture::name(&probe["value"])]);
                assert_eq!(
                    outcome(&actual.check(&value)),
                    fixture::name(&probe["expected"]),
                    "{}",
                    case["id"]
                );
            }
        }
        let corpus = fixture::corpus();
        for case in fixture::entries(&corpus["set_cases"]) {
            let dimension = fixture::name(&case["dimension"]);
            let atoms = &corpus["atoms"][dimension];
            match dimension {
                "actions" => check_set::<ActionRef>(case, atoms),
                "resource_domains" => check_domain_set(case, &corpus),
                "processors" => check_set::<ProcessorRef>(case, atoms),
                other => {
                    assert_eq!(other, "audiences", "unsupported corpus dimension");
                    check_set::<AudienceRef>(case, atoms);
                }
            }
        }
        for case in fixture::entries(&corpus["lifetime_cases"]) {
            let mut actual = LifetimeRestriction::unrestricted();
            for input in fixture::entries(&case["inputs"]) {
                actual = actual.conjoin(&fixture::lifetime(input));
            }
            assert_eq!(
                actual,
                fixture::lifetime(&case["expected"]),
                "{}",
                case["id"]
            );
            for probe in fixture::entries(&case["probes"]) {
                let now = probe["now_ms"].as_u64().expect("exact Unix milliseconds");
                assert_eq!(
                    outcome(&actual.check(now)),
                    fixture::name(&probe["expected"]),
                    "{}",
                    case["id"]
                );
            }
        }
        assert_eq!(fixture::entries(&corpus["set_cases"]).len(), 9);
        assert_eq!(fixture::entries(&corpus["lifetime_cases"]).len(), 3);
    }

    #[test]
    fn shared_json_parent_attenuation_and_depth_vectors() {
        use crate::conformance_tests as fixture;
        let corpus = fixture::corpus();
        for case in fixture::entries(&corpus["depth_cases"]) {
            let parent = fixture::depth(&case["parent"]);
            let requested = fixture::depth(&case["requested"]);
            let actual = parent.for_child().map(|child| child.conjoin(&requested));
            if let Some(error) = case.get("expected_error") {
                let actual_error = match actual.expect_err("fixture requires refusal") {
                    DelegationFailure::Exhausted => "exhausted",
                    DelegationFailure::Unresolved(_) => "unresolved",
                };
                assert_eq!(actual_error, fixture::name(error), "{}", case["id"]);
            } else {
                assert_eq!(
                    actual.expect("fixture permits child computation"),
                    fixture::depth(&case["expected"]),
                    "{}",
                    case["id"]
                );
            }
        }
        for case in fixture::entries(&corpus["attenuation_cases"]) {
            let parent = fixture::execution(&case["parent"], &corpus);
            let requested = fixture::execution(&case["requested"], &corpus);
            let actual = parent
                .for_child(&requested)
                .expect("fixture child computation");
            assert_eq!(
                actual,
                fixture::execution(&case["expected"], &corpus),
                "{}",
                case["id"]
            );
        }
        assert_eq!(fixture::entries(&corpus["depth_cases"]).len(), 5);
        assert_eq!(fixture::entries(&corpus["attenuation_cases"]).len(), 3);
    }
}

#[test]
#[allow(clippy::expect_used)]
fn readonly_scalar_projections_match_raw_bounds_and_preserve_wire() {
    use serde_json::json;
    for start in 0..4 {
        for end in 0..4 {
            let value = LifetimeRestriction::window(start, end);
            let bound = if start < end {
                json!({"bound":"window","window":{"not_before_ms":start,"expires_at_ms":end}})
            } else {
                json!({"bound":"empty"})
            };
            let wire = json!({"bound":bound,"unresolved":[]});
            assert_eq!(serde_json::to_value(&value).expect("encode"), wire);
            let decoded: LifetimeRestriction =
                serde_json::from_value(wire.clone()).expect("decode");
            assert_eq!(decoded, value);
            for now in 0..5 {
                let projected = matches!(decoded.bound, LifetimeBound::Unrestricted)
                    || (matches!(decoded.bound, LifetimeBound::Window { .. })
                        && decoded.not_before_ms <= now
                        && now < decoded.expires_at_ms);
                assert_eq!(projected, matches!(decoded.check(now), BoundMatch::Matches));
            }
            let mut forged = wire;
            forged["expires_at_ms"] = json!(999);
            assert!(serde_json::from_value::<LifetimeRestriction>(forged).is_err());
        }
    }
    for edges in [0, 1, 2, u32::MAX] {
        let value = DelegationDepth::remaining(edges);
        let wire = json!({"bound":{"bound":"remaining","edges":edges},"unresolved":[]});
        assert_eq!(serde_json::to_value(&value).expect("encode"), wire);
        let decoded: DelegationDepth = serde_json::from_value(wire.clone()).expect("decode");
        assert_eq!(decoded.remaining_edges, u64::from(edges));
        assert_eq!(decoded.bound(), DepthBound::Remaining(edges));
        let mut forged = wire;
        forged["remaining_edges"] = json!(999);
        assert!(serde_json::from_value::<DelegationDepth>(forged).is_err());
    }
}

#[test]
#[allow(clippy::expect_used)]
fn projection_wrappers_keep_existing_literal_wire_bytes() {
    assert_eq!(
        serde_json::to_string(&LifetimeRestriction::window(10, 20)).expect("encode"),
        r#"{"bound":{"bound":"window","window":{"not_before_ms":10,"expires_at_ms":20}},"unresolved":[]}"#
    );
    assert_eq!(
        serde_json::to_string(&DelegationDepth::remaining(2)).expect("encode"),
        r#"{"bound":{"bound":"remaining","edges":2},"unresolved":[]}"#
    );
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod domain_restriction_tests {
    use super::*;
    use meerkat_core::auth::{PrincipalKind, PrincipalRef};
    use serde_json::json;
    use std::collections::BTreeSet;

    fn principal(id: &str) -> PrincipalRef {
        PrincipalRef::new(PrincipalKind::ServiceAccount, id).expect("test principal")
    }

    fn domain(authority: &PrincipalRef, namespace: &str) -> ResourceDomain {
        ResourceDomain {
            authority: authority.clone(),
            namespace: namespace.into(),
        }
    }

    #[test]
    fn owned_by_refuses_equal_namespace_under_another_authority() {
        let app = principal("app");
        let other = principal("other-app");
        let bound = DomainRestriction::owned_by([app.clone()]);
        for namespace in ["toolkit-session-catalog/s1", "toolkit-session-setup/s2", ""] {
            assert_eq!(bound.check(&domain(&app, namespace)), BoundMatch::Matches);
            assert_eq!(
                bound.check(&domain(&other, namespace)),
                BoundMatch::OutsideBounds
            );
        }
        assert!(matches!(
            bound
                .conjoin(&DomainRestriction::unresolved(
                    UnresolvedConstraint::Unknown
                ))
                .check(&domain(&app, "records")),
            BoundMatch::Unresolved(_)
        ));
    }

    #[test]
    fn owned_by_parent_meets_another_authority_as_empty() {
        let app = principal("app");
        let other = principal("other-app");
        let parent = DomainRestriction::owned_by([app.clone()]);
        let foreign = parent.conjoin(&DomainRestriction::exact([domain(&other, "s1")]));
        assert_eq!(foreign.bound(), &DomainBound::Exact(BTreeSet::new()));
        assert_eq!(
            foreign.check(&domain(&other, "s1")),
            BoundMatch::OutsideBounds
        );
        assert_eq!(
            foreign.check(&domain(&app, "s1")),
            BoundMatch::OutsideBounds
        );
        let own = parent.conjoin(&DomainRestriction::exact([
            domain(&app, "s1"),
            domain(&other, "s1"),
        ]));
        assert_eq!(own, DomainRestriction::exact([domain(&app, "s1")]));
    }

    #[test]
    fn child_cannot_widen_owned_by() {
        let app = principal("app");
        let parent = DomainRestriction::owned_by([app.clone()]);
        assert_eq!(parent.conjoin(&DomainRestriction::unrestricted()), parent);
        assert_eq!(
            parent.conjoin(&DomainRestriction::owned_by([app, principal("other-app")])),
            parent
        );
    }

    fn session_values<'a>(
        domain: &'a ResourceDomain,
        action: &'a ActionRef,
        processor: &'a ProcessorRef,
        audience: &'a AudienceRef,
    ) -> OperationRestrictionValues<'a> {
        OperationRestrictionValues {
            action,
            resource_domain: domain,
            processor,
            audience,
            now_ms: 150,
        }
    }

    /// A child grant under an owner-scoped parent: naming another
    /// authority's domain permits nothing, naming the owner's own domain
    /// permits only that domain, and asking for more keeps the owner bound.
    #[test]
    fn child_of_owned_by_parent_naming_another_authority_permits_nothing() {
        let app = principal("app");
        let other = principal("other-app");
        let action = ActionRef {
            feature: "catalog".into(),
            action: "read".into(),
        };
        let processor = ProcessorRef::Principal {
            principal: principal("helper"),
        };
        let audience = AudienceRef::Principal {
            principal: principal("helper"),
        };
        let mut parent = ExecutionRestrictions::unrestricted();
        parent.resource_domains = DomainRestriction::owned_by([app.clone()]);
        parent.delegation_depth = DelegationDepth::remaining(1);
        let mut foreign = ExecutionRestrictions::unrestricted();
        foreign.resource_domains = DomainRestriction::exact([domain(&other, "s1")]);
        let child = parent.for_child(&foreign).expect("one child edge");
        assert_eq!(child.resource_domains, DomainRestriction::exact([]));
        assert_eq!(
            child.check_bounds(session_values(
                &domain(&other, "s1"),
                &action,
                &processor,
                &audience
            )),
            Err(RestrictionFailure::OutsideBounds {
                dimension: RestrictionDimension::ResourceDomain
            })
        );
        let mut own = ExecutionRestrictions::unrestricted();
        own.resource_domains = DomainRestriction::exact([domain(&app, "s1")]);
        let child = parent.for_child(&own).expect("one child edge");
        assert_eq!(
            child.check_bounds(session_values(
                &domain(&app, "s1"),
                &action,
                &processor,
                &audience
            )),
            Ok(())
        );
        assert_eq!(
            child.check_bounds(session_values(
                &domain(&app, "s2"),
                &action,
                &processor,
                &audience
            )),
            Err(RestrictionFailure::OutsideBounds {
                dimension: RestrictionDimension::ResourceDomain
            })
        );
        let wider = parent
            .for_child(&ExecutionRestrictions::unrestricted())
            .expect("one child edge");
        assert_eq!(wider.resource_domains, DomainRestriction::owned_by([app]));
    }

    #[test]
    fn empty_owner_set_is_exact_empty() {
        assert_eq!(
            DomainRestriction::owned_by([]),
            DomainRestriction::exact([])
        );
        assert_eq!(
            DomainRestriction::owned_by([principal("a")])
                .conjoin(&DomainRestriction::owned_by([principal("b")])),
            DomainRestriction::exact([])
        );
    }

    /// Exhaustive over three authorities and three namespaces. Exact sets list
    /// two authorities and two namespaces, so neither an owner bound nor the
    /// unrestricted bound is ever semantically equal to a finite exact set
    /// over the probes, and structural equality must equal semantic equality.
    #[test]
    fn domain_meet_is_exact_intersection_in_canonical_form() {
        let owners = [principal("a"), principal("b")];
        let listed: Vec<ResourceDomain> = owners
            .iter()
            .flat_map(|owner| ["n1", "n2"].map(|namespace| domain(owner, namespace)))
            .collect();
        let probes: Vec<ResourceDomain> = [principal("a"), principal("b"), principal("c")]
            .iter()
            .flat_map(|owner| ["n1", "n2", "n3"].map(|namespace| domain(owner, namespace)))
            .collect();
        let mut bounds = vec![DomainRestriction::unrestricted()];
        for mask in 0_u8..16 {
            bounds.push(DomainRestriction::exact(
                listed
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| mask & (1 << index) != 0)
                    .map(|(_, value)| value.clone()),
            ));
        }
        for mask in 1_u8..4 {
            bounds.push(DomainRestriction::owned_by(
                owners
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| mask & (1 << index) != 0)
                    .map(|(_, owner)| owner.clone()),
            ));
        }
        let matches = |bound: &DomainRestriction, value| bound.check(value) == BoundMatch::Matches;
        for x in &bounds {
            assert_eq!(&x.conjoin(x), x, "idempotent");
            for y in &bounds {
                let meet = x.conjoin(y);
                assert_eq!(meet, y.conjoin(x), "commutative");
                for value in &probes {
                    assert_eq!(
                        matches(&meet, value),
                        matches(x, value) && matches(y, value),
                        "the meet is the exact intersection"
                    );
                }
                let same_match_set = probes
                    .iter()
                    .all(|value| matches(x, value) == matches(y, value));
                assert_eq!(x == y, same_match_set, "canonical form");
                for z in &bounds {
                    assert_eq!(meet.conjoin(z), x.conjoin(&y.conjoin(z)), "associative");
                }
            }
        }
    }

    #[test]
    fn domain_decode_refuses_noncanonical_unknown_and_reserved_shapes() {
        let owner = serde_json::to_value(principal("a")).expect("principal");
        for wire in [
            json!({"bound":{"bound":"owned_by","values":[]},"unresolved":[]}),
            json!({"bound":{"bound":"owned_by"},"unresolved":[]}),
            json!({"bound":{"bound":"owned_by","values":null},"unresolved":[]}),
            json!({"bound":{"bound":"unrestricted","values":null},"unresolved":[]}),
            json!({"bound":{"bound":"owned_by","values":[owner],"pattern":"*"},"unresolved":[]}),
            json!({"bound":{"bound":"prefix","values":["records"]},"unresolved":[]}),
            json!({"bound":{"bound":"unrestricted"}}),
            json!({"bound":{"bound":"unrestricted"},"unresolved":[],"owners":[]}),
        ] {
            assert!(
                serde_json::from_value::<DomainRestriction>(wire.clone()).is_err(),
                "{wire}"
            );
        }
    }

    /// Unrestricted and exact bounds keep the earlier `ExactRestriction`
    /// bytes in both directions; the owner-scoped kind encodes as its own
    /// tag, which the earlier type refuses.
    #[test]
    fn domain_restriction_keeps_the_exact_restriction_wire() {
        let app = principal("app");
        let records = domain(&app, "records");
        let unknown = UnresolvedConstraint::Unknown;
        for (earlier, current) in [
            (
                ExactRestriction::<ResourceDomain>::unrestricted(),
                DomainRestriction::unrestricted(),
            ),
            (
                ExactRestriction::exact([records.clone()]),
                DomainRestriction::exact([records.clone()]),
            ),
            (ExactRestriction::exact([]), DomainRestriction::exact([])),
            (
                ExactRestriction::exact([records.clone()])
                    .conjoin(&ExactRestriction::unresolved(unknown)),
                DomainRestriction::exact([records])
                    .conjoin(&DomainRestriction::unresolved(unknown)),
            ),
        ] {
            let earlier_wire = serde_json::to_string(&earlier).expect("encode earlier");
            let current_wire = serde_json::to_string(&current).expect("encode current");
            assert_eq!(current_wire, earlier_wire, "byte-identical");
            assert_eq!(
                serde_json::from_str::<DomainRestriction>(&earlier_wire).expect("upgrade"),
                current
            );
            assert_eq!(
                serde_json::from_str::<ExactRestriction<ResourceDomain>>(&current_wire)
                    .expect("downgrade"),
                earlier
            );
        }
        let owned = DomainRestriction::owned_by([app.clone()]);
        let wire = serde_json::to_value(&owned).expect("encode owned");
        assert_eq!(
            wire,
            json!({
                "bound": {"bound": "owned_by", "values": [serde_json::to_value(&app).expect("principal")]},
                "unresolved": []
            })
        );
        assert_eq!(
            serde_json::from_value::<DomainRestriction>(wire.clone()).expect("decode owned"),
            owned
        );
        assert!(serde_json::from_value::<ExactRestriction<ResourceDomain>>(wire).is_err());
    }

    #[test]
    fn shared_json_owner_scoped_domain_vectors() {
        use crate::conformance_tests as fixture;
        fn outcome(result: &BoundMatch) -> &'static str {
            match result {
                BoundMatch::Matches => "matches",
                BoundMatch::OutsideBounds => "outside_bounds",
                BoundMatch::Unresolved(_) => "unresolved",
            }
        }
        let corpus = fixture::corpus();
        for case in fixture::entries(&corpus["domain_cases"]) {
            let mut actual = DomainRestriction::unrestricted();
            for input in fixture::entries(&case["inputs"]) {
                actual = actual.conjoin(&fixture::domain_restriction(input, &corpus));
            }
            assert_eq!(
                actual,
                fixture::domain_restriction(&case["expected"], &corpus),
                "{}",
                case["id"]
            );
            for probe in fixture::entries(&case["probes"]) {
                let value: ResourceDomain = fixture::decode(
                    &corpus["atoms"]["resource_domains"][fixture::name(&probe["value"])],
                );
                assert_eq!(
                    outcome(&actual.check(&value)),
                    fixture::name(&probe["expected"]),
                    "{}",
                    case["id"]
                );
            }
        }
        for case in fixture::entries(&corpus["domain_decode_rejections"]) {
            assert!(
                serde_json::from_value::<DomainRestriction>(case["wire"].clone()).is_err(),
                "{}",
                case["id"]
            );
        }
        assert_eq!(fixture::entries(&corpus["domain_cases"]).len(), 7);
        assert_eq!(
            fixture::entries(&corpus["domain_decode_rejections"]).len(),
            5
        );
    }
}
