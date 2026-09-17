use std::collections::{BTreeMap, BTreeSet};

use forme_coordination::{AuthorizedFederatedPlacementSelector, FederatedPlacementRequest};
use forme_protocol as p;

fn grant(peer: &str, roles: Vec<p::FederatedPeerRole>, expires_at: i64) -> p::FederatedPeerGrant {
    p::FederatedPeerGrant {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: p::FederatedPeerRef(peer.into()),
        owner: p::VerifiedPrincipal("owner:forme".into()),
        roles,
        scopes: vec![p::Scope("workspace:s80".into())],
        capabilities: vec![p::CapabilityRef("fixture.mutate".into())],
        transport_identity: p::TransportIdentityDigest(format!("sha256:{peer}")),
        authority_epoch: p::AuthorityEpoch(4),
        grant_version: p::PeerGrantVersion(1),
        expires_at,
        created_by: p::OwnerControlRef(format!("owner-control:{peer}")),
    }
}

fn snapshot(grants: Vec<p::FederatedPeerGrant>) -> p::FederationSnapshot {
    let mut grant_refs = grants
        .iter()
        .map(|grant| grant.reference().unwrap())
        .collect::<Vec<_>>();
    grant_refs.sort();
    let mut snapshot = p::FederationSnapshot {
        schema_version: p::M4_SCHEMA_VERSION,
        authority: p::AuthorityRef("authority:forme".into()),
        authority_epoch: p::AuthorityEpoch(4),
        registry_version: p::FederationAggregateVersion {
            schema_version: p::M4_SCHEMA_VERSION,
            aggregate: p::FederationAggregateRef("federation".into()),
            version: 4,
        },
        grants: grant_refs,
        digest: p::SchemaDigest(String::new()),
    };
    snapshot.digest = p::canonical_digest(&(
        &snapshot.authority,
        snapshot.authority_epoch,
        &snapshot.registry_version,
        &snapshot.grants,
    ))
    .unwrap();
    snapshot
}

fn grant_map(
    grants: &[p::FederatedPeerGrant],
) -> BTreeMap<p::FederatedPeerGrantRef, p::FederatedPeerGrant> {
    grants
        .iter()
        .cloned()
        .map(|grant| (grant.reference().unwrap(), grant))
        .collect()
}

fn candidate(peer: &str, score: u16) -> p::FederatedExecutorCandidate {
    let grant = grant(peer, vec![p::FederatedPeerRole::Executor], 10_000);
    let grant_ref = grant.reference().unwrap();
    p::FederatedExecutorCandidate {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: grant.peer,
        grant: grant_ref,
        profile: p::ExecutorProfileRef("profile:s80".into()),
        scope: p::Scope("workspace:s80".into()),
        capability: p::CapabilityRef("fixture.mutate".into()),
        expires_at: 10_000,
        health: p::ExecutorHealthState::Healthy,
        health_observed_at: 1_900,
        capability_evidence: vec![p::CapabilityEvidenceRef(format!("evidence:{peer}"))],
        failure_evidence: Vec::new(),
        managed_policy_allowed: true,
        score_basis_points: score,
    }
}

#[test]
fn s80_filters_authority_before_ranking_and_score_never_authorizes() {
    let allowed = candidate("peer:allowed", 100);
    let mut denied = candidate("peer:denied", 10_000);
    denied.managed_policy_allowed = false;
    let mut stale = candidate("peer:stale", 9_000);
    stale.health_observed_at = 1;
    let mut failed = candidate("peer:failed", 8_000);
    failed.failure_evidence = vec![p::FailureEvidenceRef("failure:recent".into())];
    let grants = vec![
        grant("peer:allowed", vec![p::FederatedPeerRole::Executor], 10_000),
        grant("peer:denied", vec![p::FederatedPeerRole::Executor], 10_000),
        grant("peer:stale", vec![p::FederatedPeerRole::Executor], 10_000),
        grant("peer:failed", vec![p::FederatedPeerRole::Executor], 10_000),
    ];
    let grant_projection = grant_map(&grants);
    let placements = BTreeMap::from([
        (
            allowed.peer.clone(),
            p::RemotePlacementPlanRef("placement:allowed".into()),
        ),
        (
            denied.peer.clone(),
            p::RemotePlacementPlanRef("placement:denied".into()),
        ),
        (
            stale.peer.clone(),
            p::RemotePlacementPlanRef("placement:stale".into()),
        ),
        (
            failed.peer.clone(),
            p::RemotePlacementPlanRef("placement:failed".into()),
        ),
    ]);

    let decision = AuthorizedFederatedPlacementSelector
        .select(FederatedPlacementRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            decision: p::PlacementDecisionRef("decision:s80".into()),
            snapshot: snapshot(grants),
            scope: p::Scope("workspace:s80".into()),
            capability: p::CapabilityRef("fixture.mutate".into()),
            evaluated_at: 2_000,
            maximum_health_age: p::DurationMs(500),
            compatible_profiles: BTreeSet::from([p::ExecutorProfileRef("profile:s80".into())]),
            grants: grant_projection,
            candidates: vec![denied, stale, failed, allowed],
            placements,
        })
        .unwrap();

    assert_eq!(
        decision.chosen,
        Some(p::FederatedPeerRef("peer:allowed".into()))
    );
    assert_eq!(
        decision.placement,
        Some(p::RemotePlacementPlanRef("placement:allowed".into()))
    );
    let denied = decision
        .candidates
        .iter()
        .find(|trace| trace.candidate.peer.0 == "peer:denied")
        .unwrap();
    assert!(!denied.eligible);
    assert!(denied
        .reasons
        .contains(&p::PlacementFilterReason::ManagedPolicyDenied));
    let stale = decision
        .candidates
        .iter()
        .find(|trace| trace.candidate.peer.0 == "peer:stale")
        .unwrap();
    assert!(stale
        .reasons
        .contains(&p::PlacementFilterReason::HealthStale));
    let failed = decision
        .candidates
        .iter()
        .find(|trace| trace.candidate.peer.0 == "peer:failed")
        .unwrap();
    assert!(failed
        .reasons
        .contains(&p::PlacementFilterReason::EvidenceInsufficient));
}

#[test]
fn s80_no_candidate_returns_a_governed_no_placement_decision() {
    let mut denied = candidate("peer:denied", 10_000);
    denied.managed_policy_allowed = false;
    let grants = vec![grant(
        "peer:denied",
        vec![p::FederatedPeerRole::Executor],
        10_000,
    )];
    let decision = AuthorizedFederatedPlacementSelector
        .select(FederatedPlacementRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            decision: p::PlacementDecisionRef("decision:s80:none".into()),
            snapshot: snapshot(grants.clone()),
            scope: p::Scope("workspace:s80".into()),
            capability: p::CapabilityRef("fixture.mutate".into()),
            evaluated_at: 2_000,
            maximum_health_age: p::DurationMs(500),
            compatible_profiles: BTreeSet::from([p::ExecutorProfileRef("profile:s80".into())]),
            grants: grant_map(&grants),
            candidates: vec![denied],
            placements: BTreeMap::new(),
        })
        .unwrap();
    assert_eq!(decision.chosen, None);
    assert_eq!(decision.placement, None);
}
