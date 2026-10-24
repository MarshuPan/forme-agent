//! The production owner-brain tick engine (prd/23 B1, architecture/09 §5).
//! One claimed window runs the three cognition phases in order. Impulses pass
//! the same value, competence, policy, and emission guards as every proactive
//! proposal; reflections become low-confidence candidates; maintenance only
//! marks stale stable cognition for re-evaluation. Nothing here starts an
//! action, promotes cognition, or takes a foreground session lock.

use std::collections::{BTreeMap, BTreeSet};

use forme_cognition::{
    self as cognition, CognitiveMapStore, CognitiveTickEngine, CognitiveTickInput,
    CognitiveTickPhase, CognitiveTickPhaseReport, CognitiveTickReport,
};
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore};

use crate::{followup_link_run, internal_provenance, ReactiveHarness, TickAppender, TickLabels};

pub(crate) const BRAIN_TICK_RUN_PREFIX: &str = "brain-tick:";
const FOREGROUND_DEFERRAL: &str =
    "a foreground session is active; heavy phases wait for a later window";
const PRIOR_PHASE_FAILED: &str = "an earlier phase of this window failed";

pub(crate) fn brain_tick_run(window: p::Timestamp) -> p::RunId {
    p::RunId(format!("{BRAIN_TICK_RUN_PREFIX}{window}"))
}

/// True for streams a tick reads as input. Tick streams hold tick output and
/// the brain aggregates hold the facts the phases write, so reading either
/// would feed the tick its own conclusions.
pub(crate) fn is_tick_input(run: &p::RunId, aggregates: &BTreeSet<p::RunId>) -> bool {
    !run.0.starts_with(BRAIN_TICK_RUN_PREFIX)
        && !run.0.starts_with("tick:")
        && !aggregates.contains(run)
}

/// The engine over one harness and its owner-brain cognition.
pub(crate) struct HarnessCognitiveTick<'a> {
    pub(crate) harness: &'a ReactiveHarness,
    pub(crate) cognition: &'a cognition::CognitiveRuntime<SqliteEventStore>,
    /// The aggregate stream cognition and memory append their facts to.
    pub(crate) cognition_stream: p::RunId,
    /// Age after which stable cognition without reproduction is re-evaluated.
    pub(crate) stale_after_ms: i64,
    pub(crate) relevance_mode: p::RelevanceMode,
}

/// What earlier attempts of the same window already recorded in its stream.
#[derive(Default)]
struct PriorAttempts {
    proposals: BTreeSet<p::ProposalRef>,
    candidates: BTreeSet<p::CandidateId>,
    reflections: BTreeSet<Vec<p::CandidateId>>,
    maintenance: BTreeSet<p::MemoryDeltasRef>,
}

impl PriorAttempts {
    fn read(store: &SqliteEventStore, run: &p::RunId) -> p::Result<Self> {
        let mut prior = Self::default();
        for event in store.read_run(run.clone()) {
            match event?.payload {
                p::EventPayload::ProactiveProposalEmitted(payload) => {
                    prior.proposals.insert(payload.proposal_ref);
                }
                p::EventPayload::CandidateCreated(payload) => {
                    prior.candidates.insert(payload.candidate_id);
                }
                p::EventPayload::ReflectionProduced(payload) => {
                    prior.reflections.insert(payload.candidate_refs);
                }
                p::EventPayload::MemoryMaintenanceApplied(payload) => {
                    prior.maintenance.insert(payload.deltas_ref);
                }
                _ => {}
            }
        }
        Ok(prior)
    }
}

impl CognitiveTickEngine for HarnessCognitiveTick<'_> {
    fn run(&self, input: CognitiveTickInput) -> p::Result<CognitiveTickReport> {
        let prior = PriorAttempts::read(&self.harness.store, &input.run)?;
        let appender = TickAppender::sequenced(
            format!("{}:attempt:{}", input.run.0, input.claim.attempt),
            input.now,
        );
        let mut phases = Vec::with_capacity(CognitiveTickPhase::ORDER.len());
        let mut failed = false;
        for phase in CognitiveTickPhase::ORDER {
            let report = if failed {
                CognitiveTickPhaseReport::skipped(phase, PRIOR_PHASE_FAILED)
            } else if phase.is_heavy()
                && (input.foreground_active || self.harness.foreground_active()?)
            {
                CognitiveTickPhaseReport::deferred(phase, FOREGROUND_DEFERRAL)
            } else {
                let mut events = Vec::new();
                let result = match phase {
                    CognitiveTickPhase::ObserveAndPropose => {
                        self.observe(&input, &prior, &appender)
                    }
                    CognitiveTickPhase::ReflectAndCandidate => {
                        self.reflect(&input, &prior, &appender, &mut events)
                    }
                    CognitiveTickPhase::MaintainMemory => {
                        self.maintain(&input, &prior, &appender, &mut events)
                    }
                };
                events.extend(appender.drain());
                let mut phase_report = match result {
                    Ok(ran) => CognitiveTickPhaseReport::completed(phase, ran, events),
                    Err(error)
                        if phase == CognitiveTickPhase::ObserveAndPropose
                            && error.0 == FOREGROUND_DEFERRAL =>
                    {
                        let mut deferred =
                            CognitiveTickPhaseReport::deferred(phase, FOREGROUND_DEFERRAL);
                        deferred.event_refs = events;
                        deferred
                    }
                    Err(error) => {
                        failed = true;
                        CognitiveTickPhaseReport::failed(phase, events, error.0)
                    }
                };
                phase_report.model_calls = self
                    .harness
                    .store
                    .read_run(input.run.clone())
                    .collect::<p::Result<Vec<_>>>()?
                    .into_iter()
                    .filter(|event| {
                        phase_report.event_refs.contains(&event.event_id)
                            && event.kind == p::EventKind::ModelCallStarted
                    })
                    .count() as u32;
                phase_report
            };
            phases.push(report);
        }
        CognitiveTickReport::seal(input.claim, input.run, phases)
    }
}

impl HarnessCognitiveTick<'_> {
    /// Phase 1: due commitments and failure tensions become impulses governed
    /// like every proactive proposal. With a foreground session active only
    /// due commitments run; failure observations wait with the rest of the
    /// range, so the owner's session is never interrupted by background work.
    fn observe(
        &self,
        input: &CognitiveTickInput,
        prior: &PriorAttempts,
        appender: &TickAppender,
    ) -> p::Result<bool> {
        // Each failure is proposed once: one the scheduler's failure
        // follow-up already turned into a proposal is left to it, and every
        // event of an impulse cites its failure, so that follow-up in turn
        // skips a failure the tick handled first.
        let mut sources = BTreeMap::new();
        let mut observations = Vec::new();
        let mut learning = BTreeMap::new();
        if !input.foreground_active && !self.harness.foreground_active()? {
            if let Some(activation) = &input.activation {
                let config = cognition::ActivationConfig::default();
                let mut surfaced = BTreeSet::new();
                let mut remaining = config.per_tick_nodes;
                for (seed, shape) in &activation.seeds {
                    if self.harness.foreground_active()? {
                        return Err(p::Error(FOREGROUND_DEFERRAL.into()));
                    }
                    if remaining == 0 || surfaced.len() >= config.top_k_frontier {
                        break;
                    }
                    let Some(node) = activation.graph.nodes.get(seed) else {
                        continue;
                    };
                    let bounded = cognition::ActivationConfig {
                        per_tick_nodes: remaining,
                        ..config.clone()
                    };
                    let result = if self.relevance_mode == p::RelevanceMode::Diffusion {
                        activation.graph.activate(
                            std::slice::from_ref(seed),
                            *shape,
                            &node.scope,
                            &bounded,
                            input.now,
                        )?
                    } else {
                        use cognition::RelevanceBaseline;
                        let services = self.harness.brain_services().ok_or_else(|| {
                            p::Error("relevance requires production brain".into())
                        })?;
                        let candidates = activation
                            .graph
                            .nodes
                            .values()
                            .filter(|candidate| {
                                candidate.scope == node.scope
                                    && candidate.provenance.trust_tier != p::TrustTier::Untrusted
                            })
                            .take(remaining.min(256))
                            .cloned()
                            .collect::<Vec<_>>();
                        let model = services
                            .model_registry
                            .get(services.model_registry.cognition_ref())
                            .ok_or_else(|| p::Error("no bound relevance model".into()))?;
                        let baseline = crate::brain_relevance::ProviderRelevanceBaseline {
                            model,
                            store: &services.store,
                            run: input.run.clone(),
                            measurement: std::sync::Mutex::new(None),
                            events: std::sync::Mutex::new(vec![]),
                            reuse_completed: true,
                        };
                        let selected = baseline.select(
                            &cognition::RecallCue {
                                schema_version: p::SchemaVersion(1),
                                scope: node.scope.clone(),
                                query: format!(
                                    "Find useful context for {:?}: {}",
                                    shape, node.content_ref.0
                                ),
                            },
                            &candidates,
                            config.top_k_frontier,
                        );
                        for id in baseline
                            .events
                            .lock()
                            .map_err(|_| p::Error("relevance receipts unavailable".into()))?
                            .drain(..)
                        {
                            appender.record_existing(id);
                        }
                        let selected = selected?;
                        cognition::ActivationRun {
                            schema_version: p::SchemaVersion(1),
                            snapshot: Some(activation.graph.digest.clone()),
                            shape: *shape,
                            considered: candidates.len().min(remaining),
                            hops: 0,
                            truncated: false,
                            surfaced: selected
                                .into_iter()
                                .map(|node| cognition::Activated {
                                    schema_version: p::SchemaVersion(1),
                                    node,
                                    level: p::Confidence(0.8),
                                })
                                .collect(),
                        }
                    };
                    remaining = remaining.saturating_sub(result.considered);
                    for active in result.surfaced {
                        if surfaced.len() >= config.top_k_frontier {
                            break;
                        }
                        if !surfaced.insert(active.node.clone()) {
                            continue;
                        }
                        let target = &activation.graph.nodes[&active.node];
                        let signal = match shape {
                            cognition::ActivationShape::Association => {
                                cognition::ImpulseSource::Association
                            }
                            cognition::ActivationShape::Change => cognition::ImpulseSource::Change,
                            cognition::ActivationShape::Pressure => {
                                cognition::ImpulseSource::Pressure
                            }
                            cognition::ActivationShape::Gap => cognition::ImpulseSource::Gap,
                            cognition::ActivationShape::Tension => {
                                cognition::ImpulseSource::Tension
                            }
                        };
                        observations.push(cognition::Observation {
                            schema_version: p::SchemaVersion(1),
                            signal,
                            scope: target.scope.scope.clone(),
                            seed: vec![active.node],
                            estimated_value: u64::from((active.level.0 * 90.0) as u8),
                            urgency: 50,
                            source: p::Source::Internal,
                            grant_ref: None,
                            authorized: true,
                            proposal_intent: cognition::ProposalIntent::Communication(
                                cognition::CommunicationPurpose::Insight,
                            ),
                            delivery: cognition::DeliveryMode::Hitchhike,
                            requested_level: cognition::InterventionLevel::L1Suggest,
                        });
                    }
                }
            }
            let current_learning = self
                .harness
                .brain_services()
                .map(|services| {
                    let mut events = Vec::new();
                    for run in services.store.run_ids()? {
                        events.extend(
                            services
                                .store
                                .read_run(run)
                                .collect::<p::Result<Vec<_>>>()?,
                        );
                    }
                    Ok::<_, p::Error>(cognition::LearningSnapshot::rebuild(&events))
                })
                .transpose()?;
            for gap in input.learning.gaps(&input.stable_objects)? {
                if current_learning
                    .as_ref()
                    .is_some_and(|current| current.origin_seen(&gap.record.origin))
                {
                    continue;
                }
                observations.push(gap.observation());
                learning.insert(gap.record.seed(), gap.record);
            }
            for failure in &input.signals.failures {
                if self.followed_up(&failure.source)? {
                    continue;
                }
                sources.insert(failure.seed(), failure.source.clone());
                observations.push(failure.observation());
            }
        }
        let snapshot = cognition::CognitionSnapshot {
            observations,
            ..input.snapshot.clone()
        };
        let labels = TickLabels::Brain {
            window: input.claim.window,
        };
        let impulses = self
            .harness
            .proactivity
            .tick(cognition::TickTrigger::Schedule, &snapshot);
        let mut ran = false;
        for (index, mut impulse) in impulses.into_iter().enumerate() {
            let snapshot = if let Some(services) = self.harness.brain_services() {
                let competence = services.competence_inputs_at(
                    &impulse.scope,
                    None,
                    input.now,
                    Some(&input.claim.range.to),
                )?;
                impulse.capability_evidence = competence
                    .capability_evidence
                    .iter()
                    .map(|e| e.reference.clone())
                    .collect();
                cognition::CognitionSnapshot {
                    competence,
                    ..snapshot.clone()
                }
            } else {
                snapshot.clone()
            };
            let learning_record = impulse
                .seed
                .iter()
                .find_map(|seed| learning.get(seed).cloned());
            let provenance = p::Provenance {
                caused_by: impulse
                    .seed
                    .iter()
                    .find_map(|seed| sources.get(seed).cloned()),
                ..internal_provenance(p::Source::Internal)
            };
            let origin = impulse.origin_key();
            let fulfilled = labels
                .proposal(&origin)
                .is_some_and(|reference| prior.proposals.contains(&reference))
                || prior.candidates.contains(&labels.candidate(index, &origin));
            if fulfilled {
                // An earlier attempt of this window already governed it.
                ran = true;
                continue;
            }
            let outcome = self.harness.govern_impulse(
                appender,
                &labels,
                &input.run,
                &provenance,
                index,
                impulse,
                &snapshot,
                learning_record,
            )?;
            ran |= outcome.proposal_event.is_some() || outcome.candidate_event.is_some();
        }
        Ok(ran || appender.has_pending())
    }

    /// Phase 2: each failure group becomes a low-confidence blind-spot
    /// candidate with the group's evidence. Candidate ids derive from the
    /// group, so a retried window confirms rather than repeats them.
    fn reflect(
        &self,
        input: &CognitiveTickInput,
        prior: &PriorAttempts,
        appender: &TickAppender,
        events: &mut Vec<p::EventId>,
    ) -> p::Result<bool> {
        if input.signals.groups.is_empty() {
            return Ok(false);
        }
        let head = self.harness.store.run_head(&self.cognition_stream)?;
        let mut candidates = Vec::new();
        let mut inputs = Vec::<p::EvidenceRef>::new();
        let result = (|| {
            for group in &input.signals.groups {
                candidates.push(group.candidate_id());
                self.cognition.propose_update(group.reflection())?;
                for evidence in &group.evidence {
                    if !inputs.contains(evidence) {
                        inputs.push(evidence.clone());
                    }
                }
            }
            Ok::<_, p::Error>(())
        })();
        let produced = candidates.iter().collect::<BTreeSet<_>>();
        events.extend(self.cognition_events_after(head, |payload| {
            match payload {
                p::EventPayload::ReflectionProduced(payload) => payload
                    .candidate_refs
                    .iter()
                    .any(|candidate| produced.contains(candidate)),
                p::EventPayload::CognitiveMapUpdateProposed(payload) => {
                    produced.contains(&payload.candidate_id)
                }
                p::EventPayload::CandidateCreated(payload) => {
                    produced.contains(&payload.candidate_id)
                }
                _ => false,
            }
        })?);
        // Collect the partial chain even when a later append failed.
        result?;
        if !prior.reflections.contains(&candidates) {
            self.harness.tick_append(
                appender,
                input.run.clone(),
                None,
                internal_provenance(p::Source::Internal),
                p::EventPayload::ReflectionProduced(p::ReflectionProducedPayload {
                    inputs,
                    candidate_refs: candidates,
                }),
            )?;
        }
        Ok(true)
    }

    /// Phase 3: stable cognition whose evidence aged without reproduction is
    /// marked for re-evaluation with a downgrade candidate. The window then
    /// records the maintenance it applied in its own stream.
    fn maintain(
        &self,
        input: &CognitiveTickInput,
        prior: &PriorAttempts,
        appender: &TickAppender,
        events: &mut Vec<p::EventId>,
    ) -> p::Result<bool> {
        let head = self.harness.store.run_head(&self.cognition_stream)?;
        let services = self
            .harness
            .brain_services()
            .ok_or_else(|| p::Error("maintenance requires production brain".into()))?;
        let graph = services
            .memory
            .graph_snapshot_at(Some(input.claim.range.to.position(&self.cognition_stream)))?;
        let result = self.cognition.audit_maintenance_snapshot(
            cognition::Tick {
                schema_version: p::SchemaVersion(1),
                now: input.now,
                stale_after_ms: self.stale_after_ms,
            },
            &input.stable_objects,
            &graph,
        );
        let targets = input
            .stable_objects
            .iter()
            .map(|stable| format!("downgrade:{}", stable.object.0))
            .collect::<BTreeSet<_>>();
        let failures = input
            .stable_objects
            .iter()
            .map(|stable| format!("memory-misevolution:{}:{}", stable.object.0, input.now))
            .collect::<BTreeSet<_>>();
        let triggers = input
            .stable_objects
            .iter()
            .map(|stable| format!("stale:{}", stable.object.0))
            .collect::<BTreeSet<_>>();
        events.extend(self.cognition_events_after(head, |payload| match payload {
            p::EventPayload::FailureEvidenceRecorded(payload) => {
                payload.class == p::FailureClass::MemoryMisevolution
                    && (failures.contains(&payload.failure_ref.0)
                        || payload.failure_ref.0.starts_with("memory-maintenance:"))
            }
            p::EventPayload::ReevaluationTaskCreated(payload) => {
                triggers.contains(&payload.trigger.0)
                    || payload.trigger.0.starts_with("maintenance:")
            }
            p::EventPayload::CandidateCreated(payload) => {
                (payload.candidate_id.0.starts_with("candidate:stale:")
                    || payload.candidate_id.0.starts_with("candidate:maintenance:"))
                    && targets.contains(&payload.target.0)
            }
            _ => false,
        })?);
        let downgrades = result?;
        let graph_receipt = services.memory.maintain_low_value_branches(
            &graph,
            input.now,
            self.stale_after_ms,
            p::IdempotencyKey(format!(
                "brain-maintenance:{}:{}",
                input.claim.window, graph.digest.0
            )),
        )?;
        if let Some(receipt) = &graph_receipt {
            events.push(receipt.clone());
        }
        if downgrades.is_empty() && graph_receipt.is_none() {
            return Ok(false);
        }
        if downgrades.is_empty() {
            return Ok(true);
        }
        let deltas = maintenance_deltas(input.claim.window, &downgrades);
        if !prior.maintenance.contains(&deltas) {
            self.harness.tick_append(
                appender,
                input.run.clone(),
                None,
                internal_provenance(p::Source::Internal),
                p::EventPayload::MemoryMaintenanceApplied(p::MemoryMaintenanceAppliedPayload {
                    deltas_ref: deltas,
                }),
            )?;
        }
        Ok(true)
    }

    fn followed_up(&self, source: &p::EventId) -> p::Result<bool> {
        Ok(self.harness.store.run_head(&followup_link_run(source))? > 0)
    }

    /// Events the cognition stream gained after `head` that belong to the
    /// current phase; unrelated concurrent appends are never attributed.
    fn cognition_events_after(
        &self,
        head: u64,
        belongs: impl Fn(&p::EventPayload) -> bool,
    ) -> p::Result<Vec<p::EventId>> {
        let mut events = Vec::new();
        for event in self
            .harness
            .store
            .read_run(self.cognition_stream.clone())
            .after(head)
        {
            let event = event?;
            if belongs(&event.payload) {
                events.push(event.event_id);
            }
        }
        Ok(events)
    }
}

/// The window's maintenance record: which downgrade candidates it opened,
/// identified by a digest so the record stays bounded.
fn maintenance_deltas(window: p::Timestamp, downgrades: &[p::CandidateId]) -> p::MemoryDeltasRef {
    let mut sorted = downgrades
        .iter()
        .map(|id| id.0.as_str())
        .collect::<Vec<_>>();
    sorted.sort_unstable();
    let digest = p::sha256_content_digest(sorted.join("\n").as_bytes());
    p::MemoryDeltasRef(format!(
        "brain-tick-maintenance|{window}|{}|{}",
        sorted.len(),
        digest.0
    ))
}
