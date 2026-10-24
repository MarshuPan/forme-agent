//! Pure intake plan for owner feedback. The Harness commits it with the
//! original cognition's invalidation; this module cannot promote or execute.
use super::*;

pub fn owner_correction_candidate(
    id: p::CandidateId,
    scope: p::Scope,
    target: p::ObjectRef,
    statement: String,
    evidence: p::EvidenceRef,
    provenance: p::Provenance,
) -> p::Result<Vec<p::EventPayload>> {
    if provenance.actor != p::Actor::Owner
        || provenance.source != p::Source::OwnerControl
        || provenance.trust_tier != p::TrustTier::OwnerInput
        || target.0.trim().is_empty()
        || statement.trim().is_empty()
        || statement.len() > 8_192
    {
        return Err(p::Error("owner correction evidence is invalid".into()));
    }
    let proposal = CognitiveMapUpdateProposal {
        schema_version: p::SchemaVersion(1),
        candidate_id: Some(id.clone()),
        scope: MapScope {
            schema_version: p::SchemaVersion(1),
            scope,
        },
        kind: MapUpdateKind::Frame,
        confidence: p::Confidence(0.4),
        evidence: vec![evidence.clone()],
        frame: Some(p::JudgmentFrameRef(format!(
            "owner-feedback-frame:{}",
            id.0
        ))),
        quality: None,
        blindspot: None,
        resource: None,
        reflection_inputs: vec![evidence],
        statement: format!("Owner correction for {}: {}", target.0, statement),
        provenance: provenance.clone(),
    };
    validate_proposal(&proposal)?;
    let mut spec = map_candidate_spec(&proposal, &id);
    spec.target_tier = p::StabilityTier::Working;
    Ok(vec![
        p::EventPayload::ReflectionProduced(p::ReflectionProducedPayload {
            inputs: proposal.reflection_inputs,
            candidate_refs: vec![id.clone()],
        }),
        p::EventPayload::CognitiveMapUpdateProposed(p::CognitiveMapUpdateProposedPayload {
            candidate_id: id.clone(),
            kind: MapUpdateKind::Frame.as_ref(),
            evidence: proposal.evidence,
            frame: proposal.frame,
            quality: None,
            blindspot: None,
            resource: None,
            confidence: p::Confidence(0.4),
        }),
        p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
            candidate_id: id,
            target: spec.target,
            evidence_refs: spec.evidence_refs,
            confidence: spec.confidence,
            provenance,
            target_tier: spec.target_tier,
            capability_update: None,
            strategy_candidate: None,
        }),
    ])
}
