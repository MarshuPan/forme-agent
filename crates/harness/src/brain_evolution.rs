//! Production strategy bodies are immutable protected artifacts, resolved from owner events.
use crate::{production::ProductionRuntime, M3DomainRuntime, M3RuntimeCompatibility};
use forme_capabilities::{SelectionEvidenceSet, SelectionPolicyRegistry};
use forme_coordination::CoordinationRegistry;
use forme_loop::LoopRegistry;
use forme_models::ModelAdaptationRegistry;
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductionStrategySpec {
    Seed(crate::brain_evolution_seed::ProductionSeedSpec),
    Loop(p::LoopStrategySpec),
    Coordination(p::CoordinationStrategySpec),
    Selection(p::SelectionStrategySpec),
    ModelAdaptation(p::ModelAdaptationSpec),
}

impl ProductionStrategySpec {
    pub fn domain(&self) -> p::StrategyDomain {
        match self {
            Self::Seed(s) => s.domain,
            Self::Loop(_) => p::StrategyDomain::Loop,
            Self::Coordination(_) => p::StrategyDomain::Coordination,
            Self::Selection(s) => match s.target {
                p::SelectionTarget::Capability => p::StrategyDomain::CapabilitySelection,
                p::SelectionTarget::Model => p::StrategyDomain::ModelSelection,
                p::SelectionTarget::Backend => p::StrategyDomain::BackendSelection,
            },
            Self::ModelAdaptation(_) => p::StrategyDomain::ModelAdaptation,
        }
    }
    pub fn validate(&self) -> p::Result<()> {
        match self {
            Self::Seed(s) => s.validate(),
            Self::Loop(s) => s.validate(),
            Self::Coordination(s) => s.validate(),
            Self::Selection(s) => s.validate(),
            Self::ModelAdaptation(s) => s.validate(),
        }
    }
    pub fn version(&self) -> &p::StrategyVersionRef {
        match self {
            Self::Seed(s) => &s.version,
            Self::Loop(s) => &s.version,
            Self::Coordination(s) => &s.version,
            Self::Selection(s) => &s.version,
            Self::ModelAdaptation(s) => &s.version,
        }
    }
    pub fn scope(&self) -> &p::Scope {
        match self {
            Self::Seed(s) => &s.scope,
            Self::Loop(s) => &s.scope,
            Self::Coordination(s) => &s.scope,
            Self::Selection(s) => &s.scope,
            Self::ModelAdaptation(s) => &s.scope,
        }
    }
    pub fn content_ref(&self) -> &p::ContentRef {
        match self {
            Self::Seed(s) => &s.content_ref,
            Self::Loop(s) => &s.content_ref,
            Self::Coordination(s) => &s.content_ref,
            Self::Selection(s) => &s.content_ref,
            Self::ModelAdaptation(s) => &s.content_ref,
        }
    }
    pub fn digest(&self) -> &p::SchemaDigest {
        match self {
            Self::Seed(s) => &s.content_digest,
            Self::Loop(s) => &s.content_digest,
            Self::Coordination(s) => &s.content_digest,
            Self::Selection(s) => &s.content_digest,
            Self::ModelAdaptation(s) => &s.content_digest,
        }
    }
    fn set_identity(&mut self, reference: p::ContentRef, digest: p::SchemaDigest) {
        match self {
            Self::Seed(s) => {
                s.content_ref = reference;
                s.content_digest = digest;
            }
            Self::Loop(s) => {
                s.content_ref = reference;
                s.content_digest = digest;
            }
            Self::Coordination(s) => {
                s.content_ref = reference;
                s.content_digest = digest;
            }
            Self::Selection(s) => {
                s.content_ref = reference;
                s.content_digest = digest;
            }
            Self::ModelAdaptation(s) => {
                s.content_ref = reference;
                s.content_digest = digest;
            }
        }
    }
    fn object(&self) -> String {
        format!("production-strategy:{}", self.version().0)
    }
    fn canonical_digest(&self) -> p::Result<p::SchemaDigest> {
        let mut normalized = self.clone();
        normalized.set_identity(self.content_ref().clone(), p::SchemaDigest(String::new()));
        p::canonical_digest(&normalized)
    }
    pub(crate) fn seal_identity(mut self) -> p::Result<Self> {
        let reference = p::ContentRef(format!(
            "content:{}",
            p::sha256_content_digest(self.object().as_bytes()).0
        ));
        self.set_identity(reference, p::SchemaDigest(String::new()));
        let digest = self.canonical_digest()?;
        self.set_identity(self.content_ref().clone(), digest);
        self.validate()?;
        Ok(self)
    }
}

pub(crate) struct ProductionStrategyRegistry {
    pub store: SqliteEventStore,
    pub owner: p::VerifiedPrincipal,
    pub aggregate: p::RunId,
}
impl ProductionStrategyRegistry {
    fn read_content(&self, reference: &p::ContentRef) -> p::Result<ProductionStrategySpec> {
        let protection = self
            .store
            .content_protection()?
            .ok_or_else(|| p::Error("strategy content protection is unavailable".into()))?;
        let record = protection
            .lifecycle_service(self.store.clone())
            .protected_content(&self.owner, reference)?
            .ok_or_else(|| p::Error("strategy body is absent or erased".into()))?;
        let bytes = protection
            .content_service()
            .decrypt(&record.envelope.binding(), &record.envelope)?;
        let spec: ProductionStrategySpec = serde_json::from_slice(&bytes)
            .map_err(|_| p::Error("strategy body is not a typed specification".into()))?;
        spec.validate()?;
        if spec.content_ref() != reference
            || spec.digest() != &spec.canonical_digest()?
            || spec.scope() != &record.scope
        {
            return Err(p::Error(
                "strategy body digest, scope or reference was altered".into(),
            ));
        }
        Ok(spec)
    }
    pub(crate) fn resolve(
        &self,
        version: &p::StrategyVersionRef,
    ) -> p::Result<ProductionStrategySpec> {
        let id = spec_node(version);
        let records = self
            .store
            .read_run(self.aggregate.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let reference = records
            .iter()
            .find_map(|e| match &e.payload {
                p::EventPayload::MemoryNodeAppended(n)
                    if n.node_id == id
                        && n.kind.0 == "production-strategy"
                        && e.provenance.trust_tier == p::TrustTier::OwnerInput
                        && e.provenance.actor == p::Actor::Owner =>
                {
                    Some(&n.content_ref)
                }
                p::EventPayload::MemoryNodeAppended(n)
                    if n.node_id == id
                        && n.kind.0 == "production-strategy"
                        && e.provenance.actor == p::Actor::System
                        && e.provenance.trust_tier == p::TrustTier::VerifiedProcess =>
                {
                    Some(&n.content_ref)
                }
                _ => None,
            })
            .ok_or_else(|| p::Error("active strategy has no registered production body".into()))?;
        let spec = self.read_content(reference)?;
        if spec.version() != version {
            return Err(p::Error(
                "strategy version differs from its registered identity".into(),
            ));
        }
        Ok(spec)
    }
}
impl LoopRegistry for ProductionStrategyRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<p::LoopStrategySpec> {
        match self.resolve(version)? {
            ProductionStrategySpec::Loop(s) => Ok(s),
            _ => Err(p::Error("strategy domain is not loop".into())),
        }
    }
}
impl CoordinationRegistry for ProductionStrategyRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<p::CoordinationStrategySpec> {
        match self.resolve(version)? {
            ProductionStrategySpec::Coordination(s) => Ok(s),
            _ => Err(p::Error("strategy domain is not coordination".into())),
        }
    }
}
impl SelectionPolicyRegistry for ProductionStrategyRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<p::SelectionStrategySpec> {
        match self.resolve(version)? {
            ProductionStrategySpec::Selection(s) => Ok(s),
            _ => Err(p::Error("strategy domain is not selection".into())),
        }
    }
}
impl ModelAdaptationRegistry for ProductionStrategyRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<p::ModelAdaptationSpec> {
        match self.resolve(version)? {
            ProductionStrategySpec::ModelAdaptation(s) => Ok(s),
            _ => Err(p::Error("strategy domain is not model adaptation".into())),
        }
    }
}

pub(crate) fn spec_node(version: &p::StrategyVersionRef) -> p::NodeId {
    p::NodeId(format!(
        "production-strategy:{}",
        p::sha256_content_digest(version.0.as_bytes()).0
    ))
}

impl ProductionRuntime {
    /// Registering a body does not promote or activate it. M3 governance remains mandatory.
    pub fn register_strategy_spec(
        &self,
        owner: &p::VerifiedPrincipal,
        spec: ProductionStrategySpec,
    ) -> p::Result<ProductionStrategySpec> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        if let ProductionStrategySpec::Seed(seed) = &spec {
            if spec != crate::brain_evolution_seed::repository_seed(seed.domain, &seed.scope)? {
                return Err(p::Error(
                    "only this compiled runtime's repository seed may be registered".into(),
                ));
            }
        }
        self.register_strategy_body(spec, true)
    }
    pub(crate) fn register_strategy_body(
        &self,
        spec: ProductionStrategySpec,
        owner_request: bool,
    ) -> p::Result<ProductionStrategySpec> {
        let owner = &p::VerifiedPrincipal(self.owner().0.clone());
        let spec = spec.seal_identity()?;
        let registry = ProductionStrategyRegistry {
            store: (*self.services().store).clone(),
            owner: owner.clone(),
            aggregate: self.services().aggregates.memory_cognition.clone(),
        };
        let protection = registry
            .store
            .content_protection()?
            .ok_or_else(|| p::Error("strategy content protection is unavailable".into()))?;
        let service = protection.lifecycle_service(registry.store.clone());
        if service
            .protected_content(owner, spec.content_ref())?
            .is_some()
        {
            if registry.read_content(spec.content_ref())? != spec {
                return Err(p::Error(
                    "strategy version is immutable; use a new version".into(),
                ));
            }
        } else {
            let bytes = serde_json::to_vec(&spec)
                .map_err(|_| p::Error("strategy serialization failed".into()))?;
            let (_, record) =
                protection.seal_record(&bytes, spec.scope(), &spec.object(), spec.digest())?;
            service.put_protected_content(owner, record)?;
        }
        let node = spec_node(spec.version());
        let payload = p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
            node_id: node.clone(),
            kind: p::MemoryNodeType("production-strategy".into()),
            content_ref: spec.content_ref().clone(),
            tier: p::StabilityTier::Working,
            confidence: p::Confidence(1.0),
            scope: spec.scope().clone(),
            resting_activation: p::RestingActivation(0.0),
            recency: p::Recency(0),
        });
        registry.store.append(p::Event::new(
            p::EventId(node.0),
            registry.aggregate.clone(),
            None,
            payload,
            p::SchemaVersion(1),
            0,
            if owner_request {
                p::Provenance {
                    source: p::Source::OwnerControl,
                    actor: p::Actor::Owner,
                    trust_tier: p::TrustTier::OwnerInput,
                    caused_by: None,
                }
            } else {
                p::Provenance {
                    source: p::Source::Internal,
                    actor: p::Actor::System,
                    trust_tier: p::TrustTier::VerifiedProcess,
                    caused_by: None,
                }
            },
        ))?;
        self.services().memory.refresh_projection()?;
        Ok(spec)
    }
}

impl crate::ReactiveHarness {
    pub(crate) fn record_production_strategy_candidate(
        &self,
        c: p::StrategyCandidate,
    ) -> p::Result<p::EventId> {
        c.validate()?;
        if !matches!(
            (&c.provenance.actor, c.provenance.trust_tier),
            (p::Actor::Owner, p::TrustTier::OwnerInput)
                | (p::Actor::System, p::TrustTier::VerifiedProcess)
        ) {
            return Err(p::Error(
                "production strategy candidate provenance is untrusted".into(),
            ));
        }
        let services = self
            .brain_services
            .as_ref()
            .ok_or_else(|| p::Error("owner brain is absent".into()))?;
        let body = ProductionStrategyRegistry {
            store: (*services.store).clone(),
            owner: p::VerifiedPrincipal(services.aggregates.owner.0.clone()),
            aggregate: services.aggregates.memory_cognition.clone(),
        }
        .resolve(&c.proposed_version)?;
        if body.domain() != c.domain
            || body.scope() != &c.scope
            || body.content_ref() != &c.spec_ref
            || body.digest() != &c.spec_digest
        {
            return Err(p::Error(
                "candidate differs from its immutable production body".into(),
            ));
        }
        let id = p::EventId(format!(
            "production-strategy-candidate:{}",
            p::canonical_digest(&c)?.0
        ));
        let provenance = c.provenance.clone();
        let result = services.store.append(p::Event::new(
            id,
            services.aggregates.memory_cognition.clone(),
            None,
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: c.candidate.clone(),
                target: p::CandidateTargetRef(format!(
                    "strategy:{:?}:{}",
                    c.domain, c.proposed_version.0
                )),
                evidence_refs: c.evidence.clone(),
                confidence: p::Confidence(0.5),
                provenance: provenance.clone(),
                target_tier: c.target_tier,
                capability_update: None,
                strategy_candidate: Some(c),
            }),
            p::SchemaVersion(1),
            0,
            provenance,
        ))?;
        services.memory.refresh_projection()?;
        Ok(result)
    }
}

pub(crate) fn production_m3(
    store: SqliteEventStore,
    owner: p::VerifiedPrincipal,
    aggregate: p::RunId,
    compatibility: M3RuntimeCompatibility,
) -> p::Result<Arc<M3DomainRuntime>> {
    let registry = Arc::new(ProductionStrategyRegistry {
        store: store.clone(),
        owner,
        aggregate,
    });
    Ok(Arc::new(
        M3DomainRuntime::new(
            compatibility,
            registry.clone(),
            registry.clone(),
            registry.clone(),
            registry.clone(),
            Vec::new(),
            SelectionEvidenceSet {
                schema_version: p::SchemaVersion(1),
                capability: Vec::new(),
                failures: BTreeMap::new(),
            },
        )?
        .with_production_registry(registry)
        .with_production_evidence(store),
    ))
}

pub(crate) fn production_evidence(
    store: &SqliteEventStore,
    scope: &p::Scope,
) -> p::Result<(
    BTreeMap<p::ModelProfileRef, forme_models::ModelOutcomeEvidence>,
    SelectionEvidenceSet,
)> {
    let mut models = BTreeMap::new();
    let mut selection = SelectionEvidenceSet {
        schema_version: p::SchemaVersion(1),
        capability: Vec::new(),
        failures: BTreeMap::new(),
    };
    for run in store.run_ids()? {
        let events = store.read_run(run).collect::<p::Result<Vec<_>>>()?;
        let mut run_scope = None;
        let mut model = None;
        let mut tool = None;
        let mut completed = None;
        let mut verified = std::collections::BTreeSet::new();
        let typed_turns = events.iter().filter(|e| matches!(&e.payload,
            p::EventPayload::CapabilityEvidenceRecorded(_) if e.provenance.trust_tier == p::TrustTier::VerifiedProcess))
            .map(|e| e.turn_id.clone()).collect::<std::collections::BTreeSet<_>>();
        for e in &events {
            match &e.payload {
                p::EventPayload::SessionBound(p) => {
                    run_scope = Some(p::Scope(p.workspace.0.clone()))
                }
                p::EventPayload::ModelCallStarted(p) => model = Some(p.model_profile.clone()),
                p::EventPayload::ToolCallProposed(p) => {
                    tool = Some((p::CapabilityRef(p.tool.0.clone()), e.turn_id.clone()));
                    completed = None;
                }
                p::EventPayload::ActionCompleted(p) => {
                    completed = Some((p.intent_id.clone(), e.turn_id.clone()))
                }
                p::EventPayload::ActionFailed(_) | p::EventPayload::ActionOutcomeUnknown(_) => {
                    completed = None
                }
                p::EventPayload::VerificationFinished(p)
                    if run_scope
                        .as_ref()
                        .is_some_and(|s| p::charter_scope_contains(scope, s))
                        && !typed_turns.contains(&e.turn_id)
                        && completed.as_ref().is_some_and(|(_, t)| t == &e.turn_id)
                        && tool.as_ref().is_some_and(|(_, t)| t == &e.turn_id)
                        && verified.insert(e.turn_id.clone()) =>
                {
                    let pass = p.outcome == p::VerificationOutcome::Pass;
                    let cap = tool.as_ref().unwrap().0.clone();
                    selection.capability.push(p::CapabilityEvidence {
                        schema_version: p::SchemaVersion(1),
                        capability: cap,
                        outcome: p::CapabilityOutcome(if pass { "pass" } else { "fail" }.into()),
                        reliability: p::Reliability("verified".into()),
                    });
                    if let Some(profile) = &model {
                        let evidence = models.entry(profile.clone()).or_insert_with(|| {
                            forme_models::ModelOutcomeEvidence {
                                schema_version: p::SchemaVersion(1),
                                profile: profile.clone(),
                                // Result evidence does not prove a frontier intelligence band.
                                measured_strength: p::ModelStrengthBand::Standard,
                                verified_outcomes: 0,
                                failures: 0,
                                evidence_refs: Vec::new(),
                            }
                        });
                        if pass {
                            evidence.verified_outcomes += 1;
                        } else {
                            evidence.failures += 1;
                        }
                        evidence
                            .evidence_refs
                            .push(p::EvidenceRef(e.event_id.0.clone()));
                    }
                }
                p::EventPayload::CapabilityEvidenceRecorded(p)
                    if e.provenance.trust_tier == p::TrustTier::VerifiedProcess
                        && p.reliability.0 == "verified"
                        && run_scope
                            .as_ref()
                            .is_some_and(|s| p::charter_scope_contains(scope, s))
                        && completed.as_ref().is_some_and(|(_, t)| t == &e.turn_id)
                        && verified.insert(e.turn_id.clone()) =>
                {
                    let pass = p.outcome.0 == "pass";
                    selection.capability.push(p::CapabilityEvidence {
                        schema_version: p::SchemaVersion(1),
                        capability: p.capability.clone(),
                        outcome: p.outcome.clone(),
                        reliability: p.reliability.clone(),
                    });
                    tool = Some((p.capability.clone(), e.turn_id.clone()));
                    if let Some(profile) = &model {
                        let evidence = models.entry(profile.clone()).or_insert_with(|| {
                            forme_models::ModelOutcomeEvidence {
                                schema_version: p::SchemaVersion(1),
                                profile: profile.clone(),
                                measured_strength: p::ModelStrengthBand::Standard,
                                verified_outcomes: 0,
                                failures: 0,
                                evidence_refs: Vec::new(),
                            }
                        });
                        if pass {
                            evidence.verified_outcomes += 1;
                        } else {
                            evidence.failures += 1;
                        }
                        evidence
                            .evidence_refs
                            .push(p::EvidenceRef(e.event_id.0.clone()));
                    }
                }
                p::EventPayload::FailureEvidenceRecorded(p)
                    if p::charter_scope_contains(scope, &p.scope) =>
                {
                    if let Some((cap, _)) = &tool {
                        selection
                            .failures
                            .entry(cap.clone())
                            .or_default()
                            .push(p.failure_ref.clone());
                    }
                }
                _ => {}
            }
        }
    }
    Ok((models, selection))
}
