use std::collections::{BTreeMap, BTreeSet};

use forme_capabilities::{
    rank_prefiltered, InMemorySelectionPolicyRegistry, PrefilteredSelectionSet, SelectionCandidate,
    SelectionEvidenceSet, SelectionPolicyRegistry,
};
use forme_protocol as p;

fn policy(version: &str, fallback: p::SelectionFallback) -> p::SelectionStrategySpec {
    p::SelectionStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef(version.into()),
        scope: p::Scope("workspace:m3-b".into()),
        content_ref: p::ContentRef(format!("spec:{version}")),
        content_digest: p::SchemaDigest(format!("digest:{version}")),
        compatibility: p::StrategyRuntimeCompatibility {
            schema_version: p::SchemaVersion(1),
            minimum_runtime_schema: p::SchemaVersion(1),
            event_schema: p::SchemaVersion(1),
            model_profile: None,
            tool_schema: None,
            backend_schema: None,
        },
        target: p::SelectionTarget::Capability,
        weights: vec![
            p::SelectionFeatureWeight {
                schema_version: p::SchemaVersion(1),
                feature: p::SelectionFeature::VerifiedSuccess,
                weight_basis_points: 5_000,
            },
            p::SelectionFeatureWeight {
                schema_version: p::SchemaVersion(1),
                feature: p::SelectionFeature::FailurePenalty,
                weight_basis_points: 3_000,
            },
            p::SelectionFeatureWeight {
                schema_version: p::SchemaVersion(1),
                feature: p::SelectionFeature::Compatibility,
                weight_basis_points: 2_000,
            },
        ],
        tie_breaker: p::SelectionTieBreaker::StableIdentity,
        fallback,
        max_results: 8,
    }
}

fn candidate(id: &str) -> SelectionCandidate {
    SelectionCandidate {
        schema_version: p::SchemaVersion(1),
        reference: p::ResourceRef(id.into()),
        evidence_key: p::CapabilityRef(id.into()),
        cost_microunits: 0,
        latency_ms: 0,
        compatible: true,
        lifecycle_active: true,
        managed_allowed: true,
        permission_allowed: true,
        scope_allowed: true,
    }
}

fn evidence(capability: &str, outcome: &str, reliability: &str) -> p::CapabilityEvidence {
    p::CapabilityEvidence {
        schema_version: p::SchemaVersion(1),
        capability: p::CapabilityRef(capability.into()),
        outcome: p::CapabilityOutcome(outcome.into()),
        reliability: p::Reliability(reliability.into()),
    }
}

#[test]
fn s60_filters_authority_and_managed_state_before_strategy_ranking() {
    let allowed = candidate("capability:allowed");
    let unauthorized = candidate("capability:unauthorized");
    let mut revoked = candidate("capability:revoked");
    revoked.lifecycle_active = false;
    let mut managed_denied = candidate("capability:managed-deny");
    managed_denied.managed_allowed = false;
    let authorized = BTreeSet::from([
        allowed.reference.clone(),
        revoked.reference.clone(),
        managed_denied.reference.clone(),
    ]);
    let candidates = PrefilteredSelectionSet::from_authorized(
        p::SelectionTarget::Capability,
        vec![allowed.clone(), unauthorized, revoked, managed_denied],
        &authorized,
    )
    .unwrap();
    assert_eq!(candidates.candidates().len(), 1);
    let evidence = SelectionEvidenceSet {
        schema_version: p::SchemaVersion(1),
        capability: vec![
            evidence("capability:allowed", "pass", "verified"),
            evidence("capability:unauthorized", "pass", "verified"),
            evidence("capability:revoked", "pass", "verified"),
        ],
        failures: BTreeMap::new(),
    };
    let ranked = rank_prefiltered(
        &policy("selection:m3-b:v1", p::SelectionFallback::NoSelection),
        &candidates,
        &evidence,
    )
    .unwrap();
    assert_eq!(ranked.ordered, vec![allowed.reference]);
}

#[test]
fn s60_provider_declaration_never_raises_the_result_evidence_ceiling() {
    let declared = candidate("capability:declared-only");
    let authorized = BTreeSet::from([declared.reference.clone()]);
    let candidates = PrefilteredSelectionSet::from_authorized(
        p::SelectionTarget::Capability,
        vec![declared],
        &authorized,
    )
    .unwrap();
    let evidence = SelectionEvidenceSet {
        schema_version: p::SchemaVersion(1),
        capability: vec![evidence("capability:declared-only", "pass", "declared")],
        failures: BTreeMap::new(),
    };
    let ranked = rank_prefiltered(
        &policy("selection:m3-b:v1", p::SelectionFallback::NoSelection),
        &candidates,
        &evidence,
    )
    .unwrap();
    assert!(ranked.ordered.is_empty());
}

#[test]
fn s60_selection_registry_keeps_version_content_immutable() {
    let v1 = policy("selection:m3-b:v1", p::SelectionFallback::SeedOrder);
    let registry = InMemorySelectionPolicyRegistry::with_seed(v1.clone()).unwrap();
    assert_eq!(registry.resolve(&v1.version).unwrap(), v1);
    let mut rebound = v1.clone();
    rebound.content_digest = p::SchemaDigest("digest:rebound".into());
    assert!(registry.register(rebound).is_err());
}
