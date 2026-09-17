use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use forme_approval::{ApprovalGrant, GrantScope};
use forme_capabilities::{
    CapabilityPackageVerifier, CapabilityRegistry, Ed25519CapabilityPackageVerifier,
    InMemoryCapabilityRegistry, InMemoryPublisherKeyring, PublisherPublicKey,
};
use forme_harness::{
    AgentHarness, EcosystemGatewayControl, EcosystemHarnessRuntime, HarnessActionIngress,
    ProjectAppApiRuntimeConfig, ReactiveHarness, ResumeInput,
};
use forme_protocol as p;
use forme_store::{
    EcosystemEventStore, EcosystemProjection, EventStore, SqliteEventStore, StoreOptions,
};

const EMPTY_SHA256: &str =
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn owner() -> p::VerifiedPrincipal {
    p::VerifiedPrincipal("owner:m5".into())
}

fn reactive_owner() -> p::VerifiedPrincipal {
    p::VerifiedPrincipal(std::env::var("FORME_OWNER_ID").unwrap_or_else(|_| "local-owner".into()))
}

fn policy() -> p::CapabilityAdmissionPolicy {
    p::CapabilityAdmissionPolicy {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPolicyRef("policy:m5-harness".into()),
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

fn signed_package(key: &SigningKey, version: u32) -> p::SignedCapabilityPackage {
    let mut package = p::SignedCapabilityPackage {
        schema_version: p::M5_SCHEMA_VERSION,
        manifest: p::CapabilityPackageManifest {
            schema_version: p::M5_SCHEMA_VERSION,
            package: p::CapabilityPackageRef("package:m5-harness".into()),
            release: p::CapabilityReleaseRef(format!("release:m5-harness:v{version}")),
            version: p::Version(version),
            kind: p::CapabilityPackageKind::Skill,
            publisher: p::CapabilityPublisherRef("publisher:m5-harness".into()),
            scope: p::Scope("workspace:m5-harness".into()),
            contributions: vec![p::CapabilityContributionDescriptor {
                schema_version: p::M5_SCHEMA_VERSION,
                kind: p::CapabilityPackageKind::Skill,
                capability: p::CapabilityRef(format!("skill:m5-harness:v{version}")),
                payload_digest: p::SchemaDigest(EMPTY_SHA256.into()),
                required_permissions: vec![p::PermissionRef("permission:read".into())],
                risk: p::Risk::Low,
                network: false,
                hook: false,
            }],
            dependencies: Vec::new(),
            sbom_digest: p::SchemaDigest(EMPTY_SHA256.into()),
            license_expression: "Apache-2.0".into(),
            body_digest: p::SchemaDigest(EMPTY_SHA256.into()),
            max_unpacked_bytes: 4_096,
            contains_executable: false,
        },
        resources: vec![p::CapabilityPackageResource {
            schema_version: p::M5_SCHEMA_VERSION,
            relative_path: format!("skills/v{version}.txt"),
            content: format!("governed skill version {version}"),
            digest: p::SchemaDigest(EMPTY_SHA256.into()),
        }],
        package_digest: p::SchemaDigest(EMPTY_SHA256.into()),
        signature: p::PackageSignature(format!("ed25519:{}", "00".repeat(64))),
    };
    package.refresh_digests().unwrap();
    package.manifest.contributions[0].payload_digest = package.resources[0].digest.clone();
    package.refresh_digests().unwrap();
    let digest = p::sha256_digest_bytes(&package.package_digest).unwrap();
    let signature = key.sign(&digest).to_bytes();
    package.signature = p::PackageSignature(format!(
        "ed25519:{}",
        signature
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ));
    package.validate().unwrap();
    package
}

fn grant(
    key: &SigningKey,
    version: u32,
    status: p::CapabilityPublisherStatus,
) -> p::CapabilityPublisherGrant {
    let public = PublisherPublicKey::from_bytes(key.verifying_key().to_bytes());
    p::CapabilityPublisherGrant {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPublisherGrantRef(format!("grant:m5-harness:v{version}")),
        publisher: p::CapabilityPublisherRef("publisher:m5-harness".into()),
        public_key_digest: public.digest(),
        allowed_kinds: vec![p::CapabilityPackageKind::Skill],
        scope: p::Scope("workspace:m5-harness".into()),
        expires_at: now_ms() + 60_000,
        version: p::Version(version),
        status,
    }
}

fn append_catalog_evidence(
    store: &SqliteEventStore,
    run: &str,
    package: &p::SignedCapabilityPackage,
) {
    let intent = p::ActionId(format!("action:{run}"));
    let approval = p::ApprovalId(format!("approval:{run}"));
    let body = serde_json::to_string(package).unwrap();
    let body_digest = p::sha256_content_digest(body.as_bytes());
    let done_contract = p::DoneContractRef(format!("done-contract:{run}"));
    let events = vec![
        p::EventPayload::ApprovalRequested(p::ApprovalRequestedPayload {
            approval_id: approval.clone(),
            action_summary: p::ActionSummary("read capability registry once".into()),
            risk: p::Risk::High,
            scope: package.manifest.scope.clone(),
            rollback_boundary: p::RollbackBoundary("external read cannot be undone".into()),
            expires_at: now_ms() + 60_000,
            choices: vec![p::ApprovalChoice("approve once".into())],
            requested_permissions: vec![p::PermissionRef("permission:network-read".into())],
            affected_resources: vec![p::ResourceRef("registry:loopback".into())],
        }),
        p::EventPayload::ApprovalResolved(p::ApprovalResolvedPayload {
            approval_id: approval,
            outcome: p::ApprovalOutcome::Granted,
            grant_ref: Some(p::ApprovalGrantRef(format!("grant:{run}"))),
        }),
        p::EventPayload::ActionPlanned(p::ActionPlannedPayload {
            intent_id: intent.clone(),
            plan_digest: p::PlanDigest(format!("plan:{run}")),
            backend: p::BackendKind::AppApi,
            expected_effect: p::ExpectedEffect::Outward,
            source: p::Source::UserTurn,
            scope: package.manifest.scope.clone(),
            approval_ref: Some(p::ApprovalId(format!("approval:{run}"))),
            remote_placement: None,
        }),
        p::EventPayload::ActionStarted(p::ActionStartedPayload {
            intent_id: intent.clone(),
            backend: p::BackendKind::AppApi,
            scope: package.manifest.scope.clone(),
            remote_lease: None,
        }),
        p::EventPayload::ActionOutputDelta(p::ActionOutputDeltaPayload {
            intent_id: intent.clone(),
            backend: p::BackendKind::AppApi,
            scope: package.manifest.scope.clone(),
            delta: body.clone(),
            truncated: false,
            trust: p::TrustTier::Untrusted,
            content_ref: None,
            remote_lease: None,
        }),
        p::EventPayload::ActionCompleted(p::ActionCompletedPayload {
            intent_id: intent,
            result_ref: p::ActionResultRef(format!("result:{run}")),
            receipt: Some(p::ExternalActionReceipt {
                schema_version: p::M5_SCHEMA_VERSION,
                action: p::ActionId(format!("action:{run}")),
                content_ref: Some(p::ContentRef(format!("external:{}", body_digest.0))),
                content_digest: Some(body_digest),
                trust: p::TrustTier::Untrusted,
                effect: p::EffectStatus::Observed,
                probe_hint: None,
            }),
            remote_receipt: None,
        }),
        p::EventPayload::VerificationStarted(p::VerificationStartedPayload {
            verifier_kind: p::VerifierKind("deterministic".into()),
            against: done_contract.clone(),
        }),
        p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
            verifier_kind: p::VerifierKind("deterministic".into()),
            outcome: p::VerificationOutcome::Pass,
            against: done_contract,
        }),
        p::EventPayload::RunComplete(p::RunCompletePayload {
            stop_reason: p::StopReason("catalog observation verified".into()),
            result_ref: None,
        }),
    ];
    for (index, payload) in events.into_iter().enumerate() {
        let external = matches!(
            payload.kind(),
            p::EventKind::ActionOutputDelta | p::EventKind::ActionCompleted
        );
        store
            .append(p::Event::new(
                p::EventId(format!("event:{run}:{index}")),
                p::RunId(run.into()),
                None,
                payload,
                p::M5_SCHEMA_VERSION,
                now_ms() + index as i64,
                if external {
                    p::Provenance {
                        source: p::Source::Communication,
                        actor: p::Actor::External(p::ParticipantId("catalog-registry".into())),
                        trust_tier: p::TrustTier::Untrusted,
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
            ))
            .unwrap();
    }
}

fn approval(plan: &p::CapabilityInstallPlan, suffix: &str) -> p::CapabilityPackageApproval {
    p::CapabilityPackageApproval {
        schema_version: p::M5_SCHEMA_VERSION,
        approval: p::ApprovalId(format!("approval:m5:{suffix}")),
        plan_digest: plan.digest.clone(),
        principal: owner(),
        nonce: p::Nonce(format!("nonce:m5:{suffix}")),
        expires_at: now_ms() + 60_000,
    }
}

fn temporary_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "forme-m5-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn s85_s90_owner_admission_plan_and_lifecycle_are_separate_authority_steps() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let capabilities = Arc::new(InMemoryCapabilityRegistry::default());
    let runtime =
        EcosystemHarnessRuntime::new(store.clone(), owner(), policy(), capabilities.clone())
            .unwrap();
    let key = SigningKey::from_bytes(&[21; 32]);
    runtime
        .configure_publisher_key(
            p::CapabilityPublisherRef("publisher:m5-harness".into()),
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();

    let outsider = runtime.provision_capability_publisher(
        p::RunId("run:m5-outsider".into()),
        grant(&key, 1, p::CapabilityPublisherStatus::Active),
        None,
        p::EcosystemAggregateVersion::zero(),
        p::VerifiedPrincipal("owner:other".into()),
    );
    assert!(outsider.is_err());
    assert!(store
        .read_run(p::RunId("run:m5-outsider".into()))
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .is_empty());

    runtime
        .provision_capability_publisher(
            p::RunId("run:m5-publisher".into()),
            grant(&key, 1, p::CapabilityPublisherStatus::Active),
            None,
            p::EcosystemAggregateVersion::zero(),
            owner(),
        )
        .unwrap();
    let package = signed_package(&key, 1);
    append_catalog_evidence(&store, "run:m5-catalog-v1", &package);
    runtime
        .admit_capability_package(
            p::RunId("run:m5-admit-v1".into()),
            p::RunId("run:m5-catalog-v1".into()),
            package.clone(),
            now_ms(),
            owner(),
        )
        .unwrap();
    assert!(CapabilityRegistry::resolve_toolset(
        capabilities.as_ref(),
        &resolve_context(&["skill:m5-harness:v1"]),
    )
    .unwrap()
    .items
    .is_empty());

    let install = runtime
        .prepare_capability_change(
            p::CapabilityPackageOperation::Install,
            package.manifest.package.clone(),
            package.manifest.release.clone(),
            owner(),
        )
        .unwrap();
    runtime
        .apply_capability_change(
            p::RunId("run:m5-install".into()),
            install.clone(),
            approval(&install, "install"),
            p::ReasonRef("owner approved exact release".into()),
            now_ms(),
        )
        .unwrap();
    let enable = runtime
        .prepare_capability_change(
            p::CapabilityPackageOperation::Enable,
            package.manifest.package.clone(),
            package.manifest.release.clone(),
            owner(),
        )
        .unwrap();
    let wrong_key = SigningKey::from_bytes(&[99; 32]);
    runtime
        .configure_publisher_key(
            p::CapabilityPublisherRef("publisher:m5-harness".into()),
            PublisherPublicKey::from_bytes(wrong_key.verifying_key().to_bytes()),
        )
        .unwrap();
    assert!(runtime
        .apply_capability_change(
            p::RunId("run:m5-enable-key-drift".into()),
            enable.clone(),
            approval(&enable, "enable-key-drift"),
            p::ReasonRef("must fail after key drift".into()),
            now_ms(),
        )
        .is_err());
    assert!(store
        .read_run(p::RunId("run:m5-enable-key-drift".into()))
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .is_empty());
    assert!(CapabilityRegistry::resolve_toolset(
        capabilities.as_ref(),
        &resolve_context(&["skill:m5-harness:v1"]),
    )
    .unwrap()
    .items
    .is_empty());
    runtime
        .configure_publisher_key(
            p::CapabilityPublisherRef("publisher:m5-harness".into()),
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();
    runtime
        .apply_capability_change(
            p::RunId("run:m5-enable".into()),
            enable.clone(),
            approval(&enable, "enable"),
            p::ReasonRef("owner enabled admitted package".into()),
            now_ms(),
        )
        .unwrap();
    assert_eq!(
        CapabilityRegistry::resolve_toolset(
            capabilities.as_ref(),
            &resolve_context(&["skill:m5-harness:v1"]),
        )
        .unwrap()
        .items
        .len(),
        1
    );

    let kinds = store
        .read_run(p::RunId("run:m5-enable".into()))
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::ApprovalRequested,
            p::EventKind::ApprovalResolved,
            p::EventKind::CapabilityPackageStateChanged,
            p::EventKind::RunComplete,
        ]
    );

    let mut drifted = enable;
    drifted.scope = p::Scope("workspace:changed".into());
    assert!(runtime
        .apply_capability_change(
            p::RunId("run:m5-drift".into()),
            drifted.clone(),
            approval(&drifted, "drift"),
            p::ReasonRef("invalid drift".into()),
            now_ms(),
        )
        .is_err());
    assert!(store
        .read_run(p::RunId("run:m5-drift".into()))
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .is_empty());
}

#[test]
fn s92_s93_update_rollback_and_publisher_revoke_preserve_history_and_fence_visibility() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let capabilities = Arc::new(InMemoryCapabilityRegistry::default());
    let runtime =
        EcosystemHarnessRuntime::new(store.clone(), owner(), policy(), capabilities.clone())
            .unwrap();
    let key = SigningKey::from_bytes(&[22; 32]);
    runtime
        .configure_publisher_key(
            p::CapabilityPublisherRef("publisher:m5-harness".into()),
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();
    runtime
        .provision_capability_publisher(
            p::RunId("run:m5-u-publisher".into()),
            grant(&key, 1, p::CapabilityPublisherStatus::Active),
            None,
            p::EcosystemAggregateVersion::zero(),
            owner(),
        )
        .unwrap();

    let v1 = signed_package(&key, 1);
    append_catalog_evidence(&store, "run:m5-u-catalog-v1", &v1);
    runtime
        .admit_capability_package(
            p::RunId("run:m5-u-admit-v1".into()),
            p::RunId("run:m5-u-catalog-v1".into()),
            v1.clone(),
            now_ms(),
            owner(),
        )
        .unwrap();
    for (operation, suffix) in [
        (p::CapabilityPackageOperation::Install, "install"),
        (p::CapabilityPackageOperation::Enable, "enable"),
    ] {
        let plan = runtime
            .prepare_capability_change(
                operation,
                v1.manifest.package.clone(),
                v1.manifest.release.clone(),
                owner(),
            )
            .unwrap();
        runtime
            .apply_capability_change(
                p::RunId(format!("run:m5-u-{suffix}")),
                plan.clone(),
                approval(&plan, &format!("u-{suffix}")),
                p::ReasonRef(format!("owner {suffix}")),
                now_ms(),
            )
            .unwrap();
    }

    let v2 = signed_package(&key, 2);
    append_catalog_evidence(&store, "run:m5-u-catalog-v2", &v2);
    runtime
        .admit_capability_package(
            p::RunId("run:m5-u-admit-v2".into()),
            p::RunId("run:m5-u-catalog-v2".into()),
            v2.clone(),
            now_ms(),
            owner(),
        )
        .unwrap();
    let update = runtime
        .prepare_capability_change(
            p::CapabilityPackageOperation::Update,
            v2.manifest.package.clone(),
            v2.manifest.release.clone(),
            owner(),
        )
        .unwrap();
    runtime
        .apply_capability_change(
            p::RunId("run:m5-u-update".into()),
            update.clone(),
            approval(&update, "update"),
            p::ReasonRef("owner switched to v2".into()),
            now_ms(),
        )
        .unwrap();
    let rollback = runtime
        .prepare_capability_change(
            p::CapabilityPackageOperation::Rollback,
            v1.manifest.package.clone(),
            v1.manifest.release.clone(),
            owner(),
        )
        .unwrap();
    runtime
        .apply_capability_change(
            p::RunId("run:m5-u-rollback".into()),
            rollback.clone(),
            approval(&rollback, "rollback"),
            p::ReasonRef("verification selected known-good v1".into()),
            now_ms(),
        )
        .unwrap();
    assert!(!rollback.rollback_boundary.0.is_empty());

    let expected = store
        .ecosystem_version(&p::EcosystemAggregateRef("ecosystem".into()))
        .unwrap();
    runtime
        .provision_capability_publisher(
            p::RunId("run:m5-u-revoke-publisher".into()),
            grant(&key, 2, p::CapabilityPublisherStatus::Revoked),
            Some(p::CapabilityPublisherGrantRef("grant:m5-harness:v1".into())),
            expected,
            owner(),
        )
        .unwrap();
    assert!(CapabilityRegistry::resolve_toolset(
        capabilities.as_ref(),
        &resolve_context(&["skill:m5-harness:v1", "skill:m5-harness:v2"]),
    )
    .unwrap()
    .items
    .is_empty());
    assert!(runtime
        .prepare_capability_change(
            p::CapabilityPackageOperation::Enable,
            v1.manifest.package,
            v1.manifest.release,
            owner(),
        )
        .is_err());
}

#[test]
fn s91_s97_restart_reverifies_archive_signature_publisher_dependencies_and_policy() {
    let root = temporary_root("restart-reverify");
    fs::create_dir_all(&root).unwrap();
    let database = root.join("authority.sqlite3");
    let key = SigningKey::from_bytes(&[31; 32]);
    let publisher = p::CapabilityPublisherRef("publisher:m5-harness".into());
    let public_key = PublisherPublicKey::from_bytes(key.verifying_key().to_bytes());
    let keyring = Arc::new(InMemoryPublisherKeyring::default());
    keyring
        .provision(publisher.clone(), public_key.clone())
        .unwrap();
    let store = SqliteEventStore::open(&database, StoreOptions::default()).unwrap();
    let capabilities = Arc::new(InMemoryCapabilityRegistry::default());
    let runtime = EcosystemHarnessRuntime::new_with_keyring(
        store.clone(),
        owner(),
        policy(),
        capabilities.clone(),
        keyring,
    )
    .unwrap();
    runtime
        .provision_capability_publisher(
            p::RunId("run:m5-restart-publisher".into()),
            grant(&key, 1, p::CapabilityPublisherStatus::Active),
            None,
            p::EcosystemAggregateVersion::zero(),
            owner(),
        )
        .unwrap();
    let package = signed_package(&key, 1);
    append_catalog_evidence(&store, "run:m5-restart-catalog", &package);
    runtime
        .admit_capability_package(
            p::RunId("run:m5-restart-admit".into()),
            p::RunId("run:m5-restart-catalog".into()),
            package.clone(),
            now_ms(),
            owner(),
        )
        .unwrap();
    for (operation, suffix) in [
        (p::CapabilityPackageOperation::Install, "install"),
        (p::CapabilityPackageOperation::Enable, "enable"),
    ] {
        let plan = runtime
            .prepare_capability_change(
                operation,
                package.manifest.package.clone(),
                package.manifest.release.clone(),
                owner(),
            )
            .unwrap();
        runtime
            .apply_capability_change(
                p::RunId(format!("run:m5-restart-{suffix}")),
                plan.clone(),
                approval(&plan, &format!("restart-{suffix}")),
                p::ReasonRef(format!("owner {suffix}")),
                now_ms(),
            )
            .unwrap();
    }
    drop(runtime);
    drop(capabilities);
    drop(store);

    let restart_keyring = Arc::new(InMemoryPublisherKeyring::default());
    restart_keyring
        .provision(publisher.clone(), public_key)
        .unwrap();
    let restarted_registry = Arc::new(InMemoryCapabilityRegistry::default());
    let restarted = EcosystemHarnessRuntime::new_with_keyring(
        SqliteEventStore::open(&database, StoreOptions::default()).unwrap(),
        owner(),
        policy(),
        restarted_registry.clone(),
        restart_keyring,
    )
    .unwrap();
    assert_eq!(
        CapabilityRegistry::resolve_toolset(
            restarted_registry.as_ref(),
            &resolve_context(&["skill:m5-harness:v1"]),
        )
        .unwrap()
        .items
        .len(),
        1
    );
    drop(restarted);

    let wrong_keyring = Arc::new(InMemoryPublisherKeyring::default());
    wrong_keyring
        .provision(
            publisher.clone(),
            PublisherPublicKey::from_bytes(
                SigningKey::from_bytes(&[32; 32]).verifying_key().to_bytes(),
            ),
        )
        .unwrap();
    let wrong_registry = Arc::new(InMemoryCapabilityRegistry::default());
    assert!(EcosystemHarnessRuntime::new_with_keyring(
        SqliteEventStore::open(&database, StoreOptions::default()).unwrap(),
        owner(),
        policy(),
        wrong_registry.clone(),
        wrong_keyring,
    )
    .is_err());
    assert!(CapabilityRegistry::resolve_toolset(
        wrong_registry.as_ref(),
        &resolve_context(&["skill:m5-harness:v1"]),
    )
    .unwrap()
    .items
    .is_empty());

    let policy_keyring = Arc::new(InMemoryPublisherKeyring::default());
    policy_keyring
        .provision(
            publisher,
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();
    let mut changed_policy = policy();
    changed_policy.version = p::Version(2);
    assert!(EcosystemHarnessRuntime::new_with_keyring(
        SqliteEventStore::open(&database, StoreOptions::default()).unwrap(),
        owner(),
        changed_policy,
        Arc::new(InMemoryCapabilityRegistry::default()),
        policy_keyring,
    )
    .is_err());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn s91_restart_with_enabled_projection_but_missing_archive_fails_closed() {
    let key = SigningKey::from_bytes(&[33; 32]);
    let keyring = Arc::new(InMemoryPublisherKeyring::default());
    let publisher = p::CapabilityPublisherRef("publisher:m5-harness".into());
    keyring
        .provision(
            publisher,
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let grant = grant(&key, 1, p::CapabilityPublisherStatus::Active);
    let package = signed_package(&key, 1);
    let admission = Ed25519CapabilityPackageVerifier::new(keyring.clone())
        .verify(&package, &grant, &policy(), &[], now_ms())
        .unwrap();
    let aggregate = p::EcosystemAggregateRef("ecosystem".into());
    let event = |id: &str, payload: p::EventPayload, owner_event: bool| {
        p::Event::new(
            p::EventId(id.into()),
            p::RunId("run:m5-missing-archive".into()),
            None,
            payload,
            p::M5_SCHEMA_VERSION,
            now_ms(),
            p::Provenance {
                source: if owner_event {
                    p::Source::OwnerControl
                } else {
                    p::Source::Internal
                },
                actor: if owner_event {
                    p::Actor::Owner
                } else {
                    p::Actor::System
                },
                trust_tier: if owner_event {
                    p::TrustTier::OwnerInput
                } else {
                    p::TrustTier::VerifiedProcess
                },
                caused_by: None,
            },
        )
    };
    store
        .append_ecosystem_expected(
            event(
                "event:m5-missing-archive-publisher",
                p::EventPayload::CapabilityPublisherChanged(p::CapabilityPublisherChangedPayload {
                    grant,
                    previous: None,
                    committed_version: p::EcosystemAggregateVersion {
                        schema_version: p::M5_SCHEMA_VERSION,
                        value: 1,
                    },
                }),
                true,
            ),
            &aggregate,
            p::EcosystemAggregateVersion::zero(),
        )
        .unwrap();
    store
        .append_ecosystem_expected(
            event(
                "event:m5-missing-archive-admission",
                p::EventPayload::CapabilityPackageAdmitted(p::CapabilityPackageAdmittedPayload {
                    admission: admission.clone(),
                    committed_version: p::EcosystemAggregateVersion {
                        schema_version: p::M5_SCHEMA_VERSION,
                        value: 2,
                    },
                }),
                false,
            ),
            &aggregate,
            p::EcosystemAggregateVersion {
                schema_version: p::M5_SCHEMA_VERSION,
                value: 1,
            },
        )
        .unwrap();
    for (index, (from, to)) in [
        (
            p::CapabilityLifecycleState::Admitted,
            p::CapabilityLifecycleState::Installed,
        ),
        (
            p::CapabilityLifecycleState::Installed,
            p::CapabilityLifecycleState::Enabled,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let expected = p::EcosystemAggregateVersion {
            schema_version: p::M5_SCHEMA_VERSION,
            value: 2 + index as u64,
        };
        store
            .append_ecosystem_expected(
                event(
                    &format!("event:m5-missing-archive-state-{index}"),
                    p::EventPayload::CapabilityPackageStateChanged(
                        p::CapabilityPackageStateChangedPayload {
                            change: p::CapabilityPackageStateChange {
                                schema_version: p::M5_SCHEMA_VERSION,
                                plan: p::CapabilityInstallPlanRef(format!(
                                    "plan:m5-missing-archive-{index}"
                                )),
                                approval: p::ApprovalId(format!(
                                    "approval:m5-missing-archive-{index}"
                                )),
                                package: admission.package.clone(),
                                release: admission.release.clone(),
                                from,
                                to,
                                active_generation: index as u64 + 1,
                                reason: p::ReasonRef("test restart boundary".into()),
                                external_effects_reverted: false,
                            },
                            committed_version: expected.next().unwrap(),
                        },
                    ),
                    true,
                ),
                &aggregate,
                expected,
            )
            .unwrap();
    }
    assert_eq!(
        EcosystemProjection::package_state(&store, &admission.package)
            .unwrap()
            .unwrap()
            .lifecycle,
        p::CapabilityLifecycleState::Enabled
    );
    assert!(EcosystemHarnessRuntime::new_with_keyring(
        store,
        owner(),
        policy(),
        Arc::new(InMemoryCapabilityRegistry::default()),
        keyring,
    )
    .is_err());
}

#[test]
fn s89_real_registry_read_is_single_approved_untrusted_and_admission_only() {
    let key = SigningKey::from_bytes(&[23; 32]);
    let package = signed_package(&key, 1);
    let alternate = signed_package(&key, 2);
    let registry = LoopbackRegistry::start(
        serde_json::to_string(&package).unwrap(),
        serde_json::to_string(&alternate).unwrap(),
    );
    let harness = registry_harness(&registry);
    configure_reactive_publisher(&harness, &key);

    let run = submit_catalog_read(&harness, registry.endpoint("package"), "golden", true);
    wait_for_request_count(&registry, 1);
    assert_eq!(
        registry.requests(),
        1,
        "golden registry run events: {:?}",
        harness
            .stream_events(run.clone())
            .map(|event| (event.kind, event.payload))
            .collect::<Vec<_>>()
    );
    let events = harness.stream_events(run.clone()).events();
    assert_event_subsequence(
        &events,
        &[
            p::EventKind::ApprovalRequested,
            p::EventKind::RunWaiting,
            p::EventKind::ApprovalResolved,
            p::EventKind::RunResumed,
            p::EventKind::ActionPlanned,
            p::EventKind::ActionStarted,
            p::EventKind::ActionOutputDelta,
            p::EventKind::ActionCompleted,
            p::EventKind::VerificationFinished,
            p::EventKind::RunComplete,
        ],
    );
    let observed = events
        .iter()
        .filter_map(|event| match &event.payload {
            p::EventPayload::ActionOutputDelta(payload) => Some((event, payload)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].0.provenance.trust_tier, p::TrustTier::Untrusted);
    assert_eq!(observed[0].1.trust, p::TrustTier::Untrusted);
    assert!(!observed[0].1.truncated);
    assert_eq!(
        serde_json::from_str::<p::SignedCapabilityPackage>(&observed[0].1.delta).unwrap(),
        package
    );
    let completion = events
        .iter()
        .find_map(|event| match &event.payload {
            p::EventPayload::ActionCompleted(payload) => Some((event, payload)),
            _ => None,
        })
        .unwrap();
    assert_eq!(completion.0.provenance.trust_tier, p::TrustTier::Untrusted);
    let receipt = completion.1.receipt.as_ref().unwrap();
    assert_eq!(receipt.trust, p::TrustTier::Untrusted);
    assert_eq!(receipt.effect, p::EffectStatus::Observed);
    assert_eq!(
        receipt.content_digest,
        Some(p::sha256_content_digest(observed[0].1.delta.as_bytes()))
    );

    let admission = harness
        .admit_capability_package(
            p::RunId("run:m5-real-registry-admit".into()),
            run,
            package.clone(),
            now_ms(),
            reactive_owner(),
        )
        .unwrap();
    assert_eq!(admission.package_digest, package.package_digest);
    let package_registry = harness.ecosystem_package_registry();
    assert!(package_registry
        .active(&package.manifest.package)
        .unwrap()
        .is_none());
    assert!(CapabilityRegistry::resolve_toolset(
        package_registry.inner().as_ref(),
        &resolve_context(&["skill:m5-harness:v1"]),
    )
    .unwrap()
    .items
    .is_empty());
    let snapshot = harness
        .capability_ecosystem_snapshot(package.manifest.scope.clone())
        .unwrap();
    assert_eq!(snapshot.admissions, vec![admission.reference]);
    assert!(snapshot.packages.is_empty());
}

#[test]
fn s89_tampered_extra_bytes_and_unapproved_refetch_fail_closed() {
    let key = SigningKey::from_bytes(&[24; 32]);
    let package = signed_package(&key, 1);
    let alternate = signed_package(&key, 2);
    let registry = LoopbackRegistry::start(
        serde_json::to_string(&package).unwrap(),
        serde_json::to_string(&alternate).unwrap(),
    );
    let harness = registry_harness(&registry);
    configure_reactive_publisher(&harness, &key);

    let extra = submit_catalog_read(&harness, registry.endpoint("extra"), "extra", true);
    wait_for_request_count(&registry, 1);
    assert_eq!(
        registry.requests(),
        1,
        "extra response run events: {:?}",
        harness
            .stream_events(extra.clone())
            .map(|event| (event.kind, event.payload))
            .collect::<Vec<_>>()
    );
    assert!(harness
        .admit_capability_package(
            p::RunId("run:m5-extra-admit".into()),
            extra,
            package.clone(),
            now_ms(),
            reactive_owner(),
        )
        .is_err());

    let unapproved =
        submit_catalog_read(&harness, registry.endpoint("package"), "unapproved", false);
    thread::sleep(Duration::from_millis(20));
    assert_eq!(registry.requests(), 1);
    assert!(harness
        .admit_capability_package(
            p::RunId("run:m5-unapproved-admit".into()),
            unapproved,
            package.clone(),
            now_ms(),
            reactive_owner(),
        )
        .is_err());
    assert!(harness
        .capability_ecosystem_snapshot(p::Scope("workspace:m5-harness".into()))
        .unwrap()
        .admissions
        .is_empty());
}

#[test]
fn s89_catalog_evidence_cannot_mix_approvals_or_action_intents() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let runtime = EcosystemHarnessRuntime::new(
        store.clone(),
        owner(),
        policy(),
        Arc::new(InMemoryCapabilityRegistry::default()),
    )
    .unwrap();
    let key = SigningKey::from_bytes(&[25; 32]);
    runtime
        .configure_publisher_key(
            p::CapabilityPublisherRef("publisher:m5-harness".into()),
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();
    runtime
        .provision_capability_publisher(
            p::RunId("run:m5-evidence-publisher".into()),
            grant(&key, 1, p::CapabilityPublisherStatus::Active),
            None,
            p::EcosystemAggregateVersion::zero(),
            owner(),
        )
        .unwrap();

    let approval_mix = signed_package(&key, 1);
    let approval_run = "run:m5-catalog-approval-mix";
    append_catalog_evidence(&store, approval_run, &approval_mix);
    append_catalog_noise(
        &store,
        approval_run,
        "approval",
        p::EventPayload::ApprovalResolved(p::ApprovalResolvedPayload {
            approval_id: p::ApprovalId("approval:unrelated".into()),
            outcome: p::ApprovalOutcome::Granted,
            grant_ref: Some(p::ApprovalGrantRef("grant:unrelated".into())),
        }),
        p::TrustTier::VerifiedProcess,
    );
    assert!(runtime
        .admit_capability_package(
            p::RunId("run:m5-admit-approval-mix".into()),
            p::RunId(approval_run.into()),
            approval_mix,
            now_ms(),
            owner(),
        )
        .is_err());

    let intent_mix = signed_package(&key, 2);
    let intent_run = "run:m5-catalog-intent-mix";
    append_catalog_evidence(&store, intent_run, &intent_mix);
    append_catalog_noise(
        &store,
        intent_run,
        "intent",
        p::EventPayload::ActionOutputDelta(p::ActionOutputDeltaPayload {
            intent_id: p::ActionId("action:unrelated".into()),
            backend: p::BackendKind::AppApi,
            scope: intent_mix.manifest.scope.clone(),
            delta: String::new(),
            truncated: false,
            trust: p::TrustTier::Untrusted,
            content_ref: None,
            remote_lease: None,
        }),
        p::TrustTier::Untrusted,
    );
    assert!(runtime
        .admit_capability_package(
            p::RunId("run:m5-admit-intent-mix".into()),
            p::RunId(intent_run.into()),
            intent_mix,
            now_ms(),
            owner(),
        )
        .is_err());
    assert!(runtime
        .capability_ecosystem_snapshot(p::Scope("workspace:m5-harness".into()))
        .unwrap()
        .admissions
        .is_empty());
}

fn append_catalog_noise(
    store: &SqliteEventStore,
    run: &str,
    suffix: &str,
    payload: p::EventPayload,
    trust_tier: p::TrustTier,
) {
    store
        .append(p::Event::new(
            p::EventId(format!("event:{run}:noise:{suffix}")),
            p::RunId(run.into()),
            None,
            payload,
            p::M5_SCHEMA_VERSION,
            now_ms(),
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier,
                caused_by: None,
            },
        ))
        .unwrap();
}

fn configure_reactive_publisher(harness: &ReactiveHarness, key: &SigningKey) {
    harness
        .configure_ecosystem_publisher_key(
            p::CapabilityPublisherRef("publisher:m5-harness".into()),
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();
    harness
        .provision_capability_publisher(
            p::RunId("run:m5-real-registry-publisher".into()),
            grant(key, 1, p::CapabilityPublisherStatus::Active),
            None,
            p::EcosystemAggregateVersion::zero(),
            reactive_owner(),
        )
        .unwrap();
}

fn registry_harness(registry: &LoopbackRegistry) -> ReactiveHarness {
    ReactiveHarness::project_owned_app_api(ProjectAppApiRuntimeConfig {
        schema_version: p::M5_SCHEMA_VERSION,
        connector: p::ProviderId("connector:m5-registry".into()),
        base_url: registry.base_url(),
        schema_digest: p::SchemaDigest(EMPTY_SHA256.into()),
        credential_ref: None,
        scope: p::Scope("workspace:m5-harness".into()),
        capability: p::CapabilityRef("connector:m5-registry:read".into()),
        permission: p::PermissionRef("permission:m5-registry:read".into()),
        allowed_mutations: Vec::new(),
        requests_per_minute: 8,
        timeout: p::DurationMs(2_000),
        max_response_bytes: 65_536,
        envelope: registry_envelope(),
        contents: Vec::new(),
    })
    .unwrap()
}

fn registry_envelope() -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::M5_SCHEMA_VERSION,
        scope: p::Scope("workspace:m5-harness".into()),
        capability: p::CapabilitySet {
            schema_version: p::M5_SCHEMA_VERSION,
            capabilities: vec![p::CapabilityRef("connector:m5-registry:read".into())],
            permissions: vec![p::PermissionRef("permission:m5-registry:read".into())],
        },
        action_type: vec![p::ActionType::Observe],
        risk_limit: p::Risk::High,
        approval_rule: p::ApprovalRule::Ask,
        budget: p::Budget("units:1".into()),
        timebox: p::Timebox {
            schema_version: p::M5_SCHEMA_VERSION,
            starts_at: 1,
            expires_at: i64::MAX,
            max_turns: 1,
        },
        rollback: p::RollbackReq {
            schema_version: p::M5_SCHEMA_VERSION,
            required: false,
            boundary: None,
        },
    }
}

fn submit_catalog_read(
    harness: &ReactiveHarness,
    endpoint: String,
    suffix: &str,
    approve: bool,
) -> p::RunId {
    let now = now_ms();
    let session = p::SessionId(format!("session:m5-registry:{suffix}"));
    let run = harness
        .submit_action(
            p::RunRequest {
                schema_version: p::M5_SCHEMA_VERSION,
                source: p::Source::UserTurn,
                session: p::SessionRef(session.0.clone()),
                agent_profile: p::AgentProfileRef("agent:forme".into()),
                input: p::RunInput("read one exact capability package".into()),
                budget: Some(p::Budget("units:1".into())),
                idempotency_key: Some(p::IdempotencyKey(format!("m5-registry-read:{suffix}"))),
            },
            p::ActionIntent {
                schema_version: p::M5_SCHEMA_VERSION,
                intent_id: p::ActionId(format!("intent:m5-registry:{suffix}")),
                source: p::Source::UserTurn,
                goal: p::GoalRef("observe one exact registry response".into()),
                backend_hint: p::BackendKind::AppApi,
                capability_ref: p::CapabilityRef("connector:m5-registry:read".into()),
                action_type: p::ActionType::Observe,
                scope: p::Scope("workspace:m5-harness".into()),
                risk_hint: p::Risk::High,
                expected_effect: p::ExpectedEffect::Outward,
                rollback_expectation: p::RollbackBoundary(
                    "external registry read cannot be undone".into(),
                ),
                parameters: p::ActionParameters::AppApi(p::AppApiActionSpec {
                    schema_version: p::M5_SCHEMA_VERSION,
                    connector: p::ProviderId("connector:m5-registry".into()),
                    endpoint,
                    schema_digest: p::SchemaDigest(EMPTY_SHA256.into()),
                    credential: None,
                    operation: p::AppApiOperation::Read,
                    timeout: p::DurationMs(2_000),
                    participant: None,
                    representation: None,
                    disclosure_request: None,
                }),
                requested_permissions: vec![p::PermissionRef("permission:m5-registry:read".into())],
                requested_at: now,
                estimated_output_bytes: 65_536,
                estimated_duration: p::DurationMs(2_000),
            },
            registry_envelope(),
            Vec::new(),
        )
        .unwrap();
    let pending = harness.pending_approvals(session).unwrap();
    assert_eq!(
        pending.len(),
        1,
        "catalog run events: {:?}",
        harness
            .stream_events(run.clone())
            .map(|event| (event.kind, event.payload))
            .collect::<Vec<_>>()
    );
    if approve {
        let request = &pending[0];
        harness
            .resume(
                run.clone(),
                ResumeInput::Approval(ApprovalGrant {
                    schema_version: p::M5_SCHEMA_VERSION,
                    approval_id: request.approval_id.clone(),
                    outcome: p::ApprovalOutcome::Granted,
                    granted_scope: GrantScope::OneShot,
                    approver: reactive_owner(),
                    bound_plan_digest: request.plan_digest.clone(),
                    policy_version: request.policy_version,
                    tool_schema_version: request.tool_schema_version,
                    nonce: p::Nonce(format!("nonce:m5-registry:{suffix}")),
                    use_by: request.expires_at.saturating_sub(1),
                }),
            )
            .unwrap();
        let _ = harness.wait(run.clone()).unwrap();
    }
    run
}

fn assert_event_subsequence(events: &[p::Event], required: &[p::EventKind]) {
    let kinds = events.iter().map(|event| event.kind).collect::<Vec<_>>();
    let mut cursor = 0;
    for kind in required {
        let offset = kinds[cursor..]
            .iter()
            .position(|candidate| candidate == kind)
            .unwrap_or_else(|| panic!("missing {kind:?} after event index {cursor}"));
        cursor += offset + 1;
    }
}

fn wait_for_request_count(registry: &LoopbackRegistry, expected: usize) {
    for _ in 0..100 {
        if registry.requests() == expected {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

struct LoopbackRegistry {
    address: SocketAddr,
    requests: Arc<AtomicUsize>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LoopbackRegistry {
    fn start(package: String, alternate: String) -> Self {
        let listener = bind_registry_listener();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = requests.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let thread = thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if stop.load(Ordering::SeqCst) {
                            break;
                        }
                        stream
                            .set_nonblocking(false)
                            .expect("accepted registry streams must use blocking reads");
                        let package = package.clone();
                        let alternate = alternate.clone();
                        let observed = observed.clone();
                        thread::spawn(move || {
                            serve_registry_request(
                                &mut stream,
                                &package,
                                &alternate,
                                observed.as_ref(),
                            );
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(2)),
                }
            }
        });
        Self {
            address,
            requests,
            stopped,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}/registry/", self.address)
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.base_url())
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

fn bind_registry_listener() -> TcpListener {
    TcpListener::bind("127.0.0.1:0")
        .expect("the operating system must allocate a loopback registry port")
}

impl Drop for LoopbackRegistry {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve_registry_request(
    stream: &mut TcpStream,
    package: &str,
    alternate: &str,
    requests: &AtomicUsize,
) {
    let Some(path) = read_request_path(stream) else {
        return;
    };
    if path.ends_with("/ready") {
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nready");
        return;
    }
    requests.fetch_add(1, Ordering::SeqCst);
    if path.ends_with("/redirect") {
        let _ = stream.write_all(
            b"HTTP/1.1 302 Found\r\nLocation: /registry/package\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        return;
    }
    let body = if path.ends_with("/tamper") {
        alternate.as_bytes().to_vec()
    } else if path.ends_with("/extra") {
        format!("{package}\nnot-part-of-the-package").into_bytes()
    } else {
        package.as_bytes().to_vec()
    };
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(&body);
    let _ = stream.write_all(&response);
}

fn read_request_path(stream: &mut TcpStream) -> Option<String> {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 2_048];
    loop {
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if bytes.len() > 64 * 1_024 {
            return None;
        }
    }
    let request = String::from_utf8(bytes).ok()?;
    request
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)
        .map(str::to_owned)
}

fn resolve_context(capabilities: &[&str]) -> p::ResolveContext {
    let capabilities = capabilities
        .iter()
        .map(|value| p::CapabilityRef((*value).into()))
        .collect::<Vec<_>>();
    p::ResolveContext {
        schema_version: p::M5_SCHEMA_VERSION,
        session: p::SessionId("session:m5-harness".into()),
        toolset: p::ToolsetRef("toolset:m5-harness".into()),
        envelope: p::AutonomyEnvelope {
            schema_version: p::M5_SCHEMA_VERSION,
            scope: p::Scope("workspace:m5-harness".into()),
            capability: p::CapabilitySet {
                schema_version: p::M5_SCHEMA_VERSION,
                capabilities: capabilities.clone(),
                permissions: vec![p::PermissionRef("permission:read".into())],
            },
            action_type: vec![p::ActionType::Analyze],
            risk_limit: p::Risk::Low,
            approval_rule: p::ApprovalRule::Ask,
            budget: p::Budget("budget:m5-harness".into()),
            timebox: p::Timebox {
                schema_version: p::M5_SCHEMA_VERSION,
                starts_at: 1,
                expires_at: i64::MAX,
                max_turns: 1,
            },
            rollback: p::RollbackReq {
                schema_version: p::M5_SCHEMA_VERSION,
                required: false,
                boundary: None,
            },
        },
        policy_allowed_providers: Vec::new(),
        policy_allowed_capabilities: capabilities,
    }
}
