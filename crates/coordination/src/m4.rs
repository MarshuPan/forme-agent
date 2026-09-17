use std::collections::{BTreeMap, BTreeSet};

use forme_protocol as p;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedPlacementRequest {
    pub schema_version: p::SchemaVersion,
    pub decision: p::PlacementDecisionRef,
    pub snapshot: p::FederationSnapshot,
    pub scope: p::Scope,
    pub capability: p::CapabilityRef,
    pub evaluated_at: p::Timestamp,
    pub maximum_health_age: p::DurationMs,
    pub compatible_profiles: BTreeSet<p::ExecutorProfileRef>,
    pub grants: BTreeMap<p::FederatedPeerGrantRef, p::FederatedPeerGrant>,
    pub candidates: Vec<p::FederatedExecutorCandidate>,
    pub placements: BTreeMap<p::FederatedPeerRef, p::RemotePlacementPlanRef>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AuthorizedFederatedPlacementSelector;

impl AuthorizedFederatedPlacementSelector {
    pub fn select(
        &self,
        mut request: FederatedPlacementRequest,
    ) -> p::Result<p::FederatedPlacementDecision> {
        request.snapshot.validate()?;
        if request.schema_version.0 == 0
            || request.decision.0.trim().is_empty()
            || request.scope.0.trim().is_empty()
            || request.capability.0.trim().is_empty()
            || request.evaluated_at <= 0
            || request.maximum_health_age.0 == 0
            || request.candidates.is_empty()
        {
            return Err(p::Error("federated placement request is incomplete".into()));
        }

        request
            .candidates
            .sort_by(|left, right| left.peer.cmp(&right.peer));
        if request
            .candidates
            .windows(2)
            .any(|pair| pair[0].peer == pair[1].peer)
        {
            return Err(p::Error(
                "federated placement candidates must be unique".into(),
            ));
        }

        let candidates = std::mem::take(&mut request.candidates);
        let traces = candidates
            .into_iter()
            .map(|candidate| {
                let reasons = filter_reasons(&request, &candidate)?;
                Ok(p::PlacementCandidateTrace {
                    schema_version: p::M4_SCHEMA_VERSION,
                    candidate,
                    eligible: reasons.is_empty(),
                    reasons,
                })
            })
            .collect::<p::Result<Vec<_>>>()?;

        let chosen = traces
            .iter()
            .filter(|trace| trace.eligible)
            .max_by(|left, right| {
                left.candidate
                    .score_basis_points
                    .cmp(&right.candidate.score_basis_points)
                    .then_with(|| right.candidate.peer.cmp(&left.candidate.peer))
            })
            .map(|trace| trace.candidate.peer.clone());
        let placement = chosen
            .as_ref()
            .map(|peer| {
                request.placements.get(peer).cloned().ok_or_else(|| {
                    p::Error("eligible executor has no immutable placement plan".into())
                })
            })
            .transpose()?;

        let mut decision = p::FederatedPlacementDecision {
            schema_version: p::M4_SCHEMA_VERSION,
            decision: request.decision,
            federation_snapshot: p::FederationSnapshotRef(request.snapshot.digest.0),
            scope: request.scope,
            capability: request.capability,
            evaluated_at: request.evaluated_at,
            candidates: traces,
            chosen,
            placement,
            digest: p::SchemaDigest(String::new()),
        };
        decision.refresh_digest()?;
        decision.validate()?;
        Ok(decision)
    }
}

fn filter_reasons(
    request: &FederatedPlacementRequest,
    candidate: &p::FederatedExecutorCandidate,
) -> p::Result<Vec<p::PlacementFilterReason>> {
    candidate.validate()?;
    let mut reasons = Vec::new();
    let grant = request.grants.get(&candidate.grant).filter(|grant| {
        grant.peer == candidate.peer
            && request
                .snapshot
                .grants
                .binary_search(&candidate.grant)
                .is_ok()
            && grant.reference().as_ref() == Ok(&candidate.grant)
    });

    if grant.is_none_or(|grant| {
        grant.expires_at <= request.evaluated_at || candidate.expires_at <= request.evaluated_at
    }) {
        reasons.push(p::PlacementFilterReason::GrantInactive);
    }
    if grant.is_none_or(|grant| !grant.roles.contains(&p::FederatedPeerRole::Executor)) {
        reasons.push(p::PlacementFilterReason::RoleDenied);
    }
    if candidate.scope != request.scope
        || grant.is_none_or(|grant| !grant.scopes.contains(&request.scope))
    {
        reasons.push(p::PlacementFilterReason::ScopeDenied);
    }
    if candidate.capability != request.capability
        || grant.is_none_or(|grant| !grant.capabilities.contains(&request.capability))
    {
        reasons.push(p::PlacementFilterReason::CapabilityDenied);
    }
    if !candidate.managed_policy_allowed {
        reasons.push(p::PlacementFilterReason::ManagedPolicyDenied);
    }
    if !request.compatible_profiles.contains(&candidate.profile) {
        reasons.push(p::PlacementFilterReason::SchemaIncompatible);
    }
    let health_age = request
        .evaluated_at
        .checked_sub(candidate.health_observed_at)
        .and_then(|age| u64::try_from(age).ok());
    if candidate.health != p::ExecutorHealthState::Healthy
        || health_age.is_none_or(|age| age > request.maximum_health_age.0)
    {
        reasons.push(p::PlacementFilterReason::HealthStale);
    }
    if candidate.capability_evidence.is_empty() || !candidate.failure_evidence.is_empty() {
        reasons.push(p::PlacementFilterReason::EvidenceInsufficient);
    }
    Ok(reasons)
}
