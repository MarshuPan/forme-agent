use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use forme_approval::{ApprovalGrant, GrantScope};
use forme_capabilities::{InMemorySelectionPolicyRegistry, SelectionEvidenceSet, Toolset};
use forme_coordination::{InMemoryCoordinationRegistry, RuleBasedCoordinationReasoner};
use forme_eval::{
    LongHorizonArtifactBody, LongHorizonArtifactCheckpoint, LongHorizonArtifactDisposition,
    M3BArtifactStore,
};
use forme_execution::{
    ActionBackend, ActionResult, ActionStatus, CancelToken, DefaultExecutionPlanner, EventSink,
    ExecutionBackendRegistry, ExecutionPlan, ExecutionPlanner, OutputBudget,
};
use forme_harness::{
    AgentHarness, GovernanceConfig, HarnessConfig, LongHorizonDisposition, LongHorizonOutwardStep,
    LongHorizonProject, M3DomainRuntime, M3EvolutionHarness, M3RuntimeCompatibility,
    ReactiveHarness, ResumeInput,
};
use forme_loop::InMemoryLoopRegistry;
use forme_models::{
    Cost, InMemoryModelAdaptationRegistry, ModelCapability, ModelOutcomeEvidence, ModelOutput,
    ModelProfile, ModelResponse, ModelStrength, RateLimit, ScriptedModelProvider, Url,
};
use forme_policy::{
    ActionMatcher, ArgMatcher, DelegationGrant, DelegationSubject, PolicyLayer, PolicyLayerSource,
    PolicyRule,
};
use forme_protocol as p;
use forme_store::{EventStore, EvolutionEventStore, SqliteEventStore, StoreOptions};

fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn event(id: &str, run: &str, payload: p::EventPayload) -> p::Event {
    p::Event::new(
        p::EventId(id.into()),
        p::RunId(run.into()),
        None,
        payload,
        p::SchemaVersion(1),
        1,
        provenance(),
    )
}

fn seed_baseline(store: &SqliteEventStore, domain: p::StrategyDomain, suffix: &str) {
    let baseline = p::StrategyVersionRef(format!("baseline:{suffix}"));
    let candidate = p::StrategyCandidate {
        schema_version: p::SchemaVersion(1),
        candidate: p::CandidateId(format!("candidate:baseline:{suffix}")),
        domain,
        scope: p::Scope("workspace:default".into()),
        target_tier: p::StabilityTier::Stable,
        proposed_version: baseline.clone(),
        baseline: p::StrategyVersionRef(format!("seed:{suffix}")),
        spec_ref: p::ContentRef(format!("spec:baseline:{suffix}")),
        spec_digest: p::SchemaDigest(format!("digest:baseline:{suffix}")),
        evidence: vec![p::EvidenceRef(format!("evidence:baseline:{suffix}"))],
        provenance: provenance(),
        impact: p::EvolutionImpact::Cautious,
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: p::StrategyVersionRef(format!("seed:{suffix}")),
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    };
    store
        .append(event(
            &format!("event:baseline:candidate:{suffix}"),
            &format!("run:baseline:candidate:{suffix}"),
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: candidate.candidate.clone(),
                target: p::CandidateTargetRef(format!("strategy:baseline:{suffix}")),
                evidence_refs: candidate.evidence.clone(),
                confidence: p::Confidence(1.0),
                provenance: provenance(),
                target_tier: p::StabilityTier::Stable,
                capability_update: None,
                strategy_candidate: Some(candidate.clone()),
            }),
        ))
        .unwrap();
    let evaluation = p::EvolutionEvaluationRef(format!("evaluation:baseline:{suffix}"));
    store
        .append(event(
            &format!("event:baseline:evaluation:{suffix}"),
            &format!("run:baseline:evaluation:{suffix}"),
            p::EventPayload::EvolutionEvaluationRecorded(p::EvolutionEvaluationRecordedPayload {
                evaluation: evaluation.clone(),
                baseline: candidate.baseline.clone(),
                candidate: baseline.clone(),
                verdict: p::EvaluationVerdict::Pass,
                hard_invariants: vec![p::InvariantResultRef(format!(
                    "invariant:baseline:{suffix}"
                ))],
                ground_truth: vec![p::EvidenceRef(format!("ground-truth:baseline:{suffix}"))],
            }),
        ))
        .unwrap();
    let promotion = p::EventId(format!("event:baseline:promotion:{suffix}"));
    store
        .append(event(
            &promotion.0,
            &format!("run:baseline:promotion:{suffix}"),
            p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                candidate_id: candidate.candidate,
                by: p::DecisionActor::Auto,
                reason: p::ReasonRef("repository-owned seed baseline".into()),
            }),
        ))
        .unwrap();
    let aggregate = p::EvolutionAggregateRef("evolution:owner:workspace-default".into());
    let expected = store.evolution_version(&aggregate).unwrap();
    let committed = expected.next().unwrap();
    let mut activation = event(
        &format!("event:baseline:activation:{suffix}"),
        &format!("run:baseline:activation:{suffix}"),
        p::EventPayload::StrategyActivated(p::StrategyActivatedPayload {
            activation: p::StrategyActivation {
                schema_version: p::SchemaVersion(1),
                aggregate: aggregate.clone(),
                domain,
                scope: p::Scope("workspace:default".into()),
                from: None,
                to: baseline,
                spec_ref: candidate.spec_ref,
                spec_digest: candidate.spec_digest,
                evaluation,
                promotion,
                owner_confirmation: None,
                impact: p::EvolutionImpact::Cautious,
                expected_version: expected.clone(),
                committed_version: committed,
            },
            active_snapshot: p::EvolutionSnapshotRef("pending-preview".into()),
        }),
    );
    let preview = store.preview_evolution_snapshot(&activation).unwrap();
    let p::EventPayload::StrategyActivated(payload) = &mut activation.payload else {
        unreachable!()
    };
    payload.active_snapshot = preview.snapshot;
    store
        .append_evolution_expected(activation, &aggregate, expected)
        .unwrap();
}

fn profile() -> ModelProfile {
    ModelProfile {
        schema_version: p::SchemaVersion(1),
        provider: p::ProviderId("provider:m3-b-harness".into()),
        model: "model-m3-b-harness".into(),
        base_url: Url::parse("https://models.invalid/v1").unwrap(),
        capability: ModelCapability {
            schema_version: p::SchemaVersion(1),
            context_window: 32_000,
            tool_use: true,
            strength: ModelStrength::Standard,
        },
        cost: Cost {
            schema_version: p::SchemaVersion(1),
            input_microunits_per_million: 1,
            output_microunits_per_million: 1,
        },
        rate_limit: RateLimit {
            schema_version: p::SchemaVersion(1),
            requests_per_minute: 60,
            tokens_per_minute: 100_000,
        },
        credential_ref: p::CredentialRef("secret-ref:m3-b-harness".into()),
    }
}

fn final_response(value: &str) -> ModelResponse {
    ModelResponse {
        schema_version: p::SchemaVersion(1),
        output: ModelOutput::Final(value.into()),
        usage: p::ModelUsage {
            input_tokens: 2,
            output_tokens: 1,
        },
        finish_reason: p::FinishReason("stop".into()),
    }
}

fn compatibility() -> p::StrategyRuntimeCompatibility {
    p::StrategyRuntimeCompatibility {
        schema_version: p::SchemaVersion(1),
        minimum_runtime_schema: p::SchemaVersion(1),
        event_schema: p::SchemaVersion(1),
        model_profile: Some(profile().profile_ref()),
        tool_schema: Some(p::SchemaDigest("tool-schema:m3-b".into())),
        backend_schema: Some(p::SchemaDigest("backend-schema:m3-b".into())),
    }
}

fn loop_spec() -> p::LoopStrategySpec {
    p::LoopStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef("loop:m3-b:v1".into()),
        scope: p::Scope("workspace:default".into()),
        content_ref: p::ContentRef("spec:loop:m3-b:v1".into()),
        content_digest: p::SchemaDigest("digest:loop:m3-b:v1".into()),
        compatibility: compatibility(),
        phases: vec![
            p::LoopPhase::Context,
            p::LoopPhase::Deliberate,
            p::LoopPhase::Policy,
            p::LoopPhase::Approval,
            p::LoopPhase::Execute,
            p::LoopPhase::Checkpoint,
            p::LoopPhase::Verify,
            p::LoopPhase::Finish,
        ],
        triggers: vec![p::LoopTrigger::Reactive, p::LoopTrigger::CheckpointResume],
        checkpoint_cadence_turns: 1,
        verification_cadence_turns: 1,
        budget: p::LoopBudgetProfile {
            schema_version: p::SchemaVersion(1),
            max_turns: 4,
            max_tokens: 100,
            max_wall_time_ms: 30_000,
            max_cost_microunits: 10_000,
            max_tool_calls: 2,
        },
        failure_fallback: p::LoopFailureFallback::PreserveCheckpoint,
    }
}

fn coordination_spec() -> p::CoordinationStrategySpec {
    p::CoordinationStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef("coordination:m3-b:v1".into()),
        scope: p::Scope("workspace:default".into()),
        content_ref: p::ContentRef("spec:coordination:m3-b:v1".into()),
        content_digest: p::SchemaDigest("digest:coordination:m3-b:v1".into()),
        compatibility: compatibility(),
        applicability: p::CoordinationApplicabilitySpec {
            schema_version: p::SchemaVersion(1),
            scale: p::CoordinationTaskScale::MultiStage,
            decomposability: p::CoordinationDecomposability::Sequential,
            verifiability: p::CoordinationVerifiability::IndependentReview,
        },
        patterns: vec![p::CoordinationPatternSpec {
            schema_version: p::SchemaVersion(1),
            pattern: p::OrchestrationPatternRef("pattern:review".into()),
            topology: p::CoordinationTopology::ReviewGate,
        }],
        mode: p::CoordinationMode::Multi,
        role_weights: vec![p::CoordinationRoleWeight {
            schema_version: p::SchemaVersion(1),
            role: p::RoleRef("role:worker".into()),
            weight_basis_points: 10_000,
        }],
        max_subagents: 1,
        checkpoint_topology: p::CheckpointTopology::ReviewGate,
    }
}

fn selection_spec(target: p::SelectionTarget, version: &str) -> p::SelectionStrategySpec {
    p::SelectionStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef(version.into()),
        scope: p::Scope("workspace:default".into()),
        content_ref: p::ContentRef(format!("spec:{version}")),
        content_digest: p::SchemaDigest(format!("digest:{version}")),
        compatibility: compatibility(),
        target,
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
        fallback: p::SelectionFallback::NoSelection,
        max_results: 8,
    }
}

fn model_spec() -> p::ModelAdaptationSpec {
    p::ModelAdaptationSpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef("model-adaptation:m3-b:v1".into()),
        scope: p::Scope("workspace:default".into()),
        content_ref: p::ContentRef("spec:model-adaptation:m3-b:v1".into()),
        content_digest: p::SchemaDigest("digest:model-adaptation:m3-b:v1".into()),
        compatibility: compatibility(),
        predicate: p::ModelCapabilityPredicate {
            schema_version: p::SchemaVersion(1),
            minimum_context_window: 8_192,
            requires_tool_use: true,
            minimum_strength: p::ModelStrengthBand::Standard,
        },
        scaffold: p::ModelScaffoldProfile {
            schema_version: p::SchemaVersion(1),
            externalized_steps: 3,
            verification_passes: 2,
            checkpoint_cadence_steps: 1,
        },
    }
}

fn runtime(
    loop_specs: Vec<p::LoopStrategySpec>,
    coordination_spec: p::CoordinationStrategySpec,
    selections: Vec<p::SelectionStrategySpec>,
    model_spec: p::ModelAdaptationSpec,
) -> Arc<M3DomainRuntime> {
    let loop_registry = Arc::new(InMemoryLoopRegistry::with_seed(loop_specs[0].clone()).unwrap());
    for spec in loop_specs.into_iter().skip(1) {
        loop_registry.register(spec).unwrap();
    }
    let coordination_registry =
        Arc::new(InMemoryCoordinationRegistry::with_seed(coordination_spec).unwrap());
    let selection_registry =
        Arc::new(InMemorySelectionPolicyRegistry::with_seed(selections[0].clone()).unwrap());
    for spec in selections.into_iter().skip(1) {
        selection_registry.register(spec).unwrap();
    }
    let model_registry = Arc::new(InMemoryModelAdaptationRegistry::with_seed(model_spec).unwrap());
    Arc::new(
        M3DomainRuntime::new(
            M3RuntimeCompatibility {
                schema_version: p::SchemaVersion(1),
                event_schema: p::SchemaVersion(1),
                tool_schema: p::SchemaDigest("tool-schema:m3-b".into()),
                backend_schema: p::SchemaDigest("backend-schema:m3-b".into()),
            },
            loop_registry,
            coordination_registry,
            selection_registry,
            model_registry,
            vec![ModelOutcomeEvidence {
                schema_version: p::SchemaVersion(1),
                profile: profile().profile_ref(),
                measured_strength: p::ModelStrengthBand::Standard,
                verified_outcomes: 5,
                failures: 0,
                evidence_refs: vec![p::EvidenceRef("evidence:model:m3-b".into())],
            }],
            SelectionEvidenceSet {
                schema_version: p::SchemaVersion(1),
                capability: vec![
                    p::CapabilityEvidence {
                        schema_version: p::SchemaVersion(1),
                        capability: p::CapabilityRef("capability:verified".into()),
                        outcome: p::CapabilityOutcome("pass".into()),
                        reliability: p::Reliability("verified".into()),
                    },
                    p::CapabilityEvidence {
                        schema_version: p::SchemaVersion(1),
                        capability: p::CapabilityRef("capability:declared".into()),
                        outcome: p::CapabilityOutcome("pass".into()),
                        reliability: p::Reliability("declared".into()),
                    },
                ],
                failures: BTreeMap::new(),
            },
        )
        .unwrap(),
    )
}

fn comparison(candidate: &p::StrategyCandidate, suffix: &str) -> p::EvolutionComparison {
    p::EvolutionComparison {
        schema_version: p::SchemaVersion(1),
        evaluation: p::EvolutionEvaluationRef(format!("evaluation:{suffix}")),
        bundle: p::ReplayBundleRef(format!("bundle:{suffix}")),
        baseline: candidate.baseline.clone(),
        candidate: candidate.proposed_version.clone(),
        case_set_digest: p::SchemaDigest(format!("case-set:{suffix}")),
        holdout_digest: p::SchemaDigest(format!("holdout:{suffix}")),
        metrics: vec![p::FitnessMetric {
            schema_version: p::SchemaVersion(1),
            dimension: p::FitnessDimension::Quality,
            outcome: p::FitnessOutcome::Pass,
            measured: Some(10_000),
            unit: p::FitnessUnit::BasisPoints,
            evidence: vec![p::EvidenceRef(format!("ground-truth:{suffix}"))],
        }],
        hard_invariants: vec![p::InvariantResult {
            schema_version: p::SchemaVersion(1),
            reference: p::InvariantResultRef(format!("invariant:{suffix}")),
            name: "harness_first_and_governance_preserved".into(),
            outcome: p::FitnessOutcome::Pass,
            evidence: vec![p::EvidenceRef(format!("invariant-evidence:{suffix}"))],
        }],
        ground_truth: vec![p::EvidenceRef(format!("ground-truth:{suffix}"))],
        independent_verifier: true,
        self_eval_only: false,
    }
}

fn activate(
    store: &SqliteEventStore,
    control: &M3EvolutionHarness,
    domain: p::StrategyDomain,
    version: p::StrategyVersionRef,
    content_ref: p::ContentRef,
    digest: p::SchemaDigest,
    suffix: &str,
) -> p::EvolutionSnapshot {
    seed_baseline(store, domain, suffix);
    activate_from_baseline(
        store,
        control,
        domain,
        version,
        content_ref,
        digest,
        p::StrategyVersionRef(format!("baseline:{suffix}")),
        suffix,
    )
}

#[allow(clippy::too_many_arguments)]
fn activate_from_baseline(
    store: &SqliteEventStore,
    control: &M3EvolutionHarness,
    domain: p::StrategyDomain,
    version: p::StrategyVersionRef,
    content_ref: p::ContentRef,
    digest: p::SchemaDigest,
    baseline: p::StrategyVersionRef,
    suffix: &str,
) -> p::EvolutionSnapshot {
    let run = p::RunId(format!("run:control:{suffix}"));
    let candidate = p::StrategyCandidate {
        schema_version: p::SchemaVersion(1),
        candidate: p::CandidateId(format!("candidate:{suffix}")),
        domain,
        scope: p::Scope("workspace:default".into()),
        target_tier: p::StabilityTier::Stable,
        proposed_version: version,
        baseline: baseline.clone(),
        spec_ref: content_ref,
        spec_digest: digest,
        evidence: vec![p::EvidenceRef(format!("evidence:{suffix}"))],
        provenance: provenance(),
        impact: p::EvolutionImpact::Cautious,
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: baseline,
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    };
    control
        .record_candidate(run.clone(), candidate.clone())
        .unwrap();
    let evaluated = control
        .evaluate(run.clone(), &candidate, comparison(&candidate, suffix))
        .unwrap();
    let promotion = control
        .promote(run.clone(), &candidate, &evaluated.evaluation)
        .unwrap();
    let activated = control
        .activate(
            run.clone(),
            p::EvolutionAggregateRef("evolution:owner:workspace-default".into()),
            &candidate,
            &evaluated.evaluation,
            promotion,
            None,
        )
        .unwrap();
    let control_events = store.read_run(run).collect::<p::Result<Vec<_>>>().unwrap();
    assert!(!control_events.iter().any(|event| matches!(
        event.kind,
        p::EventKind::ApprovalResolved
            | p::EventKind::AutonomyEnvelopeSet
            | p::EventKind::ExternalCommunicationGranted
    )));
    activated.snapshot
}

fn request(suffix: &str) -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef(format!("session:m3-b:{suffix}")),
        agent_profile: p::AgentProfileRef("agent:m3-b".into()),
        input: p::RunInput(format!("M3-B governed task {suffix}")),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey(format!("request:m3-b:{suffix}"))),
    }
}

fn assert_subsequence(actual: &[p::EventKind], expected: &[p::EventKind]) {
    let mut cursor = 0;
    for expected_kind in expected {
        let offset = actual[cursor..]
            .iter()
            .position(|kind| kind == expected_kind)
            .unwrap_or_else(|| panic!("missing {expected_kind:?} after event index {cursor}"));
        cursor += offset + 1;
    }
}

fn now_ms() -> p::Timestamp {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as p::Timestamp
}

struct ExternalRecordingBackend {
    executions: Arc<AtomicUsize>,
}

impl ActionBackend for ExternalRecordingBackend {
    fn kind(&self) -> p::BackendKind {
        p::BackendKind::Browser
    }

    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        DefaultExecutionPlanner::new(OutputBudget::truncate_at(4_096), p::DurationMs(1_000))
            .plan(intent)
    }

    fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        _cancel: CancelToken,
    ) -> p::Result<ActionResult> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        sink.emit(p::EventPayload::ActionStarted(p::ActionStartedPayload {
            intent_id: plan.intent.intent_id.clone(),
            backend: plan.backend,
            scope: plan.scope.clone(),
            remote_lease: None,
        }))?;
        let receipt = p::ExternalActionReceipt {
            schema_version: p::SchemaVersion(1),
            action: plan.intent.intent_id.clone(),
            content_ref: Some(p::ContentRef("artifact:m3-b-outward".into())),
            content_digest: Some(p::SchemaDigest("digest:m3-b-outward".into())),
            trust: p::TrustTier::Untrusted,
            effect: p::EffectStatus::Observed,
            probe_hint: None,
        };
        let result_ref = p::ActionResultRef(format!("result:{}", plan.intent.intent_id.0));
        sink.emit(p::EventPayload::ActionCompleted(
            p::ActionCompletedPayload {
                intent_id: plan.intent.intent_id.clone(),
                result_ref: result_ref.clone(),
                receipt: Some(receipt.clone()),
                remote_receipt: None,
            },
        ))?;
        Ok(ActionResult {
            schema_version: p::SchemaVersion(1),
            result_ref,
            status: ActionStatus::Completed,
            output_ref: p::OutputRef("output:m3-b-outward".into()),
            output: "external result is untrusted data".into(),
            truncated: false,
            evidence: p::CapabilityEvidence {
                schema_version: p::SchemaVersion(1),
                capability: plan.intent.capability_ref,
                outcome: p::CapabilityOutcome("success".into()),
                reliability: p::Reliability("observed".into()),
            },
            diff: None,
            rollback: None,
            external_receipt: Some(receipt),
        })
    }

    fn cancel(&self, _action: p::ActionId) -> p::Result<()> {
        Ok(())
    }
}

fn outward_intent() -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action:m3-b-long-horizon".into()),
        source: p::Source::Schedule,
        goal: p::GoalRef("observe one governed external checkpoint".into()),
        backend_hint: p::BackendKind::Browser,
        capability_ref: p::CapabilityRef("capability:browser".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:default".into()),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Browser(p::BrowserActionSpec {
            schema_version: p::SchemaVersion(1),
            driver: p::ProviderId("driver:m3-b-fixture".into()),
            target_url: "http://127.0.0.1:34002/checkpoint".into(),
            allowed_origins: vec!["http://127.0.0.1:34002".into()],
            operation: p::BrowserOperation::ReadText {
                selector: Some("#checkpoint".into()),
            },
            artifact_scope: p::Scope("workspace:default".into()),
        }),
        requested_permissions: vec![p::PermissionRef("browser:use".into())],
        requested_at: now_ms(),
        estimated_output_bytes: 4_096,
        estimated_duration: p::DurationMs(1_000),
    }
}

fn long_horizon_governance(intent: &p::ActionIntent) -> GovernanceConfig {
    let now = now_ms();
    let envelope = p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: intent.scope.clone(),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![intent.capability_ref.clone()],
            permissions: intent.requested_permissions.clone(),
        },
        action_type: vec![intent.action_type],
        risk_limit: p::Risk::High,
        approval_rule: p::ApprovalRule::Allow,
        budget: p::Budget("units:100".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: now.saturating_sub(1_000),
            expires_at: now.saturating_add(600_000),
            max_turns: 8,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    };
    GovernanceConfig {
        schema_version: p::SchemaVersion(1),
        layers: vec![PolicyLayer {
            schema_version: p::SchemaVersion(1),
            source: PolicyLayerSource::User,
            rules: vec![PolicyRule {
                schema_version: p::SchemaVersion(1),
                matcher: ActionMatcher {
                    backend: Some(p::BackendKind::Browser),
                    capability: Some(intent.capability_ref.clone()),
                    action_type: Some(intent.action_type),
                    parameters: ArgMatcher::Any,
                },
                effect: p::PolicyDecision::Allow,
                scope: intent.scope.clone(),
            }],
        }],
        visible_capabilities: vec![intent.capability_ref.clone()],
        granted_permissions: intent.requested_permissions.clone(),
        allowed_scopes: vec![intent.scope.clone()],
        shell_allowlist: Vec::new(),
        file_roots: Vec::new(),
        mcp_allowlist: Vec::new(),
        external: forme_policy::ExternalPolicyLimits {
            schema_version: p::SchemaVersion(1),
            browser_origins: vec!["http://127.0.0.1:34002".into()],
            computer_surfaces: Vec::new(),
            pty_programs: Vec::new(),
            pty_roots: Vec::new(),
            app_api_connectors: Vec::new(),
        },
        network_allowed: false,
        sandbox_available: true,
        delegation: Some(DelegationGrant {
            schema_version: p::SchemaVersion(1),
            subject: DelegationSubject::Agent,
            envelope: envelope.clone(),
            granted_by: p::Actor::Owner,
            audit_ref: p::EventId("delegation:m3-b-long-horizon".into()),
        }),
        envelope: Some(envelope),
    }
}

fn long_horizon_route(config: &HarnessConfig) -> forme_coordination::ExecutionRoute {
    let toolset = Toolset {
        schema_version: p::SchemaVersion(1),
        toolset_ref: p::ToolsetRef("toolset:m3-b-child".into()),
        items: Vec::new(),
        scope: p::Scope("workspace:default:child".into()),
        sources: Vec::new(),
    };
    forme_coordination::ExecutionRoute {
        schema_version: p::SchemaVersion(1),
        reference: p::ExecutionRouteRef("route:m3-b-long-horizon".into()),
        pattern_ref: Some(p::OrchestrationPatternRef("pattern:review".into())),
        nodes: vec![forme_coordination::RouteNode {
            schema_version: p::SchemaVersion(1),
            subtask: forme_coordination::Subtask {
                schema_version: p::SchemaVersion(1),
                id: "worker".into(),
                instruction: "produce one verified checkpoint slice".into(),
                intent_id: p::ActionId("subtask:m3-b-worker".into()),
            },
            role: forme_coordination::SubagentProfile {
                schema_version: p::SchemaVersion(1),
                role: p::RoleRef("role:worker".into()),
                toolset: toolset.clone(),
                model: config.model_profile.clone(),
                permission: p::Scope("workspace:default:child".into()),
                budget: p::Budget("units:10".into()),
            },
            resource_slice: toolset,
            done: forme_coordination::DoneContract::final_output(p::DoneContractRef(
                "done:m3-b-child".into(),
            )),
            retry: forme_coordination::RetryPolicy {
                schema_version: p::SchemaVersion(1),
                max_attempts: 1,
            },
        }],
        edges: Vec::new(),
    }
}

fn checkpoint_request(index: u16) -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::Schedule,
        session: p::SessionRef(format!("session:m3-b-checkpoint:{index}")),
        agent_profile: p::AgentProfileRef("agent:m3-b".into()),
        input: p::RunInput(format!("advance governed checkpoint {index}")),
        budget: Some(p::Budget("units:20".into())),
        idempotency_key: Some(p::IdempotencyKey(format!("m3-b-checkpoint:{index}"))),
    }
}

fn approval_grant(request: &forme_approval::ApprovalRequest) -> ApprovalGrant {
    ApprovalGrant {
        schema_version: p::SchemaVersion(1),
        approval_id: request.approval_id.clone(),
        outcome: p::ApprovalOutcome::Granted,
        granted_scope: GrantScope::OneShot,
        approver: p::VerifiedPrincipal("owner:m3-b".into()),
        bound_plan_digest: request.plan_digest.clone(),
        policy_version: request.policy_version,
        tool_schema_version: request.tool_schema_version,
        nonce: p::Nonce("nonce:m3-b-outward".into()),
        use_by: request.expires_at.saturating_sub(1),
    }
}

#[test]
fn s58_s61_active_domain_specs_bind_one_run_and_preserve_governance_events() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let control = M3EvolutionHarness::new(store.clone());
    let loop_spec = loop_spec();
    let coordination_spec = coordination_spec();
    let capability_selection = selection_spec(
        p::SelectionTarget::Capability,
        "capability-selection:m3-b:v1",
    );
    let model_selection = selection_spec(p::SelectionTarget::Model, "model-selection:m3-b:v1");
    let backend_selection =
        selection_spec(p::SelectionTarget::Backend, "backend-selection:m3-b:v1");
    let model_spec = model_spec();
    let activations = [
        (
            p::StrategyDomain::Loop,
            loop_spec.version.clone(),
            loop_spec.content_ref.clone(),
            loop_spec.content_digest.clone(),
            "loop",
        ),
        (
            p::StrategyDomain::Coordination,
            coordination_spec.version.clone(),
            coordination_spec.content_ref.clone(),
            coordination_spec.content_digest.clone(),
            "coordination",
        ),
        (
            p::StrategyDomain::CapabilitySelection,
            capability_selection.version.clone(),
            capability_selection.content_ref.clone(),
            capability_selection.content_digest.clone(),
            "capability-selection",
        ),
        (
            p::StrategyDomain::ModelSelection,
            model_selection.version.clone(),
            model_selection.content_ref.clone(),
            model_selection.content_digest.clone(),
            "model-selection",
        ),
        (
            p::StrategyDomain::BackendSelection,
            backend_selection.version.clone(),
            backend_selection.content_ref.clone(),
            backend_selection.content_digest.clone(),
            "backend-selection",
        ),
        (
            p::StrategyDomain::ModelAdaptation,
            model_spec.version.clone(),
            model_spec.content_ref.clone(),
            model_spec.content_digest.clone(),
            "model-adaptation",
        ),
    ];
    let mut active_snapshot = None;
    for (domain, version, content_ref, digest, suffix) in activations {
        active_snapshot = Some(activate(
            &store,
            &control,
            domain,
            version,
            content_ref,
            digest,
            suffix,
        ));
    }
    let domain_runtime = runtime(
        vec![loop_spec],
        coordination_spec,
        vec![capability_selection, model_selection, backend_selection],
        model_spec,
    );
    let governance = GovernanceConfig {
        visible_capabilities: vec![
            p::CapabilityRef("capability:declared".into()),
            p::CapabilityRef("capability:verified".into()),
        ],
        ..GovernanceConfig::default()
    };
    let harness = ReactiveHarness::new(
        store,
        Arc::new(
            ScriptedModelProvider::new(profile(), vec![final_response("governed result")]).unwrap(),
        ),
        Arc::new(ExecutionBackendRegistry::default()),
        governance,
        HarnessConfig::for_model(&profile()),
    )
    .unwrap()
    .with_m3_domain_runtime(domain_runtime)
    .with_coordination(Arc::new(RuleBasedCoordinationReasoner));
    let run = harness.submit_run(request("integrated")).unwrap();
    assert_eq!(
        harness.wait(run.clone()).unwrap().status,
        p::RunStatus::Complete
    );
    let events = harness.stream_events(run).events();
    let kinds = events.iter().map(|event| event.kind).collect::<Vec<_>>();
    assert_subsequence(
        &kinds,
        &[
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::ToolsetResolved,
            p::EventKind::GoalFramed,
            p::EventKind::ResourcePlanned,
            p::EventKind::DecisionTraceRecorded,
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
            p::EventKind::RunComplete,
        ],
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == p::EventKind::VerificationFinished)
            .count(),
        2
    );
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::SessionBound(payload)
            if payload.evolution_snapshot.as_ref() == active_snapshot.as_ref().map(|snapshot| &snapshot.snapshot)
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::DecisionTraceRecorded(payload)
            if payload.evolution_snapshot.is_some()
                && payload.rationale.0.contains("capability:verified")
                && !payload.rationale.0.contains("capability:declared")
    )));
}

#[test]
fn s58_s61_incompatible_active_content_fails_before_session_bound() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let loop_spec = loop_spec();
    let coordination_spec = coordination_spec();
    let selections = vec![selection_spec(
        p::SelectionTarget::Capability,
        "capability-selection:m3-b:v1",
    )];
    let model_spec = model_spec();
    let runtime = runtime(
        vec![loop_spec.clone()],
        coordination_spec,
        selections,
        model_spec,
    );
    let aggregate = p::EvolutionAggregateRef("evolution:owner:workspace-default".into());
    let seed = p::EvolutionSnapshot {
        schema_version: p::SchemaVersion(1),
        snapshot: p::EvolutionSnapshotRef("snapshot:m3-b:incompatible".into()),
        aggregates: vec![p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: aggregate.clone(),
            value: 1,
        }],
        strategies: vec![p::ActiveStrategyRef {
            schema_version: p::SchemaVersion(1),
            id: p::ActiveStrategyId("active:m3-b:incompatible".into()),
            aggregate,
            domain: p::StrategyDomain::Loop,
            scope: loop_spec.scope.clone(),
            version: loop_spec.version.clone(),
            spec_ref: loop_spec.content_ref.clone(),
            spec_digest: p::SchemaDigest("digest:tampered".into()),
            activation_event: p::EventId("event:activation:tampered".into()),
        }],
        digest: p::SchemaDigest("digest:snapshot:m3-b:incompatible".into()),
    };
    let harness = ReactiveHarness::new(
        store.clone(),
        Arc::new(
            ScriptedModelProvider::new(profile(), vec![final_response("must not run")]).unwrap(),
        ),
        Arc::new(ExecutionBackendRegistry::default()),
        GovernanceConfig::default(),
        HarnessConfig::for_model(&profile()),
    )
    .unwrap()
    .with_evolution_seed(seed)
    .unwrap()
    .with_m3_domain_runtime(runtime);
    let run = harness.submit_run(request("incompatible")).unwrap();
    assert_eq!(
        harness.wait(run.clone()).unwrap().status,
        p::RunStatus::Failed
    );
    let events = store.read_run(run).collect::<p::Result<Vec<_>>>().unwrap();
    assert_eq!(events.first().unwrap().kind, p::EventKind::RunAccepted);
    assert!(events
        .iter()
        .any(|event| event.kind == p::EventKind::FailureEvidenceRecorded));
    assert_eq!(events.last().unwrap().kind, p::EventKind::RunFailed);
    assert!(!events
        .iter()
        .any(|event| event.kind == p::EventKind::SessionBound));
}

#[test]
fn s62_long_horizon_checkpoints_yield_to_foreground_pin_versions_and_stop_cleanly() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let control = M3EvolutionHarness::new(store.clone());
    let loop_v1 = loop_spec();
    let mut loop_v2 = loop_v1.clone();
    loop_v2.version = p::StrategyVersionRef("loop:m3-b:v2".into());
    loop_v2.content_ref = p::ContentRef("spec:loop:m3-b:v2".into());
    loop_v2.content_digest = p::SchemaDigest("digest:loop:m3-b:v2".into());
    loop_v2.budget.max_turns = 3;
    let coordination_spec = coordination_spec();
    let capability_selection = selection_spec(
        p::SelectionTarget::Capability,
        "capability-selection:m3-b:v1",
    );
    let model_selection = selection_spec(p::SelectionTarget::Model, "model-selection:m3-b:v1");
    let backend_selection =
        selection_spec(p::SelectionTarget::Backend, "backend-selection:m3-b:v1");
    let model_spec = model_spec();
    for (domain, version, content_ref, digest, suffix) in [
        (
            p::StrategyDomain::Loop,
            loop_v1.version.clone(),
            loop_v1.content_ref.clone(),
            loop_v1.content_digest.clone(),
            "long-loop",
        ),
        (
            p::StrategyDomain::Coordination,
            coordination_spec.version.clone(),
            coordination_spec.content_ref.clone(),
            coordination_spec.content_digest.clone(),
            "long-coordination",
        ),
        (
            p::StrategyDomain::CapabilitySelection,
            capability_selection.version.clone(),
            capability_selection.content_ref.clone(),
            capability_selection.content_digest.clone(),
            "long-capability-selection",
        ),
        (
            p::StrategyDomain::ModelSelection,
            model_selection.version.clone(),
            model_selection.content_ref.clone(),
            model_selection.content_digest.clone(),
            "long-model-selection",
        ),
        (
            p::StrategyDomain::BackendSelection,
            backend_selection.version.clone(),
            backend_selection.content_ref.clone(),
            backend_selection.content_digest.clone(),
            "long-backend-selection",
        ),
        (
            p::StrategyDomain::ModelAdaptation,
            model_spec.version.clone(),
            model_spec.content_ref.clone(),
            model_spec.content_digest.clone(),
            "long-model-adaptation",
        ),
    ] {
        activate(
            &store,
            &control,
            domain,
            version,
            content_ref,
            digest,
            suffix,
        );
    }
    let domain_runtime = runtime(
        vec![loop_v1.clone(), loop_v2.clone()],
        coordination_spec,
        vec![capability_selection, model_selection, backend_selection],
        model_spec,
    );
    let outward_intent = outward_intent();
    let governance = long_horizon_governance(&outward_intent);
    let outward_envelope = governance.envelope.clone().unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let backends = Arc::new(ExecutionBackendRegistry::default());
    backends
        .register(Arc::new(ExternalRecordingBackend {
            executions: executions.clone(),
        }))
        .unwrap();
    let config = HarnessConfig::for_model(&profile());
    let route = long_horizon_route(&config);
    let model = Arc::new(
        ScriptedModelProvider::new(
            profile(),
            vec![
                final_response("checkpoint 0 child"),
                final_response("checkpoint 0 parent"),
                final_response("checkpoint 1 child"),
                final_response("checkpoint 1 parent"),
                final_response("checkpoint 2 child"),
                final_response("checkpoint 2 parent"),
            ],
        )
        .unwrap(),
    );
    let harness = ReactiveHarness::new(store.clone(), model, backends, governance, config)
        .unwrap()
        .with_m3_domain_runtime(domain_runtime);
    let now = now_ms();
    let mut project = LongHorizonProject {
        schema_version: p::SchemaVersion(1),
        project: p::GoalFrameRef("goal-frame:m3-b-long".into()),
        goal: p::LongTermGoal {
            schema_version: p::SchemaVersion(1),
            goal_frame: p::GoalFrameRef("goal-frame:m3-b-long".into()),
            scope: p::Scope("workspace:default".into()),
            situation_digest: p::SchemaDigest("digest:situation:m3-b-long".into()),
            budget: p::Budget("units:3".into()),
            expires_at: now.saturating_add(600_000),
        },
        intention: p::IntentionId("intention:m3-b-long".into()),
        route,
        maximum_checkpoints: 3,
        remaining_checkpoints: 3,
        next_checkpoint: 0,
        deferred_for_foreground: false,
        cancelled: false,
        revoked: false,
        checkpoints: Vec::new(),
    };

    let runs_before_defer = store.run_ids().unwrap().len();
    let deferred = harness
        .advance_long_horizon(&mut project, checkpoint_request(0), true, None)
        .unwrap();
    assert_eq!(deferred.disposition, LongHorizonDisposition::Deferred);
    assert_eq!(store.run_ids().unwrap().len(), runs_before_defer);
    assert_eq!(executions.load(Ordering::SeqCst), 0);

    let checkpoint_0 = harness
        .advance_long_horizon(&mut project, checkpoint_request(0), false, None)
        .unwrap();
    assert_eq!(checkpoint_0.disposition, LongHorizonDisposition::Advanced);
    let checkpoint_0_run = checkpoint_0.checkpoint_run.clone().unwrap();
    let checkpoint_0_events = harness.stream_events(checkpoint_0_run.clone()).events();
    assert_subsequence(
        &checkpoint_0_events
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        &[
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::GoalFramed,
            p::EventKind::OrchestrationRouteCreated,
            p::EventKind::SubagentSpawned,
            p::EventKind::SubagentResultReturned,
            p::EventKind::MemoryNodeAppended,
            p::EventKind::RunWaiting,
            p::EventKind::RunResumed,
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
            p::EventKind::RunComplete,
        ],
    );
    let child_run = checkpoint_0_events
        .iter()
        .find_map(|event| match &event.payload {
            p::EventPayload::SubagentSpawned(payload) => Some(payload.child_run.clone()),
            _ => None,
        })
        .unwrap();
    let child_events = harness.stream_events(child_run).events();
    assert!(child_events.iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::SessionBound(payload)
            if payload.evolution_snapshot == checkpoint_0.snapshot
                && payload.workspace.0.starts_with("workspace:isolated:")
    )));
    assert!(child_events
        .iter()
        .any(|event| event.kind == p::EventKind::RunComplete));

    activate_from_baseline(
        &store,
        &control,
        p::StrategyDomain::Loop,
        loop_v2.version.clone(),
        loop_v2.content_ref.clone(),
        loop_v2.content_digest.clone(),
        loop_v1.version.clone(),
        "long-loop-v2",
    );
    let outward = LongHorizonOutwardStep {
        schema_version: p::SchemaVersion(1),
        request: p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::Schedule,
            session: p::SessionRef("session:m3-b-outward".into()),
            agent_profile: p::AgentProfileRef("agent:m3-b".into()),
            input: p::RunInput("perform one governed outward checkpoint action".into()),
            budget: Some(p::Budget("units:4".into())),
            idempotency_key: Some(p::IdempotencyKey("m3-b-outward:1".into())),
        },
        intent: outward_intent,
        envelope: outward_envelope,
        prelude: Vec::new(),
    };
    let checkpoint_1 = harness
        .advance_long_horizon(&mut project, checkpoint_request(1), false, Some(outward))
        .unwrap();
    assert_eq!(checkpoint_1.disposition, LongHorizonDisposition::Advanced);
    assert_ne!(checkpoint_0.snapshot, checkpoint_1.snapshot);
    let outward_run = checkpoint_1.outward_run.clone().unwrap();
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    let pending = harness
        .pending_approvals(p::SessionId("session:m3-b-outward".into()))
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert!(!harness
        .stream_events(outward_run.clone())
        .any(|event| event.kind == p::EventKind::ActionStarted));
    harness
        .resume(
            outward_run.clone(),
            ResumeInput::Approval(approval_grant(&pending[0])),
        )
        .unwrap();
    assert_eq!(
        harness.wait(outward_run.clone()).unwrap().status,
        p::RunStatus::Complete
    );
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_subsequence(
        &harness
            .stream_events(outward_run)
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        &[
            p::EventKind::ApprovalRequested,
            p::EventKind::ApprovalResolved,
            p::EventKind::CompetenceGateEvaluated,
            p::EventKind::ActionPlanned,
            p::EventKind::ActionStarted,
            p::EventKind::ActionCompleted,
            p::EventKind::VerificationFinished,
        ],
    );

    control
        .rollback(
            p::RunId("run:control:long-loop-rollback".into()),
            p::EvolutionAggregateRef("evolution:owner:workspace-default".into()),
            p::StrategyDomain::Loop,
            p::Scope("workspace:default".into()),
            loop_v1.version.clone(),
            vec![p::EvidenceRef("owner-evidence:long-loop-rollback".into())],
            p::InFlightDisposition::KeepPinned,
            Some(p::OwnerControlRef(
                "owner-control:long-loop-rollback".into(),
            )),
        )
        .unwrap();
    let checkpoint_2 = harness
        .advance_long_horizon(&mut project, checkpoint_request(2), false, None)
        .unwrap();
    assert_eq!(checkpoint_2.disposition, LongHorizonDisposition::Advanced);
    assert_ne!(checkpoint_1.snapshot, checkpoint_2.snapshot);
    assert!(project.checkpoints[0]
        .strategy_versions
        .contains(&loop_v1.version));
    assert!(project.checkpoints[1]
        .strategy_versions
        .contains(&loop_v2.version));
    assert!(project.checkpoints[2]
        .strategy_versions
        .contains(&loop_v1.version));
    for record in &project.checkpoints {
        let events = store
            .read_run(record.run.clone())
            .collect::<p::Result<Vec<_>>>()
            .unwrap();
        assert!(events.iter().any(|event| matches!(
            &event.payload,
            p::EventPayload::OrchestrationRouteCreated(payload)
                if payload.checkpoint.as_ref().is_some_and(|checkpoint| {
                    checkpoint.reference == record.checkpoint
                        && !checkpoint.evidence_refs.is_empty()
                })
        )));
    }

    project.cancel();
    let runs_before_cancelled_advance = store.run_ids().unwrap().len();
    let actions_before_cancelled_advance = executions.load(Ordering::SeqCst);
    let stopped = harness
        .advance_long_horizon(&mut project, checkpoint_request(3), false, None)
        .unwrap();
    assert_eq!(stopped.disposition, LongHorizonDisposition::Cancelled);
    assert_eq!(
        store.run_ids().unwrap().len(),
        runs_before_cancelled_advance
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        actions_before_cancelled_advance
    );

    let artifact = LongHorizonArtifactBody {
        schema_version: p::SchemaVersion(1),
        goal_frame: project.project.clone(),
        scope: project.goal.scope.clone(),
        maximum_checkpoints: project.maximum_checkpoints,
        disposition: LongHorizonArtifactDisposition::Cancelled,
        checkpoints: project
            .checkpoints
            .iter()
            .map(|record| {
                let events = store
                    .read_run(record.run.clone())
                    .collect::<p::Result<Vec<_>>>()
                    .unwrap();
                let outward_events = record
                    .outward_run
                    .as_ref()
                    .map(|run| {
                        store
                            .read_run(run.clone())
                            .collect::<p::Result<Vec<_>>>()
                            .unwrap()
                    })
                    .unwrap_or_default();
                LongHorizonArtifactCheckpoint {
                    schema_version: record.schema_version,
                    index: record.index,
                    run: record.run.clone(),
                    checkpoint: record.checkpoint.clone(),
                    snapshot: record.snapshot.clone(),
                    strategy_versions: record.strategy_versions.clone(),
                    status: record.status,
                    outward_run: record.outward_run.clone(),
                    outward_event_refs: outward_events
                        .iter()
                        .map(|event| event.event_id.clone())
                        .collect(),
                    outward_event_kinds: outward_events.iter().map(|event| event.kind).collect(),
                    event_refs: events.iter().map(|event| event.event_id.clone()).collect(),
                    event_kinds: events.iter().map(|event| event.kind).collect(),
                }
            })
            .collect(),
    };
    let configured_root = std::env::var_os("FORME_M3_B_ARTIFACT_DIR");
    let artifact_root = configured_root.as_ref().map_or_else(
        || {
            std::env::temp_dir().join(format!(
                "forme-m3-b-long-horizon-{}-{}",
                std::process::id(),
                now_ms()
            ))
        },
        std::path::PathBuf::from,
    );
    let artifact_store = M3BArtifactStore::new(&artifact_root).unwrap();
    let written = artifact_store.write(&artifact).unwrap();
    let verified = artifact_store.verify_complete_set().unwrap();
    assert_eq!(written.digest, verified.digest);
    println!(
        "M3-B long-horizon artifact {} {}",
        verified.digest.0,
        verified.path.display()
    );
    if configured_root.is_none() {
        std::fs::remove_dir_all(artifact_root).unwrap();
    }
}
