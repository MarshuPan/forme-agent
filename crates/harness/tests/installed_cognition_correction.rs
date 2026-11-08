#![forbid(unsafe_code)]
#[path = "support/core_brain.rs"]
mod support;
use forme_cognition::*;
use forme_harness::{DataOperationConfirmation, ProductionRuntime};
use forme_protocol as p;
use forme_store::EventStore;
use std::sync::atomic::Ordering;
use support::{events, now, Fixture};

const TEXT: &str =
    "owner-feedback-874ac9 不应把我临时的兴趣当成长期偏好。<script>不要执行</script>";
fn query() -> p::WorkbenchQuery {
    p::WorkbenchQuery {
        schema_version: p::SchemaVersion(1),
        section: p::WorkbenchSection::Cognition,
        scope: None,
        offset: 0,
        limit: 100,
        snapshot: None,
    }
}
fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::OwnerControl,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}
fn seed(runtime: &ProductionRuntime, run: &str, tier: p::StabilityTier) -> p::CandidateId {
    seed_in_scope(runtime, run, tier, "workspace:core-b")
}
fn seed_in_scope(
    runtime: &ProductionRuntime,
    run: &str,
    tier: p::StabilityTier,
    scope: &str,
) -> p::CandidateId {
    let candidate = p::CandidateId(format!("candidate:{run}"));
    runtime
        .services()
        .store
        .append(p::Event::new(
            p::EventId(format!("event:{run}")),
            p::RunId(run.into()),
            None,
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: candidate.clone(),
                target: p::CandidateTargetRef(format!(
                    "cognitive-map|{scope}|frame|a previous judgment"
                )),
                evidence_refs: vec![p::EvidenceRef("evidence:source".into())],
                confidence: p::Confidence(0.4),
                provenance: provenance(),
                target_tier: tier,
                capability_update: None,
                strategy_candidate: None,
            }),
            p::SchemaVersion(1),
            now(),
            provenance(),
        ))
        .unwrap();
    candidate
}
fn command(
    runtime: &ProductionRuntime,
    f: &Fixture,
    id: &str,
    kind: p::WorkbenchCorrectionKind,
    key: &str,
) -> p::WorkbenchCorrectionCommand {
    let view = runtime.workbench_snapshot(&f.owner(), query()).unwrap();
    let row = view
        .items
        .iter()
        .find(|v| {
            v.id == id
                && v.kind
                    == match kind {
                        p::WorkbenchCorrectionKind::Candidate => p::WorkbenchItemKind::Candidate,
                        p::WorkbenchCorrectionKind::Stable => p::WorkbenchItemKind::Stable,
                    }
        })
        .unwrap();
    p::WorkbenchCorrectionCommand {
        schema_version: p::SchemaVersion(1),
        command: p::IdempotencyKey(key.into()),
        run: row.run.clone(),
        kind,
        id: row.id.clone(),
        event: row.event.clone(),
        snapshot: view.digest,
        statement: TEXT.into(),
    }
}
fn receipts(runtime: &ProductionRuntime, f: &Fixture) -> p::WorkbenchCorrectionsView {
    runtime
        .workbench_corrections(
            &f.owner(),
            p::WorkbenchCorrectionQuery {
                schema_version: p::SchemaVersion(1),
                command: None,
            },
        )
        .unwrap()
}
fn promote(runtime: &ProductionRuntime, object: &str) {
    let c = &runtime.services().cognition;
    let scope = MapScope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:core-b".into()),
    };
    let evidence = vec![GovernanceEvidence {
        schema_version: p::SchemaVersion(1),
        reference: p::EvidenceRef(format!("verified:{object}")),
        observed_at: now(),
        verified_process: true,
    }];
    let id = c
        .propose_update(CognitiveMapUpdateProposal {
            schema_version: p::SchemaVersion(1),
            candidate_id: None,
            scope: scope.clone(),
            kind: MapUpdateKind::Frame,
            confidence: p::Confidence(0.4),
            evidence: vec![evidence[0].reference.clone()],
            frame: Some(p::JudgmentFrameRef(object.into())),
            quality: None,
            blindspot: None,
            resource: None,
            reflection_inputs: vec![],
            statement: format!("Previously checked caution about {object}"),
            provenance: provenance(),
        })
        .unwrap();
    assert_eq!(
        c.govern_and_apply(GovernanceCandidate {
            schema_version: p::SchemaVersion(1),
            candidate_id: id,
            direction: ChangeDirection::CautionIncreasing,
            confidence: p::Confidence(0.9),
            evidence: evidence.clone(),
            impact: p::Impact::Low,
            provenance: provenance(),
            conflicts: vec![],
            owner_confirmed: true,
            target: StableCognition {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef(object.into()),
                scope,
                kind: StableCognitionKind::Frame,
                statement: format!("Previously checked caution about {object}"),
                tier: p::StabilityTier::Stable,
                confidence: p::Confidence(0.9),
                evidence,
                last_reproduced_at: now(),
                active: false
            }
        })
        .unwrap(),
        GovernanceDecision::Promote
    );
}

#[test]
fn owner_feedback_atomically_downgrades_a_different_stream_and_stays_a_candidate_after_restart() {
    let f = Fixture::new();
    let runtime = f.build();
    let old = seed(&runtime, "run:separate", p::StabilityTier::Working);
    let command = command(
        &runtime,
        &f,
        &old.0,
        p::WorkbenchCorrectionKind::Candidate,
        "correction:one",
    );
    let receipt = runtime
        .correct_workbench_cognition(&f.owner(), command.clone())
        .unwrap();
    assert_eq!(receipt.status, p::WorkbenchCommandStatus::Confirmed);
    let recorded = events(&runtime);
    assert!(recorded.iter().any(
        |e| matches!(&e.payload, p::EventPayload::CandidateDowngraded(v) if v.candidate_id == old)
    ));
    let new = recorded
        .iter()
        .find_map(|e| match &e.payload {
            p::EventPayload::CandidateCreated(v) if v.candidate_id == receipt.candidate => Some(v),
            _ => None,
        })
        .unwrap();
    assert_eq!(new.target_tier, p::StabilityTier::Working);
    assert_eq!(new.confidence, p::Confidence(0.4));
    assert!(new.target.0.contains(TEXT));
    assert!(new.capability_update.is_none());
    assert!(new.strategy_candidate.is_none());
    assert!(recorded.iter().any(|e| matches!(&e.payload, p::EventPayload::CandidateConflictDetected(v) if v.candidate_id == receipt.candidate && v.conflict_with == old)));
    assert!(runtime.services().cognition.stable_objects().is_empty());
    let heads = runtime.services().store.run_heads().unwrap();
    assert_eq!(
        runtime
            .correct_workbench_cognition(&f.owner(), command.clone())
            .unwrap(),
        receipt
    );
    let mut other = command.clone();
    other.statement.push('!');
    assert!(runtime
        .correct_workbench_cognition(&f.owner(), other)
        .is_err());
    assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    assert_eq!(receipts(&runtime, &f).receipts, vec![receipt.clone()]);
    let encoded = serde_json::to_string(&receipts(&runtime, &f)).unwrap();
    assert!(!encoded.contains(TEXT));
    for path in [
        f.config.store_path.clone(),
        f.config.store_path.with_extension("db-wal"),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            assert!(!bytes.windows(TEXT.len()).any(|v| v == TEXT.as_bytes()));
        }
    }
    drop(runtime);
    let runtime = f.build();
    assert_eq!(
        runtime
            .correct_workbench_cognition(&f.owner(), command)
            .unwrap(),
        receipt
    );
    assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn stable_correction_invalidates_transitive_derivatives_and_erasing_feedback_never_revives_them() {
    let f = Fixture::new();
    let runtime = f.build();
    for object in ["frame:root", "frame:child", "frame:grandchild"] {
        promote(&runtime, object);
    }
    let cognition = runtime.services().cognition.clone();
    cognition
        .record_lineage(
            p::ObjectRef("frame:root".into()),
            p::ObjectRef("frame:child".into()),
        )
        .unwrap();
    cognition
        .record_lineage(
            p::ObjectRef("frame:child".into()),
            p::ObjectRef("frame:grandchild".into()),
        )
        .unwrap();
    let command = command(
        &runtime,
        &f,
        "frame:root",
        p::WorkbenchCorrectionKind::Stable,
        "correction:stable",
    );
    let receipt = runtime
        .correct_workbench_cognition(&f.owner(), command.clone())
        .unwrap();
    for object in ["frame:root", "frame:child", "frame:grandchild"] {
        assert!(!cognition.is_object_usable(&p::ObjectRef(object.into())));
    }
    let memory = runtime
        .workbench_snapshot(
            &f.owner(),
            p::WorkbenchQuery {
                section: p::WorkbenchSection::Memory,
                ..query()
            },
        )
        .unwrap();
    let row = memory
        .items
        .iter()
        .find(|v| v.id.starts_with("owner-correction-memory:"))
        .unwrap();
    let material_query = p::WorkbenchMaterialQuery {
        schema_version: p::SchemaVersion(1),
        node: p::NodeId(row.id.clone()),
        run: row.run.clone(),
        event: row.event.clone(),
        scope: None,
        snapshot: memory.digest,
    };
    assert_eq!(
        runtime
            .workbench_material(&f.owner(), material_query.clone())
            .unwrap()
            .text
            .as_deref(),
        Some(TEXT)
    );
    let target = runtime
        .workbench_material_lifecycle_target(&f.owner(), material_query)
        .unwrap();
    let plan = runtime
        .prepare_data_lifecycle(
            &f.owner(),
            target.scope,
            target.subject,
            p::DataLifecycleOperation::Delete,
            target.lifecycle_version,
        )
        .unwrap();
    runtime
        .apply_data_operation(
            &f.owner(),
            plan.clone(),
            DataOperationConfirmation {
                schema_version: p::SchemaVersion(1),
                plan_digest: plan.digest,
                nonce: plan.nonce,
                use_by: plan.expires_at,
                approved: true,
            },
        )
        .unwrap();
    assert!(runtime
        .services()
        .store
        .search("owner-feedback-874ac9", 100)
        .unwrap()
        .is_empty());
    let remaining = events(&runtime);
    assert!(!remaining.iter().any(|e| matches!(&e.payload,p::EventPayload::CandidateCreated(v) if v.candidate_id == receipt.candidate)));
    assert_eq!(receipts(&runtime, &f).receipts, vec![receipt.clone()]);
    drop(cognition);
    drop(runtime);
    let runtime = f.build();
    for object in ["frame:root", "frame:child", "frame:grandchild"] {
        assert!(!runtime
            .services()
            .cognition
            .is_object_usable(&p::ObjectRef(object.into())));
    }
    assert_eq!(
        runtime
            .correct_workbench_cognition(&f.owner(), command)
            .unwrap(),
        receipt
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn feedback_deletion_preview_covers_a_nondefault_workspace() {
    let f = Fixture::new();
    let runtime = f.build();
    let untouched = seed(&runtime, "run:untouched-default", p::StabilityTier::Working);
    let scope = "workspace:another-project";
    let old = seed_in_scope(
        &runtime,
        "run:another-project",
        p::StabilityTier::Working,
        scope,
    );
    let correction = command(
        &runtime,
        &f,
        &old.0,
        p::WorkbenchCorrectionKind::Candidate,
        "correction:another-project",
    );
    let corrected = runtime
        .correct_workbench_cognition(&f.owner(), correction)
        .unwrap();
    let memory = runtime
        .workbench_snapshot(
            &f.owner(),
            p::WorkbenchQuery {
                section: p::WorkbenchSection::Memory,
                ..query()
            },
        )
        .unwrap();
    let row = memory
        .items
        .iter()
        .find(|item| item.id.starts_with("owner-correction-memory:"))
        .unwrap();
    let target = runtime
        .workbench_material_lifecycle_target(
            &f.owner(),
            p::WorkbenchMaterialQuery {
                schema_version: p::SchemaVersion(1),
                node: p::NodeId(row.id.clone()),
                run: row.run.clone(),
                event: row.event.clone(),
                scope: None,
                snapshot: memory.digest,
            },
        )
        .unwrap();
    assert_eq!(target.scope.0, scope);
    let prepared = runtime
        .execute_data_workbench(
            &f.owner(),
            p::DataWorkbenchCommand {
                schema_version: p::SchemaVersion(1),
                command: p::IdempotencyKey("data:another-project:prepare".into()),
                action: p::DataWorkbenchAction::PrepareLifecycle {
                    scope: target.scope.clone(),
                    subject: target.subject.clone(),
                    operation: p::DataLifecycleOperation::Delete,
                    expected_version: target.lifecycle_version,
                },
            },
        )
        .unwrap();
    assert_eq!(prepared.state, p::DataCommandState::Prepared);
    let plan = prepared.plan.as_ref().unwrap();
    assert_eq!(plan.scope, Some(target.scope));
    assert_eq!(plan.subject, Some(target.subject));
    let apply = p::DataWorkbenchCommand {
        schema_version: p::SchemaVersion(1),
        command: p::IdempotencyKey("data:another-project:apply".into()),
        action: p::DataWorkbenchAction::Apply {
            prepared_by: prepared.command,
            plan_digest: plan.digest.clone(),
        },
    };
    let applied = runtime
        .execute_data_workbench(&f.owner(), apply.clone())
        .unwrap();
    assert_eq!(applied.state, p::DataCommandState::Confirmed);
    assert!(runtime
        .services()
        .store
        .search("owner-feedback-874ac9", 100)
        .unwrap()
        .is_empty());
    let remaining = events(&runtime);
    assert!(remaining.iter().any(|event| matches!(&event.payload,
        p::EventPayload::CandidateDowngraded(value) if value.candidate_id == old)));
    assert!(!remaining.iter().any(|event| matches!(&event.payload,
        p::EventPayload::CandidateCreated(value) if value.candidate_id == corrected.candidate)));
    assert!(remaining.iter().any(|event| matches!(&event.payload,
        p::EventPayload::CandidateCreated(value) if value.candidate_id == untouched)));
    assert_eq!(receipts(&runtime, &f).receipts, vec![corrected.clone()]);
    let heads = runtime.services().store.run_heads().unwrap();
    drop(runtime);
    let runtime = f.build();
    assert_eq!(
        runtime.execute_data_workbench(&f.owner(), apply).unwrap(),
        applied
    );
    assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    assert_eq!(receipts(&runtime, &f).receipts, vec![corrected]);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn wrong_owner_forged_source_stale_snapshot_fixed_tier_and_bad_text_write_nothing() {
    let f = Fixture::new();
    let runtime = f.build();
    let old = seed(&runtime, "run:denial", p::StabilityTier::Working);
    let valid = command(
        &runtime,
        &f,
        &old.0,
        p::WorkbenchCorrectionKind::Candidate,
        "correction:deny",
    );
    let before = runtime.services().store.run_heads().unwrap();
    assert!(runtime
        .correct_workbench_cognition(&p::VerifiedPrincipal("owner:other".into()), valid.clone())
        .is_err());
    let mut cases = Vec::new();
    let mut c = valid.clone();
    c.event = p::EventId("forged:event".into());
    cases.push(c);
    let mut c = valid.clone();
    c.snapshot = p::sha256_content_digest(b"stale");
    cases.push(c);
    let mut c = valid.clone();
    c.run = p::RunId("forged:run".into());
    cases.push(c);
    let mut c = valid.clone();
    c.schema_version = p::SchemaVersion(2);
    cases.push(c);
    let mut c = valid.clone();
    c.statement = " ".into();
    cases.push(c);
    let mut c = valid.clone();
    c.statement = "中".repeat(2731);
    cases.push(c);
    let mut c = valid.clone();
    c.statement = "bad\0text".into();
    cases.push(c);
    for c in cases {
        assert!(runtime.correct_workbench_cognition(&f.owner(), c).is_err());
    }
    assert_eq!(runtime.services().store.run_heads().unwrap(), before);
    for tier in [p::StabilityTier::Fixed, p::StabilityTier::Constitutional] {
        let id = seed(&runtime, &format!("run:{tier:?}"), tier);
        let cmd = command(
            &runtime,
            &f,
            &id.0,
            p::WorkbenchCorrectionKind::Candidate,
            &format!("correction:{tier:?}"),
        );
        let heads = runtime.services().store.run_heads().unwrap();
        assert!(runtime
            .correct_workbench_cognition(&f.owner(), cmd)
            .is_err());
        assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    }
    assert!(receipts(&runtime, &f).receipts.is_empty());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn competing_corrections_for_one_snapshot_commit_once() {
    let f = Fixture::new();
    let runtime = f.build();
    let old = seed(&runtime, "run:concurrent", p::StabilityTier::Working);
    let first = command(
        &runtime,
        &f,
        &old.0,
        p::WorkbenchCorrectionKind::Candidate,
        "correction:first",
    );
    let mut second = first.clone();
    second.command = p::IdempotencyKey("correction:second".into());
    let owner = f.owner();
    let results = std::thread::scope(|scope| {
        let runtime = &runtime;
        let left = scope.spawn(|| runtime.correct_workbench_cognition(&owner, first));
        let right = scope.spawn(|| runtime.correct_workbench_cognition(&owner, second));
        (left.join().unwrap(), right.join().unwrap())
    });
    assert_ne!(results.0.is_ok(), results.1.is_ok());
    assert_eq!(receipts(&runtime, &f).receipts.len(), 1);
    assert_eq!(events(&runtime).iter().filter(|e| matches!(&e.payload,p::EventPayload::CandidateDowngraded(v) if v.candidate_id == old)).count(),1);
}
