//! Gateway evolution boundary; public facade remains in the crate root.
use super::*;

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessGatewayControl + HarnessEvolutionGatewayControl + Send + Sync,
{
    pub fn evolution_snapshot(
        &self,
        scope: p::Scope,
        auth: AuthContext,
    ) -> p::Result<p::EvolutionSnapshot> {
        self.validate_owner_control(auth)?;
        HarnessEvolutionGatewayControl::evolution_snapshot(self.harness.as_ref(), scope)
    }

    pub fn record_strategy_candidate(
        &self,
        run: p::RunId,
        candidate: p::StrategyCandidate,
        auth: AuthContext,
    ) -> p::Result<p::EventId> {
        self.validate_owner_control(auth)?;
        if !matches!(candidate.provenance.actor, p::Actor::Owner)
            || candidate.provenance.trust_tier != p::TrustTier::OwnerInput
        {
            return Err(p::Error(
                "gateway strategy candidate must carry owner provenance".into(),
            ));
        }
        HarnessEvolutionGatewayControl::record_strategy_candidate(
            self.harness.as_ref(),
            run,
            candidate,
        )
    }

    pub fn evaluate_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        comparison: p::EvolutionComparison,
        auth: AuthContext,
    ) -> p::Result<EvolutionEvaluationResult> {
        self.validate_owner_control(auth)?;
        HarnessEvolutionGatewayControl::evaluate_strategy(
            self.harness.as_ref(),
            run,
            candidate,
            comparison,
        )
    }

    pub fn promote_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        auth: AuthContext,
    ) -> p::Result<p::EventId> {
        self.validate_owner_control(auth)?;
        HarnessEvolutionGatewayControl::promote_strategy(
            self.harness.as_ref(),
            run,
            candidate,
            evaluation,
        )
    }

    pub fn activate_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        promotion: p::EventId,
        auth: AuthContext,
    ) -> p::Result<EvolutionActivationResult> {
        self.validate_owner_control(auth)?;
        let owner_confirmation = p::OwnerControlRef(format!(
            "owner-control:{}:{}",
            self.owner.0,
            self.sequence.fetch_add(1, Ordering::SeqCst)
        ));
        HarnessEvolutionGatewayControl::activate_strategy(
            self.harness.as_ref(),
            run,
            aggregate,
            candidate,
            evaluation,
            promotion,
            Some(owner_confirmation),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rollback_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        domain: p::StrategyDomain,
        scope: p::Scope,
        restored: p::StrategyVersionRef,
        triggers: Vec<p::EvidenceRef>,
        in_flight: p::InFlightDisposition,
        auth: AuthContext,
    ) -> p::Result<EvolutionActivationResult> {
        self.validate_owner_control(auth)?;
        let owner_confirmation = p::OwnerControlRef(format!(
            "owner-control:{}:{}",
            self.owner.0,
            self.sequence.fetch_add(1, Ordering::SeqCst)
        ));
        HarnessEvolutionGatewayControl::rollback_strategy(
            self.harness.as_ref(),
            run,
            aggregate,
            domain,
            scope,
            restored,
            triggers,
            in_flight,
            Some(owner_confirmation),
        )
    }

    pub fn set_auto_activation_paused(&self, paused: bool, auth: AuthContext) -> p::Result<()> {
        self.validate_owner_control(auth)?;
        HarnessEvolutionGatewayControl::set_auto_activation_paused(self.harness.as_ref(), paused);
        Ok(())
    }

    pub fn auto_activation_paused(&self, auth: AuthContext) -> p::Result<bool> {
        self.validate_owner_control(auth)?;
        Ok(HarnessEvolutionGatewayControl::auto_activation_paused(
            self.harness.as_ref(),
        ))
    }
}

impl<H> EvolutionRunGateway for LocalGateway<H>
where
    H: HarnessIngress + HarnessGatewayControl + HarnessEvolutionGatewayControl + Send + Sync,
{
    fn evolution_snapshot(&self, scope: p::Scope) -> p::Result<p::EvolutionSnapshot> {
        LocalGateway::evolution_snapshot(
            self,
            scope,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn record_strategy_candidate(
        &self,
        run: p::RunId,
        candidate: p::StrategyCandidate,
    ) -> p::Result<p::EventId> {
        LocalGateway::record_strategy_candidate(
            self,
            run,
            candidate,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn evaluate_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        comparison: p::EvolutionComparison,
    ) -> p::Result<EvolutionEvaluationResult> {
        LocalGateway::evaluate_strategy(
            self,
            run,
            candidate,
            comparison,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn promote_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
    ) -> p::Result<p::EventId> {
        LocalGateway::promote_strategy(
            self,
            run,
            candidate,
            evaluation,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn activate_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        promotion: p::EventId,
    ) -> p::Result<EvolutionActivationResult> {
        LocalGateway::activate_strategy(
            self,
            run,
            aggregate,
            candidate,
            evaluation,
            promotion,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn rollback_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        domain: p::StrategyDomain,
        scope: p::Scope,
        restored: p::StrategyVersionRef,
        triggers: Vec<p::EvidenceRef>,
        in_flight: p::InFlightDisposition,
    ) -> p::Result<EvolutionActivationResult> {
        LocalGateway::rollback_strategy(
            self,
            run,
            aggregate,
            domain,
            scope,
            restored,
            triggers,
            in_flight,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn set_auto_activation_paused(&self, paused: bool) -> p::Result<()> {
        LocalGateway::set_auto_activation_paused(
            self,
            paused,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn auto_activation_paused(&self) -> p::Result<bool> {
        LocalGateway::auto_activation_paused(
            self,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }
}
