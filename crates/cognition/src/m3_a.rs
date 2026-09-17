use forme_protocol as p;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvolutionDecision {
    Reject,
    Promote,
    NeedOwner,
    Downgrade,
    Rollback,
}

pub trait StrategyEvolutionGovernor {
    fn decide(
        &self,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        current: &p::EvolutionSnapshot,
    ) -> EvolutionDecision;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConservativeStrategyEvolutionGovernor;

impl StrategyEvolutionGovernor for ConservativeStrategyEvolutionGovernor {
    fn decide(
        &self,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        current: &p::EvolutionSnapshot,
    ) -> EvolutionDecision {
        if candidate.validate().is_err()
            || evaluation.validate().is_err()
            || current.validate().is_err()
            || candidate.impact == p::EvolutionImpact::Constitutional
            || candidate.evidence.is_empty()
            || !trusted_evolution_provenance(&candidate.provenance)
            || evaluation.candidate != candidate.proposed_version
            || evaluation.baseline != candidate.baseline
        {
            return EvolutionDecision::Reject;
        }

        let Some(active) = current.strategies.iter().find(|strategy| {
            strategy.domain == candidate.domain && strategy.scope == candidate.scope
        }) else {
            return EvolutionDecision::Reject;
        };

        if active.version == candidate.proposed_version {
            return match evaluation.verdict {
                p::EvaluationVerdict::Fail => EvolutionDecision::Rollback,
                p::EvaluationVerdict::Unverifiable => EvolutionDecision::Downgrade,
                p::EvaluationVerdict::Pass => EvolutionDecision::Reject,
            };
        }
        if active.version != candidate.baseline
            || candidate.rollback_policy.known_good != active.version
        {
            return EvolutionDecision::Reject;
        }

        match evaluation.verdict {
            p::EvaluationVerdict::Fail => EvolutionDecision::Reject,
            p::EvaluationVerdict::Unverifiable => {
                if candidate.rollback_policy.owner_on_unverifiable {
                    EvolutionDecision::NeedOwner
                } else {
                    EvolutionDecision::Reject
                }
            }
            p::EvaluationVerdict::Pass => match candidate.impact {
                p::EvolutionImpact::Cautious => EvolutionDecision::Promote,
                p::EvolutionImpact::Bounded | p::EvolutionImpact::Expansive => {
                    EvolutionDecision::NeedOwner
                }
                p::EvolutionImpact::Constitutional => EvolutionDecision::Reject,
            },
        }
    }
}

fn trusted_evolution_provenance(provenance: &p::Provenance) -> bool {
    matches!(
        (&provenance.actor, provenance.trust_tier),
        (p::Actor::Owner, p::TrustTier::OwnerInput)
            | (p::Actor::System, p::TrustTier::VerifiedProcess)
    )
}
