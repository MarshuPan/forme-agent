//! Deterministic owner-brain project review over a fixed evidence snapshot.
use crate::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReview {
    pub schema_version: p::SchemaVersion,
    pub project: String,
    pub input_digest: p::SchemaDigest,
    pub signature: p::TaskSignature,
    pub reasons: Vec<String>,
    pub evidence: Vec<p::EventId>,
}
pub struct WorkspaceSupervisor;
impl WorkspaceSupervisor {
    pub fn input_digest(
        project: &p::WorkspaceProject,
        charter: &p::WorkspaceCharterRecord,
        events: &[p::Event],
        now: p::Timestamp,
    ) -> p::Result<p::SchemaDigest> {
        let refs = Self::evidence(project, events)
            .iter()
            .map(|e| &e.event_id)
            .collect::<Vec<_>>();
        p::canonical_digest(&(
            project,
            &charter.digest,
            &refs,
            project.deadline.is_some_and(|d| now >= d),
        ))
    }
    fn evidence<'a>(project: &p::WorkspaceProject, events: &'a [p::Event]) -> Vec<&'a p::Event> {
        let mut evidence = events
            .iter()
            .filter(|e| e.provenance.trust_tier != p::TrustTier::Untrusted)
            .filter(|e| match &e.payload {
                p::EventPayload::ObservationRecorded(p) => p.scope == project.scope,
                p::EventPayload::FailureEvidenceRecorded(p) => p.scope == project.scope,
                _ => false,
            })
            .collect::<Vec<_>>();
        evidence.sort_by(|a, b| {
            a.run_id
                .cmp(&b.run_id)
                .then(a.stream_seq.cmp(&b.stream_seq))
        });
        if evidence.len() > 128 {
            evidence.drain(..evidence.len() - 128);
        }
        evidence
    }
    pub fn reviews(
        state: &p::OrchestrationLedger,
        charter: &p::WorkspaceCharterRecord,
        events: &[p::Event],
        now: p::Timestamp,
    ) -> p::Result<Vec<WorkspaceReview>> {
        let mut reviews = Vec::new();
        for project in state
            .projects
            .values()
            .filter(|p| p.workspace == charter.workspace)
        {
            let latest = state
                .routes
                .values()
                .find(|r| {
                    r.declaration.project == project.reference && (!r.closed || !r.checkpointed)
                })
                .or_else(|| {
                    state
                        .checkpoints
                        .get(&project.reference)
                        .and_then(|c| state.routes.get(&c.route))
                });
            if latest.is_some_and(|r| !r.closed || r.has_inflight() || !r.checkpointed) {
                continue;
            }
            let evidence = Self::evidence(project, events);
            let refs = evidence
                .iter()
                .map(|e| e.event_id.clone())
                .collect::<Vec<_>>();
            let deadline_due = project.deadline.is_some_and(|d| now >= d);
            let input_digest = Self::input_digest(project, charter, events, now)?;
            let checkpoint = state.checkpoints.get(&project.reference);
            let cadence = charter
                .review_cadence
                .map_or(project.cadence_ms, |c| c.0.min(project.cadence_ms));
            let cadence_due = checkpoint.is_some_and(|c| {
                now.saturating_sub(c.at) >= i64::try_from(cadence).unwrap_or(i64::MAX)
            });
            let changed = latest.is_none_or(|r| r.declaration.review_digest != input_digest);
            if !changed && !cadence_due {
                continue;
            }
            let since = checkpoint.map_or(i64::MIN, |c| c.at);
            let failures = evidence
                .iter()
                .filter(|e| {
                    e.kind == p::EventKind::FailureEvidenceRecorded && (e.ts_unix_ms >= since
                        || latest.is_some_and(|r| matches!(&e.payload,p::EventPayload::FailureEvidenceRecorded(f)
                            if f.related_refs.contains(&p::EvidenceRef(format!("route-episode:{}",r.declaration.route.0))))))
                })
                .collect::<Vec<_>>();
            let mut signature = project.signature.clone();
            // Independent review is an evidence-driven response to a failed
            // segment, not a permanent increase in the project's complexity.
            if !failures.is_empty() && signature.clarity != p::TaskClarity::Clarify {
                signature.partition = p::TaskPartition::Sequential;
                signature.dependency = p::TaskDependency::Interdependent;
                signature.verification = p::TaskVerification::IndependentReview;
                if signature.extent == p::TaskExtent::Single {
                    signature.extent = p::TaskExtent::MultiStage;
                }
            }
            let mut reasons = Vec::new();
            if latest.is_none() {
                reasons.push("first-segment".into());
            }
            if changed {
                reasons.push("project-charter-or-evidence-diff".into());
            }
            if cadence_due {
                reasons.push("cadence".into());
            }
            if deadline_due {
                reasons.push("deadline".into());
            }
            if !failures.is_empty() {
                reasons.push("independent-review-after-failure".into());
            }
            reviews.push(WorkspaceReview {
                schema_version: p::SchemaVersion(1),
                project: project.reference.clone(),
                input_digest,
                signature,
                reasons,
                evidence: refs,
            });
            if reviews.len() == 16 {
                break;
            }
        }
        Ok(reviews)
    }
}
