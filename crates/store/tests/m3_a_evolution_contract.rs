use forme_protocol as p;
use forme_store::{
    EventStore, EvolutionEventStore, EvolutionProjection, SqliteEventStore, StoreOptions,
};

fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn owner_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::UserTurn,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}

fn untrusted_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Communication,
        actor: p::Actor::External(p::ParticipantId("participant:untrusted".into())),
        trust_tier: p::TrustTier::Untrusted,
        caused_by: None,
    }
}

fn spoofed_verified_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Communication,
        actor: p::Actor::External(p::ParticipantId("participant:spoofed".into())),
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

fn evolution_version(value: u64) -> p::EvolutionAggregateVersion {
    p::EvolutionAggregateVersion {
        schema_version: p::SchemaVersion(1),
        aggregate: p::EvolutionAggregateRef("evolution:owner:workspace".into()),
        value,
    }
}

fn candidate(number: u8, baseline: &str) -> p::StrategyCandidate {
    p::StrategyCandidate {
        schema_version: p::SchemaVersion(1),
        candidate: p::CandidateId(format!("candidate:loop:v{number}")),
        domain: p::StrategyDomain::Loop,
        scope: p::Scope("workspace".into()),
        target_tier: p::StabilityTier::Stable,
        proposed_version: p::StrategyVersionRef(format!("loop:v{number}")),
        baseline: p::StrategyVersionRef(baseline.into()),
        spec_ref: p::ContentRef(format!("content:loop:v{number}")),
        spec_digest: p::SchemaDigest(format!("digest:loop:v{number}")),
        evidence: vec![p::EvidenceRef(format!("evidence:loop:v{number}"))],
        provenance: provenance(),
        impact: p::EvolutionImpact::Cautious,
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: p::StrategyVersionRef(baseline.into()),
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    }
}

fn append_stable_strategy(store: &SqliteEventStore, number: u8, baseline: &str) {
    let candidate = candidate(number, baseline);
    store
        .append(event(
            &format!("event:candidate:v{number}"),
            &format!("run:candidate:v{number}"),
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: candidate.candidate.clone(),
                target: p::CandidateTargetRef(format!("strategy:loop:v{number}")),
                evidence_refs: candidate.evidence.clone(),
                confidence: p::Confidence(0.9),
                provenance: provenance(),
                target_tier: p::StabilityTier::Stable,
                capability_update: None,
                strategy_candidate: Some(candidate.clone()),
            }),
        ))
        .unwrap();
    store
        .append(event(
            &format!("event:evaluation:v{number}"),
            &format!("run:evaluation:v{number}"),
            p::EventPayload::EvolutionEvaluationRecorded(p::EvolutionEvaluationRecordedPayload {
                evaluation: p::EvolutionEvaluationRef(format!("evaluation:loop:v{number}")),
                baseline: candidate.baseline.clone(),
                candidate: candidate.proposed_version.clone(),
                verdict: p::EvaluationVerdict::Pass,
                hard_invariants: vec![p::InvariantResultRef("invariant:harness-first".into())],
                ground_truth: vec![p::EvidenceRef(format!("ground-truth:loop:v{number}"))],
            }),
        ))
        .unwrap();
    store
        .append(event(
            &format!("event:promotion:v{number}"),
            &format!("run:promotion:v{number}"),
            p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                candidate_id: candidate.candidate,
                by: p::DecisionActor::Auto,
                reason: p::ReasonRef("ground-truth-pass".into()),
            }),
        ))
        .unwrap();
}

fn activation(number: u8, from: Option<u8>, expected: u64, event_id: &str) -> p::Event {
    event(
        event_id,
        &format!("run:activation:{event_id}"),
        p::EventPayload::StrategyActivated(p::StrategyActivatedPayload {
            activation: p::StrategyActivation {
                schema_version: p::SchemaVersion(1),
                aggregate: p::EvolutionAggregateRef("evolution:owner:workspace".into()),
                domain: p::StrategyDomain::Loop,
                scope: p::Scope("workspace".into()),
                from: from.map(|value| p::StrategyVersionRef(format!("loop:v{value}"))),
                to: p::StrategyVersionRef(format!("loop:v{number}")),
                spec_ref: p::ContentRef(format!("content:loop:v{number}")),
                spec_digest: p::SchemaDigest(format!("digest:loop:v{number}")),
                evaluation: p::EvolutionEvaluationRef(format!("evaluation:loop:v{number}")),
                promotion: p::EventId(format!("event:promotion:v{number}")),
                owner_confirmation: None,
                impact: p::EvolutionImpact::Cautious,
                expected_version: evolution_version(expected),
                committed_version: evolution_version(expected + 1),
            },
            active_snapshot: p::EvolutionSnapshotRef("pending-preview".into()),
        }),
    )
}

fn rollback(event_id: &str, expected: u64) -> p::Event {
    event(
        event_id,
        &format!("run:rollback:{event_id}"),
        p::EventPayload::StrategyRolledBack(p::StrategyRolledBackPayload {
            rollback: p::StrategyRollback {
                schema_version: p::SchemaVersion(1),
                aggregate: p::EvolutionAggregateRef("evolution:owner:workspace".into()),
                domain: p::StrategyDomain::Loop,
                scope: p::Scope("workspace".into()),
                failed: p::StrategyVersionRef("loop:v2".into()),
                restored: p::StrategyVersionRef("loop:v1".into()),
                restored_spec_ref: p::ContentRef("content:loop:v1".into()),
                restored_spec_digest: p::SchemaDigest("digest:loop:v1".into()),
                triggers: vec![p::EvidenceRef("evidence:regression:v2".into())],
                expected_version: evolution_version(expected),
                committed_version: evolution_version(expected + 1),
                in_flight: p::InFlightDisposition::KeepPinned,
                external_effects_reverted: p::HistoricalFalse,
            },
            active_snapshot: p::EvolutionSnapshotRef("pending-preview".into()),
        }),
    )
}

fn attach_preview(store: &SqliteEventStore, event: &mut p::Event) -> p::EvolutionSnapshot {
    let snapshot = store.preview_evolution_snapshot(event).unwrap();
    match &mut event.payload {
        p::EventPayload::StrategyActivated(payload) => {
            payload.active_snapshot = snapshot.snapshot.clone();
        }
        p::EventPayload::StrategyRolledBack(payload) => {
            payload.active_snapshot = snapshot.snapshot.clone();
        }
        _ => unreachable!(),
    }
    snapshot
}

#[test]
fn s56_strategy_promotion_requires_a_recorded_passing_evaluation() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();

    let untrusted = untrusted_provenance();
    let mut untrusted_candidate = candidate(9, "loop:v0");
    untrusted_candidate.provenance = untrusted.clone();
    let mut untrusted_candidate_event = event(
        "event:candidate:untrusted",
        "run:candidate:untrusted",
        p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
            candidate_id: untrusted_candidate.candidate.clone(),
            target: p::CandidateTargetRef("strategy:loop:v9".into()),
            evidence_refs: untrusted_candidate.evidence.clone(),
            confidence: p::Confidence(0.9),
            provenance: untrusted.clone(),
            target_tier: p::StabilityTier::Stable,
            capability_update: None,
            strategy_candidate: Some(untrusted_candidate),
        }),
    );
    untrusted_candidate_event.provenance = untrusted.clone();
    assert!(store.append(untrusted_candidate_event).is_err());
    assert!(store
        .read_run(p::RunId("run:candidate:untrusted".into()))
        .next()
        .is_none());

    let mut untrusted_evaluation = event(
        "event:evaluation:untrusted",
        "run:evaluation:untrusted",
        p::EventPayload::EvolutionEvaluationRecorded(p::EvolutionEvaluationRecordedPayload {
            evaluation: p::EvolutionEvaluationRef("evaluation:loop:untrusted".into()),
            baseline: p::StrategyVersionRef("loop:v0".into()),
            candidate: p::StrategyVersionRef("loop:v9".into()),
            verdict: p::EvaluationVerdict::Pass,
            hard_invariants: vec![p::InvariantResultRef("invariant:harness-first".into())],
            ground_truth: vec![p::EvidenceRef("ground-truth:untrusted".into())],
        }),
    );
    untrusted_evaluation.provenance = untrusted;
    assert!(store.append(untrusted_evaluation).is_err());
    assert!(store
        .read_run(p::RunId("run:evaluation:untrusted".into()))
        .next()
        .is_none());

    let primary = candidate(1, "loop:v0");
    store
        .append(event(
            "event:candidate:promotion-gate",
            "run:candidate:promotion-gate",
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: primary.candidate.clone(),
                target: p::CandidateTargetRef("strategy:loop:v1".into()),
                evidence_refs: primary.evidence.clone(),
                confidence: p::Confidence(0.9),
                provenance: provenance(),
                target_tier: p::StabilityTier::Stable,
                capability_update: None,
                strategy_candidate: Some(primary.clone()),
            }),
        ))
        .unwrap();

    let mut conflicting_version = candidate(2, "loop:v0");
    conflicting_version.proposed_version = primary.proposed_version.clone();
    let conflicting_run = p::RunId("run:candidate:conflicting-version".into());
    assert!(store
        .append(event(
            "event:candidate:conflicting-version",
            &conflicting_run.0,
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: conflicting_version.candidate.clone(),
                target: p::CandidateTargetRef("strategy:loop:v1:conflict".into()),
                evidence_refs: conflicting_version.evidence.clone(),
                confidence: p::Confidence(0.9),
                provenance: provenance(),
                target_tier: p::StabilityTier::Stable,
                capability_update: None,
                strategy_candidate: Some(conflicting_version),
            }),
        ))
        .is_err());
    assert!(store.read_run(conflicting_run).next().is_none());

    let promotion = |id: &str, run: &str, by: p::DecisionActor, provenance| {
        let mut event = event(
            id,
            run,
            p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                candidate_id: primary.candidate.clone(),
                by,
                reason: p::ReasonRef("claimed-pass".into()),
            }),
        );
        event.provenance = provenance;
        event
    };
    assert!(store
        .append(promotion(
            "event:promotion:missing-eval",
            "run:promotion:missing-eval",
            p::DecisionActor::Auto,
            provenance(),
        ))
        .is_err());
    assert!(store
        .read_run(p::RunId("run:promotion:missing-eval".into()))
        .next()
        .is_none());

    store
        .append(event(
            "event:evaluation:failed",
            "run:evaluation:failed",
            p::EventPayload::EvolutionEvaluationRecorded(p::EvolutionEvaluationRecordedPayload {
                evaluation: p::EvolutionEvaluationRef("evaluation:loop:v1:failed".into()),
                baseline: primary.baseline.clone(),
                candidate: primary.proposed_version.clone(),
                verdict: p::EvaluationVerdict::Fail,
                hard_invariants: vec![p::InvariantResultRef("invariant:harness-first".into())],
                ground_truth: vec![p::EvidenceRef("ground-truth:failed".into())],
            }),
        ))
        .unwrap();
    assert!(store
        .append(promotion(
            "event:promotion:failed-eval",
            "run:promotion:failed-eval",
            p::DecisionActor::Auto,
            provenance(),
        ))
        .is_err());

    store
        .append(event(
            "event:evaluation:passed",
            "run:evaluation:passed",
            p::EventPayload::EvolutionEvaluationRecorded(p::EvolutionEvaluationRecordedPayload {
                evaluation: p::EvolutionEvaluationRef("evaluation:loop:v1:passed".into()),
                baseline: primary.baseline.clone(),
                candidate: primary.proposed_version.clone(),
                verdict: p::EvaluationVerdict::Pass,
                hard_invariants: vec![p::InvariantResultRef("invariant:harness-first".into())],
                ground_truth: vec![p::EvidenceRef("ground-truth:passed".into())],
            }),
        ))
        .unwrap();
    assert!(store
        .append(promotion(
            "event:promotion:user-with-system-provenance",
            "run:promotion:user-with-system-provenance",
            p::DecisionActor::User,
            provenance(),
        ))
        .is_err());
    assert!(store
        .read_run(p::RunId("run:promotion:user-with-system-provenance".into()))
        .next()
        .is_none());
    assert!(store
        .append(promotion(
            "event:promotion:auto-with-owner-provenance",
            "run:promotion:auto-with-owner-provenance",
            p::DecisionActor::Auto,
            owner_provenance(),
        ))
        .is_err());
    assert!(store
        .read_run(p::RunId("run:promotion:auto-with-owner-provenance".into()))
        .next()
        .is_none());
    store
        .append(promotion(
            "event:promotion:passed-eval",
            "run:promotion:passed-eval",
            p::DecisionActor::Auto,
            provenance(),
        ))
        .unwrap();
    assert!(store
        .stable_strategy(
            p::StrategyDomain::Loop,
            &primary.scope,
            &primary.proposed_version,
        )
        .unwrap()
        .is_some());
}

#[test]
fn s57_promotion_activation_snapshot_cas_and_rollback_remain_separate() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let aggregate = p::EvolutionAggregateRef("evolution:owner:workspace".into());
    let scope = p::Scope("workspace".into());

    append_stable_strategy(&store, 1, "loop:v0");
    assert!(store
        .stable_strategy(
            p::StrategyDomain::Loop,
            &scope,
            &p::StrategyVersionRef("loop:v1".into())
        )
        .unwrap()
        .is_some());
    assert!(store
        .active_for(&aggregate, p::StrategyDomain::Loop, &scope)
        .unwrap()
        .is_none());
    assert_eq!(store.evolution_version(&aggregate).unwrap().value, 0);

    let mut activate_v1 = activation(1, None, 0, "event:activation:v1");
    let pinned_v1 = attach_preview(&store, &mut activate_v1);
    let applied = store
        .append_evolution_expected(activate_v1.clone(), &aggregate, evolution_version(0))
        .unwrap();
    assert_eq!(applied.status, p::ExpectedAppendStatus::Applied);
    assert_eq!(applied.resulting_version, 1);
    assert_eq!(
        store
            .active_for(&aggregate, p::StrategyDomain::Loop, &scope)
            .unwrap()
            .unwrap()
            .version,
        p::StrategyVersionRef("loop:v1".into())
    );

    let duplicate = store
        .append_evolution_expected(activate_v1, &aggregate, evolution_version(0))
        .unwrap();
    assert_eq!(duplicate.status, p::ExpectedAppendStatus::Duplicate);
    assert_eq!(duplicate.resulting_version, 1);

    append_stable_strategy(&store, 2, "loop:v1");
    let stale = activation(2, Some(1), 1, "event:activation:stale");
    let mut activate_v2 = activation(2, Some(1), 1, "event:activation:v2");
    let pinned_v2 = attach_preview(&store, &mut activate_v2);
    store
        .append_evolution_expected(activate_v2, &aggregate, evolution_version(1))
        .unwrap();
    assert_ne!(pinned_v1.snapshot, pinned_v2.snapshot);
    assert_eq!(pinned_v1.strategies[0].version.0, "loop:v1");
    assert_eq!(pinned_v2.strategies[0].version.0, "loop:v2");

    let conflict = store
        .append_evolution_expected(stale, &aggregate, evolution_version(1))
        .unwrap();
    assert_eq!(conflict.status, p::ExpectedAppendStatus::Conflict);
    assert!(conflict.event_id.is_none());
    assert!(store
        .read_run(p::RunId("run:activation:event:activation:stale".into()))
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .is_empty());

    let mut rollback_event = rollback("event:rollback:v2", 2);
    let mut spoofed_rollback = rollback("event:rollback:spoofed", 2);
    attach_preview(&store, &mut spoofed_rollback);
    spoofed_rollback.provenance = spoofed_verified_provenance();
    assert!(store
        .append_evolution_expected(spoofed_rollback, &aggregate, evolution_version(2))
        .is_err());
    assert_eq!(store.evolution_version(&aggregate).unwrap().value, 2);
    assert!(store
        .read_run(p::RunId("run:rollback:event:rollback:spoofed".into()))
        .next()
        .is_none());

    let restored = attach_preview(&store, &mut rollback_event);
    store
        .append_evolution_expected(rollback_event, &aggregate, evolution_version(2))
        .unwrap();
    assert_eq!(restored.strategies[0].version.0, "loop:v1");
    assert_eq!(store.evolution_version(&aggregate).unwrap().value, 3);
    let history_before = store.evolution_history(&aggregate).unwrap();
    assert_eq!(history_before.len(), 3);
    assert_eq!(history_before[2].kind, p::EventKind::StrategyRolledBack);

    let final_snapshot = store.snapshot(scope.clone()).unwrap();
    store.rebuild_evolution_projection().unwrap();
    assert_eq!(store.snapshot(scope).unwrap(), final_snapshot);
    assert_eq!(store.evolution_history(&aggregate).unwrap(), history_before);
}

#[test]
fn s57_active_control_cannot_bypass_cas_or_owner_impact_gate() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let aggregate = p::EvolutionAggregateRef("evolution:owner:workspace".into());
    append_stable_strategy(&store, 1, "loop:v0");
    let mut control = activation(1, None, 0, "event:activation:direct");
    attach_preview(&store, &mut control);
    assert!(store.append(control.clone()).is_err());
    assert_eq!(store.evolution_version(&aggregate).unwrap().value, 0);

    let mut spoofed = activation(1, None, 0, "event:activation:spoofed");
    attach_preview(&store, &mut spoofed);
    spoofed.provenance = spoofed_verified_provenance();
    assert!(store
        .append_evolution_expected(spoofed, &aggregate, evolution_version(0))
        .is_err());
    assert_eq!(store.evolution_version(&aggregate).unwrap().value, 0);
    assert!(store
        .read_run(p::RunId("run:activation:event:activation:spoofed".into()))
        .next()
        .is_none());

    let p::EventPayload::StrategyActivated(payload) = &mut control.payload else {
        unreachable!()
    };
    payload.activation.impact = p::EvolutionImpact::Expansive;
    payload.activation.owner_confirmation = Some(p::OwnerControlRef("owner-control:1".into()));
    assert!(store
        .append_evolution_expected(control, &aggregate, evolution_version(0))
        .is_err());
    assert_eq!(store.evolution_version(&aggregate).unwrap().value, 0);
}

#[test]
fn s57_legacy_store_has_no_synthetic_active_history() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let aggregate = p::EvolutionAggregateRef("evolution:owner:workspace".into());
    let scope = p::Scope("workspace".into());
    store
        .append(event(
            "event:legacy",
            "run:legacy",
            p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 0 }),
        ))
        .unwrap();
    assert_eq!(store.evolution_version(&aggregate).unwrap().value, 0);
    assert!(store.evolution_history(&aggregate).unwrap().is_empty());
    assert!(store.snapshot(scope).is_err());
    store.rebuild_evolution_projection().unwrap();
    assert_eq!(store.evolution_version(&aggregate).unwrap().value, 0);
    assert!(store.evolution_history(&aggregate).unwrap().is_empty());
}
