use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use forme_eval::{
    M5AdmissionArtifact, M5ApprovalEvidence, M5ArtifactBundle, M5ArtifactStore,
    M5DistributionArtifact, M5DistributionReceiptEvidence, M5ExecutorRecordEvidence,
    M5InstallArtifact, M5PublisherArtifact, M5TraceArtifact,
};
use forme_protocol as p;

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "forme-m5-artifact-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn artifact_path(root: &Path, prefix: &str) -> PathBuf {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix))
        })
        .unwrap()
}

fn version(value: u64) -> p::EcosystemAggregateVersion {
    p::EcosystemAggregateVersion {
        schema_version: p::M5_SCHEMA_VERSION,
        value,
    }
}

fn trace_events() -> Vec<p::EventKind> {
    vec![
        p::EventKind::RunAccepted,
        p::EventKind::SessionBound,
        p::EventKind::ResourcePlanned,
        p::EventKind::DecisionTraceRecorded,
        p::EventKind::ToolCallProposed,
        p::EventKind::ToolPolicyEvaluated,
        p::EventKind::ActionPlanned,
        p::EventKind::ApprovalRequested,
        p::EventKind::RunWaiting,
        p::EventKind::ApprovalResolved,
        p::EventKind::RunResumed,
        p::EventKind::CompetenceGateEvaluated,
        p::EventKind::RemoteExecutionLeaseChanged,
        p::EventKind::ActionStarted,
        p::EventKind::ActionOutcomeUnknown,
        p::EventKind::RunWaiting,
        p::EventKind::ActionCompleted,
        p::EventKind::CapabilityPackageDistributionRecorded,
        p::EventKind::RemoteExecutionLeaseChanged,
        p::EventKind::VerificationStarted,
        p::EventKind::VerificationFinished,
        p::EventKind::RunComplete,
    ]
}

fn bundle() -> M5ArtifactBundle {
    let package = p::CapabilityPackageRef("package:m5-artifact".into());
    let release = p::CapabilityReleaseRef("release:m5-artifact:1".into());
    let package_digest = p::sha256_content_digest(b"m5 artifact package");
    let grant = p::CapabilityPublisherGrant {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPublisherGrantRef("publisher-grant:m5-artifact:1".into()),
        publisher: p::CapabilityPublisherRef("publisher:m5-artifact".into()),
        public_key_digest: p::sha256_content_digest(b"m5 artifact public key"),
        allowed_kinds: vec![p::CapabilityPackageKind::Skill],
        scope: p::Scope("workspace:m5-artifact".into()),
        expires_at: 9_999_999,
        version: p::Version(1),
        status: p::CapabilityPublisherStatus::Active,
    };
    let admission = p::CapabilityPackageAdmission {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityAdmissionRef("admission:m5-artifact".into()),
        package: package.clone(),
        release: release.clone(),
        package_digest: package_digest.clone(),
        publisher_grant: grant.reference.clone(),
        publisher_version: grant.version,
        policy: p::CapabilityPolicyRef("policy:m5-artifact".into()),
        policy_version: p::Version(1),
        checks: p::CapabilityAdmissionCheckKind::ALL
            .into_iter()
            .map(|kind| p::CapabilityAdmissionCheck {
                schema_version: p::M5_SCHEMA_VERSION,
                kind,
                verdict: p::CapabilityAdmissionVerdict::Pass,
                evidence: p::EvidenceRef(format!(
                    "runtime-evidence:admission:{kind:?}:secret:fixture-local"
                )),
            })
            .collect(),
        dependencies: Vec::new(),
        admitted_at: 1,
    };
    let manifest = p::CapabilityPackageManifest {
        schema_version: p::M5_SCHEMA_VERSION,
        package: package.clone(),
        release: release.clone(),
        version: p::Version(1),
        kind: p::CapabilityPackageKind::Skill,
        publisher: grant.publisher.clone(),
        scope: grant.scope.clone(),
        contributions: vec![p::CapabilityContributionDescriptor {
            schema_version: p::M5_SCHEMA_VERSION,
            kind: p::CapabilityPackageKind::Skill,
            capability: p::CapabilityRef("skill:m5-artifact".into()),
            payload_digest: p::sha256_content_digest(b"m5 artifact contribution"),
            required_permissions: vec![p::PermissionRef("permission:read".into())],
            risk: p::Risk::Low,
            network: false,
            hook: false,
        }],
        dependencies: Vec::new(),
        sbom_digest: p::sha256_content_digest(b"m5 artifact SBOM"),
        license_expression: "MIT".into(),
        body_digest: p::sha256_content_digest(b"m5 artifact body"),
        max_unpacked_bytes: 1024,
        contains_executable: false,
    };
    let mut plan = p::CapabilityInstallPlan {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityInstallPlanRef("plan:m5-artifact-enable".into()),
        operation: p::CapabilityPackageOperation::Enable,
        package: package.clone(),
        release: release.clone(),
        package_digest: package_digest.clone(),
        admission: admission.reference.clone(),
        scope: grant.scope.clone(),
        policy: admission.policy.clone(),
        policy_version: admission.policy_version,
        expected_version: version(3),
        previous_release: None,
        rollback_boundary: p::RollbackBoundary("registry visibility only".into()),
        digest: p::PlanDigest(String::new()),
    };
    plan.refresh_digest().unwrap();
    let mut distribution_plan = p::CapabilityInstallPlan {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityInstallPlanRef("plan:m5-artifact-distribute".into()),
        operation: p::CapabilityPackageOperation::Distribute,
        package: package.clone(),
        release: release.clone(),
        package_digest: package_digest.clone(),
        admission: admission.reference.clone(),
        scope: grant.scope.clone(),
        policy: admission.policy.clone(),
        policy_version: admission.policy_version,
        expected_version: version(4),
        previous_release: None,
        rollback_boundary: p::RollbackBoundary("remote package bytes remain historical".into()),
        digest: p::PlanDigest(String::new()),
    };
    distribution_plan.refresh_digest().unwrap();
    let approval = p::ApprovalId("approval:m5-artifact-enable".into());
    let state = p::CapabilityPackageState {
        schema_version: p::M5_SCHEMA_VERSION,
        package: package.clone(),
        release: release.clone(),
        package_digest: package_digest.clone(),
        lifecycle: p::CapabilityLifecycleState::Enabled,
        active_generation: 2,
        plan: Some(plan.reference.clone()),
        approval: Some(approval.clone()),
    };
    let envelope_digest = p::sha256_content_digest(b"m5 artifact distribution envelope");
    let mut record = p::CapabilityExecutorInstallRecord {
        schema_version: p::M5_SCHEMA_VERSION,
        peer: p::FederatedPeerRef("peer:m5-artifact-executor".into()),
        package: package.clone(),
        release: release.clone(),
        package_digest: package_digest.clone(),
        envelope_digest: envelope_digest.clone(),
        installed_generation: 1,
        record_digest: p::SchemaDigest(String::new()),
    };
    record.refresh_digest().unwrap();
    let receipt = p::CapabilityPackageDistributionReceipt {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityDistributionReceiptRef("distribution:m5-artifact".into()),
        package: package.clone(),
        release: release.clone(),
        package_digest: package_digest.clone(),
        peer: record.peer.clone(),
        peer_grant: p::FederatedPeerGrantRef("peer-grant:m5-artifact-executor".into()),
        authority_epoch: p::AuthorityEpoch(3),
        plan_digest: p::PlanDigest(p::sha256_content_digest(b"remote plan").0),
        lease: p::RemoteExecutionLeaseRef("lease:m5-artifact".into()),
        fence_token: 7,
        installed_generation: record.installed_generation,
        ground_truth: p::EvidenceRef(format!(
            "executor-package-record:{}",
            record.record_digest.0
        )),
        verified: p::RequiredTrue,
    };
    let executor_record = M5ExecutorRecordEvidence::from_runtime(&record).unwrap();
    let receipt = M5DistributionReceiptEvidence::from_runtime(
        &distribution_plan,
        &record,
        &executor_record,
        &receipt,
    )
    .unwrap();
    let event_kinds = trace_events();
    M5ArtifactBundle {
        publisher: M5PublisherArtifact::from_runtime(&grant, 1, version(1)).unwrap(),
        admission: M5AdmissionArtifact::from_runtime(
            &admission,
            &manifest,
            p::ContentRef("catalog-content:m5-artifact".into()),
            version(2),
        )
        .unwrap(),
        install: M5InstallArtifact {
            schema_version: p::M5_SCHEMA_VERSION,
            plan: plan.clone(),
            approval: M5ApprovalEvidence {
                schema_version: p::M5_SCHEMA_VERSION,
                approval,
                plan_digest: plan.digest.clone(),
                principal: p::VerifiedPrincipal("owner:m5-artifact".into()),
            },
            state,
            committed_version: version(4),
            registry_digest: p::sha256_content_digest(b"m5 artifact registry generation 2"),
        },
        distribution: M5DistributionArtifact {
            schema_version: p::M5_SCHEMA_VERSION,
            capability_plan: distribution_plan.reference,
            capability_plan_digest: distribution_plan.digest,
            semantic_envelope_digest: envelope_digest,
            executor_record,
            receipt: receipt.clone(),
            committed_version: version(5),
        },
        trace: M5TraceArtifact {
            schema_version: p::M5_SCHEMA_VERSION,
            scenario: "S98 governed ecosystem golden".into(),
            run: p::RunId("run:m5-artifact".into()),
            package,
            release,
            package_digest,
            publisher_grant: grant.reference,
            admission: admission.reference,
            install_plan: plan.reference,
            distribution_receipt: receipt.reference,
            stream_seq: (1..=event_kinds.len() as u64).collect(),
            event_kinds,
            event_taxonomy: p::EventKind::ALL[..p::M5_EVENT_KIND_COUNT].to_vec(),
            registry_fetches: 1,
            authority_driver_calls: 1,
            executor_installs: 1,
            distribution_events: 1,
            unknown_retry_count: 0,
            post_revoke_visible_contributions: 0,
            post_revoke_distribution_attempts: 0,
            restricted_material_matches: 0,
            ground_truth_verified: true,
        },
    }
}

#[test]
fn s98_runtime_admission_evidence_is_projected_without_sensitive_labels() {
    let first = bundle();
    let second = bundle();
    assert_eq!(
        first.admission.admission.checks,
        second.admission.admission.checks
    );
    for check in &first.admission.admission.checks {
        assert!(check
            .evidence
            .0
            .starts_with("admission-check-evidence:sha256:"));
    }
    let encoded = serde_json::to_string(&first.admission).unwrap();
    assert!(!encoded.contains("runtime-evidence:"));
    assert!(!encoded.to_ascii_lowercase().contains("secret:"));
}

#[test]
fn s98_m5_five_file_artifact_set_is_content_addressed_and_offline_verifiable() {
    let root = temp_root("pass");
    let store = M5ArtifactStore::new(&root).unwrap();
    let receipt = store.write(&bundle()).unwrap();
    assert!(receipt.digest.0.starts_with("sha256:"));
    assert_eq!(
        receipt.content_ref.0,
        format!("artifact:{}", receipt.digest.0)
    );
    assert_eq!(store.verify_complete_set().unwrap().digest, receipt.digest);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 5);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn s99_artifact_gate_rejects_tamper_unknown_fields_extra_and_missing_files() {
    let root = temp_root("body-tamper");
    let store = M5ArtifactStore::new(&root).unwrap();
    store.write(&bundle()).unwrap();
    let path = artifact_path(&root, "distribution-");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["body"]["receipt"]["fence_token"] = serde_json::Value::from(99_u64);
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert!(store.verify_complete_set().is_err());
    fs::remove_dir_all(root).unwrap();

    let root = temp_root("unknown-field");
    let store = M5ArtifactStore::new(&root).unwrap();
    store.write(&bundle()).unwrap();
    let path = artifact_path(&root, "trace-");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["body"]["unchecked"] = serde_json::Value::Bool(true);
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert!(store.verify_complete_set().is_err());
    fs::remove_dir_all(root).unwrap();

    let root = temp_root("extra");
    let store = M5ArtifactStore::new(&root).unwrap();
    store.write(&bundle()).unwrap();
    fs::write(root.join("unexpected.txt"), b"unexpected").unwrap();
    assert!(store.verify_complete_set().is_err());
    fs::remove_dir_all(root).unwrap();

    let root = temp_root("missing");
    let store = M5ArtifactStore::new(&root).unwrap();
    store.write(&bundle()).unwrap();
    fs::remove_file(artifact_path(&root, "publisher-")).unwrap();
    assert!(store.verify_complete_set().is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn s99_artifact_gate_rejects_sensitive_material_and_cross_reference_drift() {
    for (label, mutation) in [("secret", 0_u8), ("host-path", 1), ("endpoint", 2)] {
        let root = temp_root(label);
        let store = M5ArtifactStore::new(&root).unwrap();
        let mut artifact = bundle();
        match mutation {
            0 => artifact.trace.scenario = "Authorization: Bearer fixture-value".into(),
            1 => artifact.publisher.grant.scope = p::Scope("C:\\Users\\owner\\private".into()),
            _ => artifact.trace.scenario = "https://localhost:9443/private".into(),
        }
        assert!(store.write(&artifact).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    let mut artifact = bundle();
    artifact.distribution.receipt.package = p::CapabilityPackageRef("package:other".into());
    assert!(artifact.validate().is_err());

    let mut artifact = bundle();
    artifact.trace.unknown_retry_count = 1;
    assert!(artifact.validate().is_err());

    let mut artifact = bundle();
    artifact.trace.event_taxonomy.swap(0, 1);
    assert!(artifact.validate().is_err());
}
