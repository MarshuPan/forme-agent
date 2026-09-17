use forme_models::{
    adapt_model_scaffold, Cost, InMemoryModelAdaptationRegistry, ModelAdaptationRegistry,
    ModelCapability, ModelOutcomeEvidence, ModelProfile, ModelStrength, RateLimit, Url,
};
use forme_protocol as p;

fn profile(strength: ModelStrength) -> ModelProfile {
    ModelProfile {
        schema_version: p::SchemaVersion(1),
        provider: p::ProviderId("provider:m3-b".into()),
        model: "model-m3-b".into(),
        base_url: Url::parse("https://models.invalid/v1").unwrap(),
        capability: ModelCapability {
            schema_version: p::SchemaVersion(1),
            context_window: 32_000,
            tool_use: true,
            strength,
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
        credential_ref: p::CredentialRef("secret-ref:m3-b".into()),
    }
}

fn spec(
    version: &str,
    minimum_strength: p::ModelStrengthBand,
    steps: u16,
    verification_passes: u16,
) -> p::ModelAdaptationSpec {
    p::ModelAdaptationSpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef(version.into()),
        scope: p::Scope("workspace:m3-b".into()),
        content_ref: p::ContentRef(format!("spec:{version}")),
        content_digest: p::SchemaDigest(format!("digest:{version}")),
        compatibility: p::StrategyRuntimeCompatibility {
            schema_version: p::SchemaVersion(1),
            minimum_runtime_schema: p::SchemaVersion(1),
            event_schema: p::SchemaVersion(1),
            model_profile: None,
            tool_schema: None,
            backend_schema: None,
        },
        predicate: p::ModelCapabilityPredicate {
            schema_version: p::SchemaVersion(1),
            minimum_context_window: 8_192,
            requires_tool_use: true,
            minimum_strength,
        },
        scaffold: p::ModelScaffoldProfile {
            schema_version: p::SchemaVersion(1),
            externalized_steps: steps,
            verification_passes,
            checkpoint_cadence_steps: 1,
        },
    }
}

fn evidence(profile: &ModelProfile, measured: p::ModelStrengthBand) -> ModelOutcomeEvidence {
    ModelOutcomeEvidence {
        schema_version: p::SchemaVersion(1),
        profile: profile.profile_ref(),
        measured_strength: measured,
        verified_outcomes: 5,
        failures: 0,
        evidence_refs: vec![p::EvidenceRef("evidence:model:m3-b".into())],
    }
}

#[test]
fn s61_weak_and_strong_profiles_change_scaffolding_but_not_governance() {
    let weak = profile(ModelStrength::Basic);
    let strong = profile(ModelStrength::Strong);
    let weak_plan = adapt_model_scaffold(
        &spec("adaptation:weak:v1", p::ModelStrengthBand::Basic, 4, 3),
        &weak,
        &evidence(&weak, p::ModelStrengthBand::Basic),
        true,
    )
    .unwrap();
    let strong_plan = adapt_model_scaffold(
        &spec("adaptation:strong:v1", p::ModelStrengthBand::Strong, 1, 1),
        &strong,
        &evidence(&strong, p::ModelStrengthBand::Strong),
        true,
    )
    .unwrap();
    assert!(weak_plan.scaffold.externalized_steps > strong_plan.scaffold.externalized_steps);
    assert!(weak_plan.scaffold.verification_passes > strong_plan.scaffold.verification_passes);
    for plan in [weak_plan, strong_plan] {
        assert!(plan.verification_required);
        assert!(plan.high_impact_trace_required);
        assert!(plan.high_impact_approval_required);
    }
}

#[test]
fn s61_provider_strength_cannot_replace_measured_outcome_evidence() {
    let strong = profile(ModelStrength::Strong);
    let strong_spec = spec("adaptation:strong:v1", p::ModelStrengthBand::Strong, 1, 1);
    let measured_basic = evidence(&strong, p::ModelStrengthBand::Basic);
    assert!(adapt_model_scaffold(&strong_spec, &strong, &measured_basic, true).is_err());
    let mut missing = evidence(&strong, p::ModelStrengthBand::Strong);
    missing.verified_outcomes = 0;
    assert!(adapt_model_scaffold(&strong_spec, &strong, &missing, true).is_err());
}

#[test]
fn s61_model_adaptation_registry_rejects_same_version_different_digest() {
    let seed = spec("adaptation:seed:v1", p::ModelStrengthBand::Basic, 4, 2);
    let registry = InMemoryModelAdaptationRegistry::with_seed(seed.clone()).unwrap();
    assert_eq!(registry.resolve(&seed.version).unwrap(), seed);
    let mut rebound = seed.clone();
    rebound.content_digest = p::SchemaDigest("digest:rebound".into());
    assert!(registry.register(rebound).is_err());
}
