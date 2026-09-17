use forme_protocol as p;

const EMPTY_SHA256: &str =
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn package() -> p::SignedCapabilityPackage {
    let resource_digest = p::SchemaDigest(EMPTY_SHA256.into());
    let mut package = p::SignedCapabilityPackage {
        schema_version: p::M5_SCHEMA_VERSION,
        manifest: p::CapabilityPackageManifest {
            schema_version: p::M5_SCHEMA_VERSION,
            package: p::CapabilityPackageRef("package:fixture".into()),
            release: p::CapabilityReleaseRef("release:fixture:v1".into()),
            version: p::Version(1),
            kind: p::CapabilityPackageKind::Skill,
            publisher: p::CapabilityPublisherRef("publisher:fixture".into()),
            scope: p::Scope("workspace:fixture".into()),
            contributions: vec![p::CapabilityContributionDescriptor {
                schema_version: p::M5_SCHEMA_VERSION,
                kind: p::CapabilityPackageKind::Skill,
                capability: p::CapabilityRef("skill:fixture".into()),
                payload_digest: resource_digest.clone(),
                required_permissions: vec![p::PermissionRef("permission:read".into())],
                risk: p::Risk::Low,
                network: false,
                hook: false,
            }],
            dependencies: Vec::new(),
            sbom_digest: p::SchemaDigest(EMPTY_SHA256.into()),
            license_expression: "Apache-2.0".into(),
            body_digest: p::SchemaDigest(EMPTY_SHA256.into()),
            max_unpacked_bytes: 128,
            contains_executable: false,
        },
        resources: vec![p::CapabilityPackageResource {
            schema_version: p::M5_SCHEMA_VERSION,
            relative_path: "skills/fixture.txt".into(),
            content: String::new(),
            digest: resource_digest,
        }],
        package_digest: p::SchemaDigest(EMPTY_SHA256.into()),
        signature: p::PackageSignature(format!("ed25519:{}", "00".repeat(64))),
    };
    package.refresh_digests().unwrap();
    package
}

fn admission(package: &p::SignedCapabilityPackage) -> p::CapabilityPackageAdmission {
    p::CapabilityPackageAdmission {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityAdmissionRef("admission:fixture:v1".into()),
        package: package.manifest.package.clone(),
        release: package.manifest.release.clone(),
        package_digest: package.package_digest.clone(),
        publisher_grant: p::CapabilityPublisherGrantRef("grant:publisher:fixture:v1".into()),
        publisher_version: p::Version(1),
        policy: p::CapabilityPolicyRef("policy:ecosystem".into()),
        policy_version: p::Version(1),
        checks: p::CapabilityAdmissionCheckKind::ALL
            .into_iter()
            .map(|kind| p::CapabilityAdmissionCheck {
                schema_version: p::M5_SCHEMA_VERSION,
                kind,
                verdict: p::CapabilityAdmissionVerdict::Pass,
                evidence: p::EvidenceRef(format!("evidence:{kind:?}")),
            })
            .collect(),
        dependencies: Vec::new(),
        admitted_at: 10,
    }
}

fn distribution_plan(package: &p::SignedCapabilityPackage) -> p::CapabilityInstallPlan {
    let mut plan = p::CapabilityInstallPlan {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityInstallPlanRef("plan:fixture:distribute".into()),
        operation: p::CapabilityPackageOperation::Distribute,
        package: package.manifest.package.clone(),
        release: package.manifest.release.clone(),
        package_digest: package.package_digest.clone(),
        admission: p::CapabilityAdmissionRef("admission:fixture:v1".into()),
        scope: package.manifest.scope.clone(),
        policy: p::CapabilityPolicyRef("policy:ecosystem".into()),
        policy_version: p::Version(1),
        expected_version: p::EcosystemAggregateVersion {
            schema_version: p::M5_SCHEMA_VERSION,
            value: 2,
        },
        previous_release: None,
        rollback_boundary: p::RollbackBoundary("peer package bytes remain historical".into()),
        digest: p::PlanDigest(String::new()),
    };
    plan.refresh_digest().unwrap();
    plan
}

fn publisher_grant() -> p::CapabilityPublisherGrant {
    p::CapabilityPublisherGrant {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPublisherGrantRef("grant:publisher:fixture:v1".into()),
        publisher: p::CapabilityPublisherRef("publisher:fixture".into()),
        public_key_digest: p::SchemaDigest(EMPTY_SHA256.into()),
        allowed_kinds: vec![p::CapabilityPackageKind::Skill],
        scope: p::Scope("workspace:fixture".into()),
        expires_at: 1_000,
        version: p::Version(1),
        status: p::CapabilityPublisherStatus::Active,
    }
}

fn admission_policy() -> p::CapabilityAdmissionPolicy {
    p::CapabilityAdmissionPolicy {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPolicyRef("policy:ecosystem".into()),
        version: p::Version(1),
        allowed_kinds: vec![p::CapabilityPackageKind::Skill],
        allowed_licenses: vec!["Apache-2.0".into()],
        max_package_bytes: 4_096,
        max_dependencies: 4,
        max_depth: 3,
        allow_network: false,
        allow_hooks: false,
    }
}

#[test]
fn m5_taxonomy_is_additive_after_the_exact_m4_prefix() {
    assert_eq!(p::EventKind::ALL.len(), 99);
    assert_eq!(
        &p::EventKind::ALL[89..93],
        &[
            p::EventKind::FederatedPeerRegistered,
            p::EventKind::FederatedPeerRevoked,
            p::EventKind::RemoteExecutionLeaseChanged,
            p::EventKind::ReplicationCheckpointAdvanced,
        ]
    );
    assert_eq!(
        &p::EventKind::ALL[93..97],
        &[
            p::EventKind::CapabilityPublisherChanged,
            p::EventKind::CapabilityPackageAdmitted,
            p::EventKind::CapabilityPackageStateChanged,
            p::EventKind::CapabilityPackageDistributionRecorded,
        ]
    );
}

#[test]
fn signed_declarative_package_is_content_addressed_closed_and_tamper_evident() {
    let package = package();
    package.validate().unwrap();
    assert_eq!(
        p::sha256_digest_bytes(&package.package_digest)
            .unwrap()
            .len(),
        32
    );

    let mut tampered = package.clone();
    tampered.resources[0].content = "changed".into();
    assert!(tampered.validate().is_err());

    let mut traversal = package.clone();
    traversal.resources[0].relative_path = "../outside".into();
    assert!(traversal.validate().is_err());

    let mut executable = package;
    executable.manifest.contains_executable = true;
    assert!(executable.validate().is_err());
}

#[test]
fn admission_requires_every_hard_check_to_pass() {
    let package = package();
    let admission = admission(&package);
    admission.validate().unwrap();

    let mut failed = admission.clone();
    failed.checks[6].verdict = p::CapabilityAdmissionVerdict::Fail;
    assert!(failed.validate().is_err());

    let mut missing = admission;
    missing.checks.pop();
    assert!(missing.validate().is_err());
}

#[test]
fn lifecycle_plan_is_digest_bound_and_rollback_never_claims_external_reversal() {
    let package = package();
    let mut plan = p::CapabilityInstallPlan {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityInstallPlanRef("plan:fixture:install".into()),
        operation: p::CapabilityPackageOperation::Install,
        package: package.manifest.package.clone(),
        release: package.manifest.release.clone(),
        package_digest: package.package_digest.clone(),
        admission: p::CapabilityAdmissionRef("admission:fixture:v1".into()),
        scope: package.manifest.scope.clone(),
        policy: p::CapabilityPolicyRef("policy:ecosystem".into()),
        policy_version: p::Version(1),
        expected_version: p::EcosystemAggregateVersion::zero(),
        previous_release: None,
        rollback_boundary: p::RollbackBoundary("registry visibility only".into()),
        digest: p::PlanDigest(String::new()),
    };
    plan.refresh_digest().unwrap();
    plan.validate().unwrap();
    plan.scope = p::Scope("workspace:changed".into());
    assert!(plan.validate().is_err());

    let invalid = p::CapabilityPackageStateChange {
        schema_version: p::M5_SCHEMA_VERSION,
        plan: p::CapabilityInstallPlanRef("plan:rollback".into()),
        approval: p::ApprovalId("approval:rollback".into()),
        package: package.manifest.package,
        release: package.manifest.release,
        from: p::CapabilityLifecycleState::Enabled,
        to: p::CapabilityLifecycleState::Enabled,
        active_generation: 2,
        reason: p::ReasonRef("rollback".into()),
        external_effects_reverted: true,
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn m5_payloads_round_trip_without_reinterpreting_admission_as_activation() {
    let package = package();
    let admission = admission(&package);
    let payload = p::EventPayload::CapabilityPackageAdmitted(p::CapabilityPackageAdmittedPayload {
        admission,
        committed_version: p::EcosystemAggregateVersion {
            schema_version: p::M5_SCHEMA_VERSION,
            value: 1,
        },
    });
    let bytes = serde_json::to_vec(&payload).unwrap();
    let decoded: p::EventPayload = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded.kind(), p::EventKind::CapabilityPackageAdmitted);
    assert!(!matches!(
        decoded,
        p::EventPayload::CapabilityPackageStateChanged(_)
    ));
}

#[test]
fn distribution_envelope_and_executor_record_are_closed_content_addresses() {
    let package = package();
    let mut envelope = p::CapabilityPackageDistributionEnvelope {
        schema_version: p::M5_SCHEMA_VERSION,
        install_plan: distribution_plan(&package),
        package: package.clone(),
        admission: admission(&package),
        dependency_admissions: Vec::new(),
        publisher_grant: publisher_grant(),
        authority_policy: admission_policy(),
        target_peer: p::FederatedPeerRef("peer:executor".into()),
        authority_epoch: p::AuthorityEpoch(3),
        content_digest: p::SchemaDigest(String::new()),
    };
    envelope.refresh_digest().unwrap();
    envelope.validate().unwrap();

    let mut tampered = envelope.clone();
    tampered.target_peer = p::FederatedPeerRef("peer:other".into());
    assert!(tampered.validate().is_err());

    let mut encoded = serde_json::to_value(&envelope).unwrap();
    encoded
        .as_object_mut()
        .unwrap()
        .insert("credential".into(), serde_json::json!("forbidden"));
    assert!(serde_json::from_value::<p::CapabilityPackageDistributionEnvelope>(encoded).is_err());

    let mut record = p::CapabilityExecutorInstallRecord {
        schema_version: p::M5_SCHEMA_VERSION,
        peer: envelope.target_peer,
        package: package.manifest.package,
        release: package.manifest.release,
        package_digest: package.package_digest,
        envelope_digest: envelope.content_digest,
        installed_generation: 1,
        record_digest: p::SchemaDigest(String::new()),
    };
    record.refresh_digest().unwrap();
    record.validate().unwrap();
    record.installed_generation = 2;
    assert!(record.validate().is_err());
}
