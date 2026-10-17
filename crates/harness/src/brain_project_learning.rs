//! Route outcomes return to the existing owner memory and M3 candidate path.
use crate::{internal_provenance, production::ProductionRuntime};
use forme_protocol as p;
use forme_store::EventStore;

impl ProductionRuntime {
    pub(crate) fn sediment_project_route(&self, record: &p::WorkRouteRecord) -> p::Result<()> {
        if !record.closed {
            return Err(p::Error(
                "unfinished routes cannot produce an outcome episode".into(),
            ));
        }
        let route = &record.declaration;
        let store = self.services().store.clone();
        let stream = self.services().aggregates.memory_cognition.clone();
        let provenance = internal_provenance(p::Source::Internal);
        let episode = p::NodeId(format!("route-episode:{}", route.route.0));
        let mut payloads = Vec::new();
        let scope = p::Scope(self.config().workspace.0.clone());
        let node = |id: p::NodeId, kind: &str, content: String| {
            p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
                node_id: id,
                kind: p::MemoryNodeType(kind.into()),
                content_ref: p::ContentRef(content),
                tier: p::StabilityTier::Working,
                confidence: p::Confidence(0.4),
                scope: scope.clone(),
                resting_activation: p::RestingActivation(0.1),
                recency: p::Recency(route.created_at),
            })
        };
        payloads.push(node(
            episode.clone(),
            "episodic",
            format!(
                "route outcome {:?}; input {}; spent {}; unresolved conflicts {}",
                record.status,
                route.input_digest.0,
                record.spent,
                record.conflicts.len()
            ),
        ));
        let mut anchors = vec![
            ("goal".to_owned(), route.goal.0.clone()),
            (
                "pattern".into(),
                route
                    .pattern
                    .as_ref()
                    .map_or("custom", |p| p.0.as_str())
                    .into(),
            ),
            (
                "verification".into(),
                format!(
                    "{:?}",
                    record
                        .nodes
                        .values()
                        .map(|n| &n.verification)
                        .collect::<Vec<_>>()
                ),
            ),
            ("failure".into(), format!("{:?}", record.status)),
        ];
        for (id, n) in &record.nodes {
            anchors.push((
                format!("node:{id}"),
                format!("{}:{:?}:{}", id, n.status, n.attempts),
            ));
            anchors.push((
                format!("result:{id}"),
                n.result
                    .as_ref()
                    .map_or("no-result", |r| r.0.as_str())
                    .into(),
            ));
            for (i, evidence) in n.evidence.iter().enumerate() {
                anchors.push((format!("evidence:{id}:{i}"), evidence.0.clone()));
            }
        }
        for (kind, content) in anchors {
            let target = p::NodeId(format!(
                "route-lineage:{}:{}",
                route.route.0,
                p::canonical_digest(&kind)?.0
            ));
            payloads.push(node(target.clone(), "event", content));
            payloads.push(p::EventPayload::MemoryEdgeAppended(
                p::MemoryEdgeAppendedPayload {
                    edge_id: p::EdgeId(format!(
                        "route-lineage:{}:{}",
                        route.route.0,
                        p::canonical_digest(&kind)?.0
                    )),
                    from: episode.clone(),
                    to: target,
                    kind: p::MemoryEdgeType(
                        if kind.starts_with("result") || kind == "failure" {
                            "causal"
                        } else {
                            "part-of"
                        }
                        .into(),
                    ),
                    weight: p::Weight(0.7),
                },
            ));
        }
        let candidate_id = p::CandidateId(format!("route-strategy:{}", route.route.0));
        // A later checkpoint is administrative progress, not a new outcome.
        let spec_digest = p::canonical_digest(&(
            &record.declaration,
            &record.nodes,
            record.remaining,
            record.spent,
            record.status,
            &record.conflicts,
        ))?;
        let candidate = p::StrategyCandidate {
            schema_version: p::SchemaVersion(1),
            candidate: candidate_id.clone(),
            domain: p::StrategyDomain::Coordination,
            scope: route.main.scope.clone(),
            target_tier: p::StabilityTier::Stable,
            proposed_version: p::StrategyVersionRef(format!("route-proposal:{}", spec_digest.0)),
            baseline: p::StrategyVersionRef("coordination:seed-v1".into()),
            spec_ref: p::ContentRef(format!("orchestration-evidence:{}", route.route.0)),
            spec_digest,
            evidence: vec![
                p::EvidenceRef(episode.0.clone()),
                p::EvidenceRef(format!(
                    "route-outcome:{}:{:?}",
                    route.route.0, record.status
                )),
            ],
            provenance: provenance.clone(),
            impact: if record.status == p::WorkRouteStatus::Completed {
                p::EvolutionImpact::Bounded
            } else {
                p::EvolutionImpact::Cautious
            },
            rollback_policy: p::StrategyRollbackPolicy {
                schema_version: p::SchemaVersion(1),
                known_good: p::StrategyVersionRef("coordination:seed-v1".into()),
                rollback_on_hard_regression: true,
                owner_on_unverifiable: true,
            },
        };
        candidate.validate()?;
        payloads.push(p::EventPayload::CandidateCreated(
            p::CandidateCreatedPayload {
                candidate_id,
                target: p::CandidateTargetRef(format!(
                    "strategy:coordination:{}",
                    candidate.proposed_version.0
                )),
                evidence_refs: candidate.evidence.clone(),
                confidence: p::Confidence(0.4),
                provenance: provenance.clone(),
                target_tier: p::StabilityTier::Stable,
                capability_update: None,
                strategy_candidate: Some(candidate),
            },
        ));
        if record.status != p::WorkRouteStatus::Completed {
            let failure = p::FailureEvidenceRef(format!("route-failure:{}", route.route.0));
            payloads.push(p::EventPayload::FailureEvidenceRecorded(
                p::FailureEvidenceRecordedPayload {
                    failure_ref: failure.clone(),
                    class: p::FailureClass::ResourceSelectionFailure,
                    impact: p::Impact::High,
                    scope: route.main.scope.clone(),
                    related_refs: vec![p::EvidenceRef(episode.0.clone())],
                    suggested_fix: None,
                },
            ));
            payloads.push(p::EventPayload::FailureDigestUpdated(
                p::FailureDigestUpdatedPayload {
                    digest_ref: p::FailureDigestRef(format!(
                        "route-failure-digest:{}",
                        route.route.0
                    )),
                    members: vec![failure],
                    summary: p::DigestSummaryRef(format!("{:?}", record.status)),
                },
            ));
        }
        // Deterministic IDs, timestamps, and payloads let a crash retry fill
        // only the missing tail without duplicating episodes or candidates.
        for (i, payload) in payloads.into_iter().enumerate() {
            store.append(p::Event::new(
                p::EventId(format!("route-learning:{}:{i}", route.route.0)),
                stream.clone(),
                None,
                payload,
                p::SchemaVersion(1),
                route.created_at,
                provenance.clone(),
            ))?;
        }
        self.services().memory.refresh_projection()?;
        Ok(())
    }
}
