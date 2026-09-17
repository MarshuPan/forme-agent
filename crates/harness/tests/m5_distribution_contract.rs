use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use forme_approval::{ApprovalGrant, GrantScope};
use forme_capabilities::{
    AppApiConnectorRegistry, CapabilityRegistry, InMemoryPublisherKeyring, ProjectAppApiConnector,
    PublisherPublicKey,
};
use forme_eval::{
    M5AdmissionArtifact, M5ApprovalEvidence, M5ArtifactBundle, M5ArtifactStore,
    M5DistributionArtifact, M5InstallArtifact, M5PublisherArtifact, M5TraceArtifact,
};
use forme_execution::{
    transport_identity_digest, AppApiBackend, ExecutionBackendRegistry, ExecutorAdmissionProfile,
    FileExecutorLedger, GuardedRemoteExecutor, HttpAppApiDriver, InMemoryContentResolver,
    OutputBudget, RejectingSecretResolver, RemoteExecutorDriver, RemoteReceiptRepository,
    RemoteReceiptSource, RemoteTransport, SystemRemoteClock, TlsIdentityFiles,
    TlsRemoteClientConfig, TlsRemoteTransport,
};
use forme_harness::{
    capability_distribution_operation, AgentHarness, CapabilityPackageLedgerGroundTruth,
    CapabilityPackageReceiver, EcosystemGatewayControl, FederationActionGateway,
    FederationGatewayControl, GovernanceConfig, HarnessActionIngress, HarnessConfig,
    ReactiveHarness, ResumeInput,
};
use forme_models::{
    Cost, ModelCapability, ModelProfile, ModelStrength, RateLimit, ScriptedModelProvider, Url,
};
use forme_policy::{
    ActionMatcher, ArgMatcher, DelegationGrant, DelegationSubject, PolicyLayer, PolicyLayerSource,
    PolicyRule,
};
use forme_protocol as p;
use forme_store::{
    EcosystemEventStore, EcosystemProjection, EcosystemRuntimeLedger, EventStore, SqliteEventStore,
    StoreOptions,
};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair, KeyUsagePurpose,
};

const OWNER: &str = "local-owner";
const SCOPE: &str = "workspace:m5-distribution";
const CAPABILITY: &str = "forme.ecosystem.package-receiver";
const PROFILE: &str = "profile:m5-package-executor";
const EMPTY_SHA256: &str =
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

fn temporary_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "forme-m5-distribution-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn ecosystem_policy() -> p::CapabilityAdmissionPolicy {
    p::CapabilityAdmissionPolicy {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPolicyRef("policy:ecosystem-default".into()),
        version: p::Version(1),
        allowed_kinds: vec![
            p::CapabilityPackageKind::Connector,
            p::CapabilityPackageKind::Plugin,
            p::CapabilityPackageKind::Skill,
            p::CapabilityPackageKind::McpServer,
            p::CapabilityPackageKind::AgentProfile,
        ],
        allowed_licenses: vec!["Apache-2.0".into(), "MIT".into()],
        max_package_bytes: 1_048_576,
        max_dependencies: 32,
        max_depth: 8,
        allow_network: false,
        allow_hooks: false,
    }
}

fn model_profile() -> ModelProfile {
    ModelProfile {
        schema_version: p::M5_SCHEMA_VERSION,
        provider: p::ProviderId("m5-distribution-test".into()),
        model: "m5-distribution-test".into(),
        base_url: Url::parse("https://models.invalid/v1").unwrap(),
        capability: ModelCapability {
            schema_version: p::M5_SCHEMA_VERSION,
            context_window: 16_384,
            tool_use: true,
            strength: ModelStrength::Standard,
        },
        cost: Cost {
            schema_version: p::M5_SCHEMA_VERSION,
            input_microunits_per_million: 1,
            output_microunits_per_million: 1,
        },
        rate_limit: RateLimit {
            schema_version: p::M5_SCHEMA_VERSION,
            requests_per_minute: 60,
            tokens_per_minute: 100_000,
        },
        credential_ref: p::CredentialRef("secret:m5-test-model".into()),
    }
}

fn autonomy(now: i64) -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::M5_SCHEMA_VERSION,
        scope: p::Scope(SCOPE.into()),
        capability: p::CapabilitySet {
            schema_version: p::M5_SCHEMA_VERSION,
            capabilities: vec![p::CapabilityRef(CAPABILITY.into())],
            permissions: vec![p::PermissionRef("permission:remote-execute".into())],
        },
        action_type: vec![p::ActionType::ExternalCommit],
        risk_limit: p::Risk::High,
        approval_rule: p::ApprovalRule::Allow,
        budget: p::Budget("units:2".into()),
        timebox: p::Timebox {
            schema_version: p::M5_SCHEMA_VERSION,
            starts_at: now.saturating_sub(1_000),
            expires_at: now.saturating_add(600_000),
            max_turns: 2,
        },
        rollback: p::RollbackReq {
            schema_version: p::M5_SCHEMA_VERSION,
            required: true,
            boundary: Some(p::RollbackBoundary(
                "registry visibility only; prior external effects remain historical".into(),
            )),
        },
    }
}

fn governance(now: i64) -> GovernanceConfig {
    let envelope = autonomy(now);
    GovernanceConfig {
        schema_version: p::M5_SCHEMA_VERSION,
        layers: vec![PolicyLayer {
            schema_version: p::M5_SCHEMA_VERSION,
            source: PolicyLayerSource::User,
            rules: vec![PolicyRule {
                schema_version: p::M5_SCHEMA_VERSION,
                matcher: ActionMatcher {
                    backend: Some(p::BackendKind::Remote),
                    capability: Some(p::CapabilityRef(CAPABILITY.into())),
                    action_type: Some(p::ActionType::ExternalCommit),
                    parameters: ArgMatcher::Any,
                },
                effect: p::PolicyDecision::Allow,
                scope: p::Scope(SCOPE.into()),
            }],
        }],
        visible_capabilities: vec![p::CapabilityRef(CAPABILITY.into())],
        granted_permissions: vec![p::PermissionRef("permission:remote-execute".into())],
        allowed_scopes: vec![p::Scope(SCOPE.into())],
        shell_allowlist: Vec::new(),
        file_roots: Vec::new(),
        mcp_allowlist: Vec::new(),
        external: forme_policy::ExternalPolicyLimits::default(),
        network_allowed: true,
        sandbox_available: true,
        delegation: Some(DelegationGrant {
            schema_version: p::M5_SCHEMA_VERSION,
            subject: DelegationSubject::Owner,
            envelope: envelope.clone(),
            granted_by: p::Actor::Owner,
            audit_ref: p::EventId("delegation:m5-distribution".into()),
        }),
        envelope: Some(envelope),
    }
}

fn build_harness(store: SqliteEventStore, now: i64) -> ReactiveHarness {
    let profile = model_profile();
    let model = Arc::new(ScriptedModelProvider::new(profile.clone(), Vec::new()).unwrap());
    let mut config = HarnessConfig::for_model(&profile);
    config.policy_version = p::Version(7);
    config.tool_schema_version = p::Version(11);
    config.workspace = p::WorkspaceRef(SCOPE.into());
    config.toolset_ref = p::ToolsetRef("toolset:m5-distribution".into());
    ReactiveHarness::new(
        store,
        model,
        Arc::new(ExecutionBackendRegistry::default()),
        governance(now),
        config,
    )
    .unwrap()
}

fn catalog_envelope() -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::M5_SCHEMA_VERSION,
        scope: p::Scope(SCOPE.into()),
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

fn catalog_harness(store: SqliteEventStore, base_url: String) -> ReactiveHarness {
    let connector_id = p::ProviderId("connector:m5-registry".into());
    let schema_digest = p::SchemaDigest(EMPTY_SHA256.into());
    let connector = ProjectAppApiConnector::new(
        connector_id.clone(),
        base_url.clone(),
        schema_digest.clone(),
        None,
        p::Scope(SCOPE.into()),
        p::CapabilityRef("connector:m5-registry:read".into()),
        p::PermissionRef("permission:m5-registry:read".into()),
        Vec::new(),
        8,
        p::DurationMs(2_000),
    )
    .unwrap();
    let connectors = Arc::new(AppApiConnectorRegistry::with_connectors(vec![connector]).unwrap());
    connectors.configure(connector_id.clone()).unwrap();
    connectors.enable(connector_id.clone()).unwrap();
    connectors
        .bind_trust(
            connector_id.clone(),
            p::TrustTier::ApprovedSource,
            p::Actor::Owner,
        )
        .unwrap();
    connectors
        .grant(connector_id.clone(), catalog_envelope())
        .unwrap();
    let driver = Arc::new(
        HttpAppApiDriver::new(
            connector_id.clone(),
            base_url.clone(),
            schema_digest.clone(),
            None,
            Vec::new(),
            8,
            p::DurationMs(2_000),
            65_536,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap(),
    );
    let backends = Arc::new(ExecutionBackendRegistry::default());
    backends
        .register(Arc::new(
            AppApiBackend::new(
                connector_id.clone(),
                driver,
                Arc::new(RejectingSecretResolver),
                Arc::new(InMemoryContentResolver::default()),
                OutputBudget::truncate_at(65_536),
                p::DurationMs(2_000),
            )
            .unwrap(),
        ))
        .unwrap();
    let envelope = catalog_envelope();
    let governance = GovernanceConfig {
        schema_version: p::M5_SCHEMA_VERSION,
        layers: vec![PolicyLayer {
            schema_version: p::M5_SCHEMA_VERSION,
            source: PolicyLayerSource::User,
            rules: vec![PolicyRule {
                schema_version: p::M5_SCHEMA_VERSION,
                matcher: ActionMatcher {
                    backend: Some(p::BackendKind::AppApi),
                    capability: Some(p::CapabilityRef("connector:m5-registry:read".into())),
                    action_type: Some(p::ActionType::Observe),
                    parameters: ArgMatcher::Any,
                },
                effect: p::PolicyDecision::Ask,
                scope: p::Scope(SCOPE.into()),
            }],
        }],
        visible_capabilities: vec![p::CapabilityRef("connector:m5-registry:read".into())],
        granted_permissions: vec![p::PermissionRef("permission:m5-registry:read".into())],
        allowed_scopes: vec![p::Scope(SCOPE.into())],
        shell_allowlist: Vec::new(),
        file_roots: Vec::new(),
        mcp_allowlist: Vec::new(),
        external: forme_policy::ExternalPolicyLimits {
            schema_version: p::M5_SCHEMA_VERSION,
            browser_origins: Vec::new(),
            computer_surfaces: Vec::new(),
            pty_programs: Vec::new(),
            pty_roots: Vec::new(),
            app_api_connectors: vec![forme_policy::AppApiConnectorLimit {
                schema_version: p::M5_SCHEMA_VERSION,
                connector: connector_id,
                base_url,
                schema_digest,
                credential_ref: None,
                allowed_mutations: Vec::new(),
                max_timeout: p::DurationMs(2_000),
            }],
        },
        network_allowed: true,
        sandbox_available: true,
        delegation: Some(DelegationGrant {
            schema_version: p::M5_SCHEMA_VERSION,
            subject: DelegationSubject::Owner,
            envelope: envelope.clone(),
            granted_by: p::Actor::Owner,
            audit_ref: p::EventId("delegation:m5-registry".into()),
        }),
        envelope: Some(envelope),
    };
    let profile = model_profile();
    let mut config = HarnessConfig::for_model(&profile);
    config.workspace = p::WorkspaceRef(SCOPE.into());
    config.toolset_ref = p::ToolsetRef("toolset:m5-registry".into());
    ReactiveHarness::new(
        store,
        Arc::new(ScriptedModelProvider::new(profile, Vec::new()).unwrap()),
        backends,
        governance,
        config,
    )
    .unwrap()
    .with_capability_rechecker(connectors)
}

fn fetch_catalog_package(harness: &ReactiveHarness, endpoint: String, now: i64) -> p::RunId {
    let session = p::SessionId("session:m5-golden-registry".into());
    let run = harness
        .submit_action(
            p::RunRequest {
                schema_version: p::M5_SCHEMA_VERSION,
                source: p::Source::UserTurn,
                session: p::SessionRef(session.0.clone()),
                agent_profile: p::AgentProfileRef("agent:forme".into()),
                input: p::RunInput("read one exact capability package".into()),
                budget: Some(p::Budget("units:1".into())),
                idempotency_key: Some(p::IdempotencyKey("m5-golden-registry-read".into())),
            },
            p::ActionIntent {
                schema_version: p::M5_SCHEMA_VERSION,
                intent_id: p::ActionId("intent:m5-golden-registry".into()),
                source: p::Source::UserTurn,
                goal: p::GoalRef("observe one exact registry response".into()),
                backend_hint: p::BackendKind::AppApi,
                capability_ref: p::CapabilityRef("connector:m5-registry:read".into()),
                action_type: p::ActionType::Observe,
                scope: p::Scope(SCOPE.into()),
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
            catalog_envelope(),
            Vec::new(),
        )
        .unwrap();
    let pending = harness.pending_approvals(session).unwrap();
    assert_eq!(pending.len(), 1);
    let request = &pending[0];
    harness
        .resume(
            run.clone(),
            ResumeInput::Approval(ApprovalGrant {
                schema_version: p::M5_SCHEMA_VERSION,
                approval_id: request.approval_id.clone(),
                outcome: p::ApprovalOutcome::Granted,
                granted_scope: GrantScope::OneShot,
                approver: p::VerifiedPrincipal(OWNER.into()),
                bound_plan_digest: request.plan_digest.clone(),
                policy_version: request.policy_version,
                tool_schema_version: request.tool_schema_version,
                nonce: p::Nonce("nonce:m5-golden-registry".into()),
                use_by: request.expires_at.saturating_sub(1),
            }),
        )
        .unwrap();
    harness.wait(run.clone()).unwrap();
    run
}

fn signed_package(key: &SigningKey) -> p::SignedCapabilityPackage {
    let mut package = p::SignedCapabilityPackage {
        schema_version: p::M5_SCHEMA_VERSION,
        manifest: p::CapabilityPackageManifest {
            schema_version: p::M5_SCHEMA_VERSION,
            package: p::CapabilityPackageRef("package:m5-distribution".into()),
            release: p::CapabilityReleaseRef("release:m5-distribution:v1".into()),
            version: p::Version(1),
            kind: p::CapabilityPackageKind::Skill,
            publisher: p::CapabilityPublisherRef("publisher:m5-distribution".into()),
            scope: p::Scope(SCOPE.into()),
            contributions: vec![p::CapabilityContributionDescriptor {
                schema_version: p::M5_SCHEMA_VERSION,
                kind: p::CapabilityPackageKind::Skill,
                capability: p::CapabilityRef("skill:m5-distributed".into()),
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
            relative_path: "skills/distributed.txt".into(),
            content: "governed distributed skill".into(),
            digest: p::SchemaDigest(EMPTY_SHA256.into()),
        }],
        package_digest: p::SchemaDigest(EMPTY_SHA256.into()),
        signature: p::PackageSignature(format!("ed25519:{}", "00".repeat(64))),
    };
    package.refresh_digests().unwrap();
    package.manifest.contributions[0].payload_digest = package.resources[0].digest.clone();
    package.refresh_digests().unwrap();
    let signature = key
        .sign(&p::sha256_digest_bytes(&package.package_digest).unwrap())
        .to_bytes();
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

fn publisher_grant(key: &SigningKey, now: i64) -> p::CapabilityPublisherGrant {
    let public = PublisherPublicKey::from_bytes(key.verifying_key().to_bytes());
    p::CapabilityPublisherGrant {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPublisherGrantRef("grant:m5-distribution:v1".into()),
        publisher: p::CapabilityPublisherRef("publisher:m5-distribution".into()),
        public_key_digest: public.digest(),
        allowed_kinds: vec![p::CapabilityPackageKind::Skill],
        scope: p::Scope(SCOPE.into()),
        expires_at: now.saturating_add(3_600_000),
        version: p::Version(1),
        status: p::CapabilityPublisherStatus::Active,
    }
}

fn append_catalog_evidence(
    store: &SqliteEventStore,
    run: &p::RunId,
    package: &p::SignedCapabilityPackage,
    now: i64,
) {
    let intent = p::ActionId(format!("action:{}", run.0));
    let approval = p::ApprovalId(format!("approval:{}", run.0));
    let body = serde_json::to_string(package).unwrap();
    let body_digest = p::sha256_content_digest(body.as_bytes());
    let done = p::DoneContractRef(format!("done-contract:{}", run.0));
    let events = vec![
        p::EventPayload::ApprovalRequested(p::ApprovalRequestedPayload {
            approval_id: approval.clone(),
            action_summary: p::ActionSummary("read one registry package".into()),
            risk: p::Risk::High,
            scope: package.manifest.scope.clone(),
            rollback_boundary: p::RollbackBoundary("external read cannot be undone".into()),
            expires_at: now.saturating_add(60_000),
            choices: vec![p::ApprovalChoice("approve once".into())],
            requested_permissions: vec![p::PermissionRef("permission:network-read".into())],
            affected_resources: vec![p::ResourceRef("registry:loopback".into())],
        }),
        p::EventPayload::ApprovalResolved(p::ApprovalResolvedPayload {
            approval_id: approval.clone(),
            outcome: p::ApprovalOutcome::Granted,
            grant_ref: Some(p::ApprovalGrantRef(format!("grant:{}", run.0))),
        }),
        p::EventPayload::ActionPlanned(p::ActionPlannedPayload {
            intent_id: intent.clone(),
            plan_digest: p::PlanDigest(format!("plan:{}", run.0)),
            backend: p::BackendKind::AppApi,
            expected_effect: p::ExpectedEffect::Outward,
            source: p::Source::UserTurn,
            scope: package.manifest.scope.clone(),
            approval_ref: Some(approval),
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
            delta: body,
            truncated: false,
            trust: p::TrustTier::Untrusted,
            content_ref: None,
            remote_lease: None,
        }),
        p::EventPayload::ActionCompleted(p::ActionCompletedPayload {
            intent_id: intent.clone(),
            result_ref: p::ActionResultRef(format!("result:{}", run.0)),
            receipt: Some(p::ExternalActionReceipt {
                schema_version: p::M5_SCHEMA_VERSION,
                action: intent,
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
            against: done.clone(),
        }),
        p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
            verifier_kind: p::VerifierKind("deterministic".into()),
            outcome: p::VerificationOutcome::Pass,
            against: done,
        }),
        p::EventPayload::RunComplete(p::RunCompletePayload {
            stop_reason: p::StopReason("registry read verified".into()),
            result_ref: None,
        }),
    ];
    for (index, payload) in events.into_iter().enumerate() {
        let untrusted = matches!(
            payload.kind(),
            p::EventKind::ActionOutputDelta | p::EventKind::ActionCompleted
        );
        store
            .append(p::Event::new(
                p::EventId(format!("event:{}:{index}", run.0)),
                run.clone(),
                None,
                payload,
                p::M5_SCHEMA_VERSION,
                now.saturating_add(index as i64),
                if untrusted {
                    p::Provenance {
                        source: p::Source::Communication,
                        actor: p::Actor::External(p::ParticipantId("registry:fixture".into())),
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

fn federation_version(value: u64) -> p::FederationAggregateVersion {
    p::FederationAggregateVersion {
        schema_version: p::M5_SCHEMA_VERSION,
        aggregate: p::FederationAggregateRef("federation".into()),
        version: value,
    }
}

fn peer_grant(
    peer: &str,
    roles: Vec<p::FederatedPeerRole>,
    epoch: u64,
    now: i64,
) -> p::FederatedPeerGrant {
    p::FederatedPeerGrant {
        schema_version: p::M5_SCHEMA_VERSION,
        peer: p::FederatedPeerRef(peer.into()),
        owner: p::VerifiedPrincipal(OWNER.into()),
        roles,
        scopes: vec![p::Scope(SCOPE.into())],
        capabilities: vec![p::CapabilityRef(CAPABILITY.into())],
        transport_identity: p::TransportIdentityDigest(format!("sha256:identity:{peer}")),
        authority_epoch: p::AuthorityEpoch(epoch),
        grant_version: p::PeerGrantVersion(1),
        expires_at: now.saturating_add(3_600_000),
        created_by: p::OwnerControlRef(format!("owner-control:{peer}")),
    }
}

fn configure_peers(
    harness: &ReactiveHarness,
    now: i64,
) -> (p::FederatedPeerGrant, p::FederatedPeerGrant) {
    let executor = peer_grant(
        "peer:m5-executor",
        vec![p::FederatedPeerRole::Executor],
        1,
        now,
    );
    harness
        .register_federated_peer(
            p::RunId("owner-control:m5-register-executor".into()),
            executor.clone(),
            None,
            federation_version(0),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    let owner_client = peer_grant(
        "peer:m5-owner-client",
        vec![p::FederatedPeerRole::OwnerClient],
        2,
        now,
    );
    harness
        .register_federated_peer(
            p::RunId("owner-control:m5-register-owner-client".into()),
            owner_client.clone(),
            None,
            federation_version(1),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    (executor, owner_client)
}

fn provision_and_admit(
    harness: &ReactiveHarness,
    store: &SqliteEventStore,
    key: &SigningKey,
    now: i64,
) -> (p::SignedCapabilityPackage, p::CapabilityPublisherGrant) {
    let grant = publisher_grant(key, now);
    harness
        .configure_ecosystem_publisher_key(
            grant.publisher.clone(),
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();
    harness
        .provision_capability_publisher(
            p::RunId("owner-control:m5-publisher".into()),
            grant.clone(),
            None,
            p::EcosystemAggregateVersion::zero(),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    let package = signed_package(key);
    let catalog_run = p::RunId("run:m5-catalog".into());
    append_catalog_evidence(store, &catalog_run, &package, now);
    harness
        .admit_capability_package(
            p::RunId("owner-control:m5-admit".into()),
            catalog_run,
            package.clone(),
            now,
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    (package, grant)
}

fn distribution_plan(
    harness: &ReactiveHarness,
    package: &p::SignedCapabilityPackage,
) -> p::CapabilityInstallPlan {
    harness
        .prepare_capability_change(
            p::CapabilityPackageOperation::Distribute,
            package.manifest.package.clone(),
            package.manifest.release.clone(),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap()
}

fn placement(
    executor: &p::FederatedPeerGrant,
    epoch: p::AuthorityEpoch,
    envelope: &p::CapabilityPackageDistributionEnvelope,
) -> p::RemotePlacementPlan {
    let operation = capability_distribution_operation(envelope).unwrap();
    let mut placement = p::RemotePlacementPlan {
        schema_version: p::M5_SCHEMA_VERSION,
        executor: executor.peer.clone(),
        peer_grant: executor.reference().unwrap(),
        grant_version: executor.grant_version,
        authority_epoch: epoch,
        executor_profile: p::ExecutorProfileRef(PROFILE.into()),
        operation,
        digest: p::SchemaDigest(String::new()),
    };
    placement.refresh_digest().unwrap();
    placement
}

fn candidate(
    grant: &p::FederatedPeerGrant,
    placement: &p::RemotePlacementPlan,
    now: i64,
) -> p::FederatedExecutorCandidate {
    p::FederatedExecutorCandidate {
        schema_version: p::M5_SCHEMA_VERSION,
        peer: grant.peer.clone(),
        grant: grant.reference().unwrap(),
        profile: placement.executor_profile.clone(),
        scope: placement.operation.scope.clone(),
        capability: placement.operation.capability.clone(),
        expires_at: grant.expires_at,
        health: p::ExecutorHealthState::Healthy,
        health_observed_at: now,
        capability_evidence: vec![p::CapabilityEvidenceRef(
            "capability-evidence:m5-package-receiver".into(),
        )],
        failure_evidence: Vec::new(),
        managed_policy_allowed: true,
        score_basis_points: 5_000,
    }
}

fn action_intent(placement: p::RemotePlacementPlan, now: i64, suffix: &str) -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::M5_SCHEMA_VERSION,
        intent_id: p::ActionId(format!("intent:m5-distribution:{suffix}")),
        source: p::Source::UserTurn,
        goal: p::GoalRef("distribute one governed declarative package".into()),
        backend_hint: p::BackendKind::Remote,
        capability_ref: p::CapabilityRef(CAPABILITY.into()),
        action_type: p::ActionType::ExternalCommit,
        scope: p::Scope(SCOPE.into()),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: placement.operation.rollback_boundary.clone(),
        parameters: p::ActionParameters::Remote(Box::new(p::RemoteActionSpec {
            schema_version: p::M5_SCHEMA_VERSION,
            placement,
        })),
        requested_permissions: vec![p::PermissionRef("permission:remote-execute".into())],
        requested_at: now,
        estimated_output_bytes: 2_048,
        estimated_duration: p::DurationMs(2_000),
    }
}

fn run_request(suffix: &str) -> p::RunRequest {
    p::RunRequest {
        schema_version: p::M5_SCHEMA_VERSION,
        source: p::Source::UserTurn,
        session: p::SessionRef(format!("session:m5-distribution:{suffix}")),
        agent_profile: p::AgentProfileRef("agent:forme".into()),
        input: p::RunInput("distribute an admitted capability package".into()),
        budget: Some(p::Budget("units:2".into())),
        idempotency_key: Some(p::IdempotencyKey(format!("m5-distribution:{suffix}"))),
    }
}

fn owner_envelope(
    owner_client: &p::FederatedPeerGrant,
    command: &p::FederatedOwnerCommand,
    nonce: &str,
    now: i64,
) -> p::FederatedControlEnvelope {
    p::FederatedControlEnvelope {
        schema_version: p::M5_SCHEMA_VERSION,
        peer: owner_client.peer.clone(),
        session: p::FederatedSessionRef("session:m5-owner-control".into()),
        owner: p::VerifiedPrincipal(OWNER.into()),
        nonce: p::Nonce(nonce.into()),
        expires_at: now.saturating_add(60_000),
        command_digest: p::canonical_digest(command).unwrap(),
    }
}

struct ExecutorTransport {
    service: Arc<GuardedRemoteExecutor>,
    dispatches: Arc<AtomicUsize>,
    probes: Arc<AtomicUsize>,
    receipt_reads: Arc<AtomicUsize>,
    fail_next_receipt: AtomicBool,
}

impl ExecutorTransport {
    fn new(
        service: Arc<GuardedRemoteExecutor>,
        dispatches: Arc<AtomicUsize>,
        probes: Arc<AtomicUsize>,
        receipt_reads: Arc<AtomicUsize>,
        fail_next_receipt: bool,
    ) -> Self {
        Self {
            service,
            dispatches,
            probes,
            receipt_reads,
            fail_next_receipt: AtomicBool::new(fail_next_receipt),
        }
    }
}

impl RemoteTransport for ExecutorTransport {
    fn dispatch(
        &self,
        plan: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
    ) -> p::Result<p::RemoteDispatchAcceptance> {
        self.dispatches.fetch_add(1, Ordering::SeqCst);
        let receipt = self.service.execute(self.service.admit(plan, lease)?)?;
        Ok(p::RemoteDispatchAcceptance {
            schema_version: p::M5_SCHEMA_VERSION,
            dispatch: lease.dispatch.clone(),
            lease: lease.lease.clone(),
            accepted: p::RequiredTrue,
            receipt: receipt.receipt,
            authority_epoch: lease.authority_epoch,
            fence: lease.fence,
        })
    }

    fn probe(&self, request: p::RemoteProbeRequest) -> p::Result<p::RemoteProbeResult> {
        request.validate()?;
        self.probes.fetch_add(1, Ordering::SeqCst);
        self.service.probe(&request.lease)
    }

    fn cancel(&self, request: p::RemoteCancelRequest) -> p::Result<p::RemoteCancelResult> {
        request.validate()?;
        self.service.cancel(&request.lease)
    }
}

impl RemoteReceiptSource for ExecutorTransport {
    fn receipt(&self, request: p::RemoteReceiptRequest) -> p::Result<p::RemoteDriverReceipt> {
        request.validate()?;
        self.receipt_reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_next_receipt.swap(false, Ordering::SeqCst) {
            return Err(p::Error(
                "executor receipt channel is temporarily unavailable".into(),
            ));
        }
        let receipt = self.service.fetch_receipt(&request.receipt)?;
        if receipt.lease != request.lease
            || receipt.dispatch != request.dispatch
            || receipt.authority_epoch != request.authority_epoch
            || receipt.fence != request.fence
        {
            return Err(p::Error(
                "executor receipt request changed its authority binding".into(),
            ));
        }
        Ok(receipt)
    }
}

fn executor_service(
    executor: &p::FederatedPeerGrant,
    publisher: &p::CapabilityPublisherGrant,
    key: &SigningKey,
    epoch: p::AuthorityEpoch,
    package_root: &Path,
    replay_root: &Path,
) -> (
    Arc<GuardedRemoteExecutor>,
    Arc<forme_harness::FileCapabilityPackageLedger>,
) {
    let keyring = Arc::new(InMemoryPublisherKeyring::default());
    keyring
        .provision(
            publisher.publisher.clone(),
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();
    let package_ledger =
        Arc::new(forme_harness::FileCapabilityPackageLedger::open(package_root).unwrap());
    let receiver = Arc::new(
        CapabilityPackageReceiver::new(
            executor.peer.clone(),
            publisher.clone(),
            ecosystem_policy(),
            epoch,
            keyring,
            package_ledger.clone(),
            Arc::new(SystemRemoteClock),
        )
        .unwrap(),
    );
    let service = GuardedRemoteExecutor::new(
        ExecutorAdmissionProfile {
            schema_version: p::M5_SCHEMA_VERSION,
            peer: executor.peer.clone(),
            grant: executor.clone(),
            profile: p::ExecutorProfileRef(PROFILE.into()),
            authority_epoch: epoch,
            minimum_fence: p::FenceToken(1),
            enabled_backends: BTreeSet::from([p::BackendKind::File]),
        },
        receiver,
        Arc::new(SystemRemoteClock),
    )
    .unwrap()
    .with_ledger(Arc::new(FileExecutorLedger::open(replay_root).unwrap()));
    (Arc::new(service), package_ledger)
}

fn event_kinds(store: &SqliteEventStore, run: &p::RunId) -> Vec<p::EventKind> {
    store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect()
}

struct ProcessIdentity {
    certificate: Vec<u8>,
    key: Vec<u8>,
}

struct ProcessPki {
    ca: Vec<u8>,
    authority: ProcessIdentity,
    executor: ProcessIdentity,
}

fn process_pki() -> ProcessPki {
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    let ca_key = KeyPair::generate().unwrap();
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);
    ProcessPki {
        ca: ca.der().to_vec(),
        authority: process_leaf(
            "authority.local",
            ExtendedKeyUsagePurpose::ClientAuth,
            &issuer,
        ),
        executor: process_leaf("localhost", ExtendedKeyUsagePurpose::ServerAuth, &issuer),
    }
}

fn process_leaf(
    name: &str,
    usage: ExtendedKeyUsagePurpose,
    issuer: &Issuer<'_, KeyPair>,
) -> ProcessIdentity {
    let mut params = CertificateParams::new(vec![name.to_owned()]).unwrap();
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![usage];
    let key = KeyPair::generate().unwrap();
    let certificate: Certificate = params.signed_by(&key, issuer).unwrap();
    ProcessIdentity {
        certificate: certificate.der().to_vec(),
        key: key.serialize_der(),
    }
}

fn write_process_identity(
    root: &Path,
    prefix: &str,
    identity: &ProcessIdentity,
    ca: &[u8],
) -> TlsIdentityFiles {
    let certificate_der = root.join(format!("{prefix}.cert.der"));
    let private_key_der = root.join(format!("{prefix}.key.der"));
    let trust_anchor_der = root.join(format!("{prefix}.ca.der"));
    fs::write(&certificate_der, &identity.certificate).unwrap();
    fs::write(&private_key_der, &identity.key).unwrap();
    fs::write(&trust_anchor_der, ca).unwrap();
    TlsIdentityFiles {
        certificate_der,
        private_key_der,
        trust_anchor_der,
    }
}

fn built_binary(name: &str) -> PathBuf {
    let configured = match name {
        "forme-m5-executord" => std::env::var_os("FORME_M5_EXECUTORD_BIN"),
        "forme-m5-registryd" => std::env::var_os("FORME_M5_REGISTRYD_BIN"),
        _ => None,
    };
    if let Some(path) = configured {
        return PathBuf::from(path);
    }
    std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_ready(child: &mut Child, marker: &Path, label: &str) {
    for _ in 0..500 {
        if marker.is_file() {
            return;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("{label} process exited before ready: {status}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("{label} process did not become ready");
}

fn free_loopback_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

struct FailOnceTlsTransport {
    inner: Arc<TlsRemoteTransport>,
    fail_receipt: AtomicBool,
}

impl FailOnceTlsTransport {
    fn new(inner: Arc<TlsRemoteTransport>) -> Self {
        Self {
            inner,
            fail_receipt: AtomicBool::new(true),
        }
    }
}

impl RemoteTransport for FailOnceTlsTransport {
    fn dispatch(
        &self,
        plan: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
    ) -> p::Result<p::RemoteDispatchAcceptance> {
        self.inner.dispatch(plan, lease)
    }

    fn probe(&self, request: p::RemoteProbeRequest) -> p::Result<p::RemoteProbeResult> {
        self.inner.probe(request)
    }

    fn cancel(&self, request: p::RemoteCancelRequest) -> p::Result<p::RemoteCancelResult> {
        self.inner.cancel(request)
    }
}

impl RemoteReceiptSource for FailOnceTlsTransport {
    fn receipt(&self, request: p::RemoteReceiptRequest) -> p::Result<p::RemoteDriverReceipt> {
        if self.fail_receipt.swap(false, Ordering::SeqCst) {
            return Err(p::Error(
                "golden receipt channel is temporarily unavailable".into(),
            ));
        }
        self.inner.receipt(request)
    }
}

struct ExecutorProcessConfig<'a> {
    binary: PathBuf,
    grant_path: &'a Path,
    publisher_path: &'a Path,
    policy_path: &'a Path,
    public_key_path: &'a Path,
    package_ledger_root: &'a Path,
    replay_ledger_root: &'a Path,
    wire_replay_root: &'a Path,
    executor_files: &'a TlsIdentityFiles,
    authority_identity: &'a p::TransportIdentityDigest,
    peer: &'a p::FederatedPeerRef,
    epoch: p::AuthorityEpoch,
    port: u16,
}

fn spawn_executor(config: &ExecutorProcessConfig<'_>, ready_path: &Path) -> ChildGuard {
    let mut child = ChildGuard(
        Command::new(&config.binary)
            .env("FORME_EXECUTOR_GRANT_PATH", config.grant_path)
            .env("FORME_EXECUTOR_PEER", &config.peer.0)
            .env("FORME_M5_PUBLISHER_GRANT_PATH", config.publisher_path)
            .env("FORME_M5_ADMISSION_POLICY_PATH", config.policy_path)
            .env("FORME_M5_PUBLISHER_PUBLIC_KEY_PATH", config.public_key_path)
            .env("FORME_M5_PACKAGE_LEDGER_ROOT", config.package_ledger_root)
            .env("FORME_EXECUTOR_PROFILE", PROFILE)
            .env("FORME_AUTHORITY_EPOCH", config.epoch.0.to_string())
            .env("FORME_EXECUTOR_LEDGER_ROOT", config.replay_ledger_root)
            .env("FORME_EXECUTOR_BIND", format!("127.0.0.1:{}", config.port))
            .env("FORME_AUTHORITY_ID", "authority:local")
            .env(
                "FORME_EXPECTED_AUTHORITY_IDENTITY",
                &config.authority_identity.0,
            )
            .env(
                "FORME_EXECUTOR_CERT_DER",
                &config.executor_files.certificate_der,
            )
            .env(
                "FORME_EXECUTOR_KEY_DER",
                &config.executor_files.private_key_der,
            )
            .env(
                "FORME_AUTHORITY_CA_DER",
                &config.executor_files.trust_anchor_der,
            )
            .env("FORME_EXECUTOR_REPLAY_ROOT", config.wire_replay_root)
            .env("FORME_EXECUTOR_TIMEOUT_MS", "10000")
            .env("FORME_EXECUTOR_MAX_BODY_BYTES", "1048576")
            .env("FORME_EXECUTOR_READY_PATH", ready_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    wait_for_ready(&mut child.0, ready_path, "M5 executor");
    child
}

fn resolve_context(with_permission: bool) -> p::ResolveContext {
    let capability = p::CapabilityRef("skill:m5-distributed".into());
    let permissions = if with_permission {
        vec![p::PermissionRef("permission:read".into())]
    } else {
        Vec::new()
    };
    p::ResolveContext {
        schema_version: p::M5_SCHEMA_VERSION,
        session: p::SessionId("session:m5-golden-toolset".into()),
        toolset: p::ToolsetRef("toolset:m5-golden".into()),
        envelope: p::AutonomyEnvelope {
            schema_version: p::M5_SCHEMA_VERSION,
            scope: p::Scope(SCOPE.into()),
            capability: p::CapabilitySet {
                schema_version: p::M5_SCHEMA_VERSION,
                capabilities: vec![capability.clone()],
                permissions,
            },
            action_type: vec![p::ActionType::Analyze],
            risk_limit: p::Risk::Low,
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
        },
        policy_allowed_providers: Vec::new(),
        policy_allowed_capabilities: vec![capability],
    }
}

fn apply_lifecycle(
    harness: &ReactiveHarness,
    package: &p::SignedCapabilityPackage,
    operation: p::CapabilityPackageOperation,
    suffix: &str,
    now: i64,
) -> p::CapabilityPackageState {
    apply_lifecycle_with_evidence(harness, package, operation, suffix, now).0
}

fn apply_lifecycle_with_evidence(
    harness: &ReactiveHarness,
    package: &p::SignedCapabilityPackage,
    operation: p::CapabilityPackageOperation,
    suffix: &str,
    now: i64,
) -> (
    p::CapabilityPackageState,
    p::CapabilityInstallPlan,
    p::CapabilityPackageApproval,
) {
    let plan = harness
        .prepare_capability_change(
            operation,
            package.manifest.package.clone(),
            package.manifest.release.clone(),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    let approval = p::CapabilityPackageApproval {
        schema_version: p::M5_SCHEMA_VERSION,
        approval: p::ApprovalId(format!("approval:m5-golden-{suffix}")),
        plan_digest: plan.digest.clone(),
        principal: p::VerifiedPrincipal(OWNER.into()),
        nonce: p::Nonce(format!("nonce:m5-golden-{suffix}")),
        expires_at: now.saturating_add(60_000),
    };
    let state = harness
        .apply_capability_change(
            p::RunId(format!("owner-control:m5-golden-{suffix}")),
            plan.clone(),
            approval.clone(),
            p::ReasonRef(format!("owner approved golden {suffix}")),
            now,
        )
        .unwrap();
    (state, plan, approval)
}

#[test]
fn s95_distribution_rejects_non_executor_stale_and_revoked_inputs_before_network() {
    let now = now_ms();
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let harness = build_harness(store.clone(), now);
    let key = SigningKey::from_bytes(&[51; 32]);
    let (package, publisher) = provision_and_admit(&harness, &store, &key, now);
    let (executor, owner_client) = configure_peers(&harness, now);
    let plan = distribution_plan(&harness, &package);

    let mut replica = executor.clone();
    replica.peer = p::FederatedPeerRef("peer:m5-replica".into());
    replica.roles = vec![p::FederatedPeerRole::Replica];
    replica.authority_epoch = p::AuthorityEpoch(3);
    replica.created_by = p::OwnerControlRef("owner-control:m5-replica".into());
    harness
        .register_federated_peer(
            p::RunId("owner-control:m5-register-replica".into()),
            replica.clone(),
            None,
            federation_version(2),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    let snapshot = harness.federation_snapshot(p::Scope(SCOPE.into())).unwrap();
    let replica_envelope = harness
        .ecosystem_runtime()
        .prepare_distribution_envelope(
            plan.clone(),
            replica.peer.clone(),
            snapshot.authority_epoch,
            p::VerifiedPrincipal(OWNER.into()),
            now,
        )
        .unwrap();
    let replica_placement = placement(&replica, snapshot.authority_epoch, &replica_envelope);
    harness
        .federation_runtime()
        .configure_executor_candidate(
            candidate(&replica, &replica_placement, now),
            replica_placement.clone(),
        )
        .unwrap();
    assert!(harness
        .submit_federated_remote_action(
            p::RunId("run:m5-replica".into()),
            run_request("replica"),
            action_intent(replica_placement, now, "replica"),
        )
        .is_err());

    let envelope = harness
        .ecosystem_runtime()
        .prepare_distribution_envelope(
            plan.clone(),
            executor.peer.clone(),
            snapshot.authority_epoch,
            p::VerifiedPrincipal(OWNER.into()),
            now,
        )
        .unwrap();
    let valid = placement(&executor, snapshot.authority_epoch, &envelope);

    let mut stale = valid.clone();
    stale.authority_epoch = p::AuthorityEpoch(snapshot.authority_epoch.0.saturating_sub(1));
    stale.refresh_digest().unwrap();
    let stale_intent = action_intent(stale, now, "stale");
    assert!(harness
        .submit_federated_remote_action(
            p::RunId("run:m5-stale".into()),
            run_request("stale"),
            stale_intent,
        )
        .is_err());

    let mut wrong_grant = valid.clone();
    wrong_grant.peer_grant = p::FederatedPeerGrantRef("grant:wrong".into());
    wrong_grant.refresh_digest().unwrap();
    let wrong_intent = action_intent(wrong_grant, now, "wrong-grant");
    assert!(harness
        .submit_federated_remote_action(
            p::RunId("run:m5-wrong-grant".into()),
            run_request("wrong-grant"),
            wrong_intent,
        )
        .is_err());

    assert!(store
        .read_run(p::RunId("run:m5-replica".into()))
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .is_empty());
    assert!(store
        .read_run(p::RunId("run:m5-stale".into()))
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .is_empty());

    let root = temporary_root("revoked-before-dispatch");
    fs::create_dir_all(&root).unwrap();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let (service, package_ledger) = executor_service(
        &executor,
        &publisher,
        &key,
        snapshot.authority_epoch,
        &root.join("packages"),
        &root.join("replay"),
    );
    harness
        .federation_runtime()
        .configure_remote(Arc::new(ExecutorTransport::new(
            service,
            dispatches.clone(),
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
            false,
        )))
        .unwrap();
    harness
        .federation_runtime()
        .configure_executor_candidate(candidate(&executor, &valid, now), valid.clone())
        .unwrap();
    let revoke_run = p::RunId("run:m5-revoked-before-dispatch".into());
    let submission = harness
        .submit_federated_remote_action(
            revoke_run.clone(),
            run_request("revoked-before-dispatch"),
            action_intent(valid, now, "revoked-before-dispatch"),
        )
        .unwrap();
    let control_nonce = "nonce:m5-revoked-before-dispatch";
    harness
        .bind_capability_distribution(
            revoke_run.clone(),
            plan.clone(),
            p::CapabilityPackageApproval {
                schema_version: p::M5_SCHEMA_VERSION,
                approval: submission.approval.clone(),
                plan_digest: plan.digest.clone(),
                principal: p::VerifiedPrincipal(OWNER.into()),
                nonce: p::Nonce(control_nonce.into()),
                expires_at: now.saturating_add(60_000),
            },
            Arc::new(CapabilityPackageLedgerGroundTruth::new(package_ledger)),
        )
        .unwrap();
    let revoke = harness
        .prepare_capability_change(
            p::CapabilityPackageOperation::Revoke,
            package.manifest.package.clone(),
            package.manifest.release.clone(),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    harness
        .apply_capability_change(
            p::RunId("owner-control:m5-revoke-package".into()),
            revoke.clone(),
            p::CapabilityPackageApproval {
                schema_version: p::M5_SCHEMA_VERSION,
                approval: p::ApprovalId("approval:m5-revoke-package".into()),
                plan_digest: revoke.digest.clone(),
                principal: p::VerifiedPrincipal(OWNER.into()),
                nonce: p::Nonce("nonce:m5-revoke-package".into()),
                expires_at: now.saturating_add(60_000),
            },
            p::ReasonRef("owner revoked package before distribution".into()),
            now,
        )
        .unwrap();
    let command = p::FederatedOwnerCommand::ResolveApproval {
        approval: submission.approval,
        plan_digest: submission.plan_digest,
        outcome: p::ApprovalOutcome::Granted,
    };
    assert!(harness
        .apply_federated_owner_command(
            owner_envelope(&owner_client, &command, control_nonce, now),
            command,
            now,
        )
        .is_err());
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    assert!(!event_kinds(&store, &revoke_run).contains(&p::EventKind::ActionStarted));
    fs::remove_dir_all(root).unwrap();
    assert!(store
        .read_run(p::RunId("run:m5-wrong-grant".into()))
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .is_empty());
}

#[test]
fn s96_s97_distribution_unknown_recovers_after_authority_and_executor_restart_once() {
    let root = temporary_root("restart");
    fs::create_dir_all(&root).unwrap();
    let database = root.join("authority.sqlite3");
    let package_root = root.join("package-ledger");
    let replay_root = root.join("executor-replay");
    let now = now_ms();
    let key = SigningKey::from_bytes(&[52; 32]);
    let dispatches = Arc::new(AtomicUsize::new(0));
    let probes = Arc::new(AtomicUsize::new(0));
    let receipt_reads = Arc::new(AtomicUsize::new(0));
    let run = p::RunId("run:m5-distribution-restart".into());

    let store = SqliteEventStore::open(&database, StoreOptions::default()).unwrap();
    let harness = build_harness(store.clone(), now);
    let (package, publisher) = provision_and_admit(&harness, &store, &key, now);
    let (executor, owner_client) = configure_peers(&harness, now);
    let snapshot = harness.federation_snapshot(p::Scope(SCOPE.into())).unwrap();
    let plan = distribution_plan(&harness, &package);
    let envelope = harness
        .ecosystem_runtime()
        .prepare_distribution_envelope(
            plan.clone(),
            executor.peer.clone(),
            snapshot.authority_epoch,
            p::VerifiedPrincipal(OWNER.into()),
            now,
        )
        .unwrap();
    let placement = placement(&executor, snapshot.authority_epoch, &envelope);
    let (service, package_ledger) = executor_service(
        &executor,
        &publisher,
        &key,
        snapshot.authority_epoch,
        &package_root,
        &replay_root,
    );
    let transport = Arc::new(ExecutorTransport::new(
        service,
        dispatches.clone(),
        probes.clone(),
        receipt_reads.clone(),
        true,
    ));
    harness
        .federation_runtime()
        .configure_remote(transport)
        .unwrap();
    harness
        .federation_runtime()
        .configure_executor_candidate(candidate(&executor, &placement, now), placement.clone())
        .unwrap();
    let intent = action_intent(placement, now, "restart");
    let submission = harness
        .submit_federated_remote_action(run.clone(), run_request("restart"), intent)
        .unwrap();
    let control_nonce = "nonce:m5-distribution-once";
    let package_approval = p::CapabilityPackageApproval {
        schema_version: p::M5_SCHEMA_VERSION,
        approval: submission.approval.clone(),
        plan_digest: plan.digest.clone(),
        principal: p::VerifiedPrincipal(OWNER.into()),
        nonce: p::Nonce(control_nonce.into()),
        expires_at: now.saturating_add(60_000),
    };
    harness
        .bind_capability_distribution(
            run.clone(),
            plan.clone(),
            package_approval.clone(),
            Arc::new(CapabilityPackageLedgerGroundTruth::new(
                package_ledger.clone(),
            )),
        )
        .unwrap();
    let command = p::FederatedOwnerCommand::ResolveApproval {
        approval: submission.approval,
        plan_digest: submission.plan_digest,
        outcome: p::ApprovalOutcome::Granted,
    };
    let resolved = harness
        .apply_federated_owner_command(
            owner_envelope(&owner_client, &command, control_nonce, now),
            command,
            now,
        )
        .unwrap();
    assert!(matches!(
        resolved,
        forme_harness::FederationControlResult::ApprovalResolved {
            terminal: false,
            ..
        }
    ));
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    assert_eq!(package_ledger.record_count().unwrap(), 1);
    let unknown = event_kinds(&store, &run);
    assert!(unknown.windows(3).any(|events| {
        events
            == [
                p::EventKind::ActionStarted,
                p::EventKind::FailureEvidenceRecorded,
                p::EventKind::ActionOutcomeUnknown,
            ]
            || events
                == [
                    p::EventKind::ActionStarted,
                    p::EventKind::ActionOutcomeUnknown,
                    p::EventKind::RunWaiting,
                ]
    }));
    assert!(unknown.contains(&p::EventKind::ActionOutcomeUnknown));
    assert!(unknown.contains(&p::EventKind::RunWaiting));
    assert!(!unknown.contains(&p::EventKind::CapabilityPackageDistributionRecorded));
    drop(harness);
    drop(store);

    let restarted_store = SqliteEventStore::open(&database, StoreOptions::default()).unwrap();
    let restarted = build_harness(restarted_store.clone(), now);
    restarted
        .configure_ecosystem_publisher_key(
            publisher.publisher.clone(),
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();
    let (restarted_service, restarted_package_ledger) = executor_service(
        &executor,
        &publisher,
        &key,
        snapshot.authority_epoch,
        &package_root,
        &replay_root,
    );
    let restarted_transport = Arc::new(ExecutorTransport::new(
        restarted_service,
        dispatches.clone(),
        probes.clone(),
        receipt_reads.clone(),
        false,
    ));
    restarted
        .federation_runtime()
        .configure_remote(restarted_transport)
        .unwrap();
    restarted
        .bind_capability_distribution(
            run.clone(),
            plan.clone(),
            package_approval,
            Arc::new(CapabilityPackageLedgerGroundTruth::new(
                restarted_package_ledger.clone(),
            )),
        )
        .unwrap();
    let recovery = restarted.recover_federated_remote_action(&run).unwrap();
    assert!(recovery.terminal);
    assert_eq!(recovery.outcome, p::RemoteReceiptOutcome::Completed);
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    assert_eq!(probes.load(Ordering::SeqCst), 0);
    assert_eq!(restarted_package_ledger.record_count().unwrap(), 1);
    assert!(receipt_reads.load(Ordering::SeqCst) >= 2);

    let events = restarted_store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    let kinds = events.iter().map(|event| event.kind).collect::<Vec<_>>();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == p::EventKind::ActionStarted)
            .count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == p::EventKind::CapabilityPackageDistributionRecorded)
            .count(),
        1
    );
    let acquired = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                p::EventPayload::RemoteExecutionLeaseChanged(payload)
                    if payload.lease.state == p::RemoteLeaseState::Acquired
            )
        })
        .unwrap();
    let distributed = events
        .iter()
        .position(|event| event.kind == p::EventKind::CapabilityPackageDistributionRecorded)
        .unwrap();
    let released = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                p::EventPayload::RemoteExecutionLeaseChanged(payload)
                    if payload.lease.state == p::RemoteLeaseState::Released
            )
        })
        .unwrap();
    assert!(acquired < distributed && distributed < released);
    let distribution =
        EcosystemProjection::snapshot(&restarted_store, p::Scope(SCOPE.into())).unwrap();
    assert_eq!(distribution.distributions.len(), 1);
    assert_eq!(
        restarted_store
            .ecosystem_version(&p::EcosystemAggregateRef("ecosystem".into()))
            .unwrap(),
        plan.expected_version.next().unwrap()
    );
    restarted.recover_federated_remote_action(&run).unwrap();
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    assert_eq!(restarted_package_ledger.record_count().unwrap(), 1);

    drop(restarted);
    drop(restarted_store);
    drop(restarted_package_ledger);
    drop(package_ledger);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires explicitly built M5 registry and executor process binaries"]
fn s98_three_process_registry_authority_executor_golden_is_governed_end_to_end() {
    let root = temporary_root("three-process-golden");
    fs::create_dir_all(&root).unwrap();
    let database = root.join("authority.sqlite3");
    let now = now_ms();
    let key = SigningKey::from_bytes(&[53; 32]);
    let package = signed_package(&key);
    let package_path = root.join("registry-package.json");
    fs::write(&package_path, serde_json::to_vec(&package).unwrap()).unwrap();

    let registry_port = free_loopback_port();
    let registry_ready = root.join("registry.ready");
    let registry_count = root.join("registry.count");
    let mut registry_process = ChildGuard(
        Command::new(built_binary("forme-m5-registryd"))
            .env(
                "FORME_M5_REGISTRY_BIND",
                format!("127.0.0.1:{registry_port}"),
            )
            .env("FORME_M5_REGISTRY_PACKAGE_PATH", &package_path)
            .env("FORME_M5_REGISTRY_COUNT_PATH", &registry_count)
            .env("FORME_M5_REGISTRY_READY_PATH", &registry_ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    wait_for_ready(&mut registry_process.0, &registry_ready, "M5 registry");

    let store = SqliteEventStore::open(&database, StoreOptions::default()).unwrap();
    let fetch_harness = catalog_harness(store.clone(), format!("http://127.0.0.1:{registry_port}"));
    let publisher = publisher_grant(&key, now);
    fetch_harness
        .configure_ecosystem_publisher_key(
            publisher.publisher.clone(),
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();
    fetch_harness
        .provision_capability_publisher(
            p::RunId("owner-control:m5-golden-publisher".into()),
            publisher.clone(),
            None,
            p::EcosystemAggregateVersion::zero(),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    let catalog_run = fetch_catalog_package(
        &fetch_harness,
        format!("http://127.0.0.1:{registry_port}/package"),
        now,
    );
    fetch_harness
        .admit_capability_package(
            p::RunId("owner-control:m5-golden-admit".into()),
            catalog_run.clone(),
            package.clone(),
            now,
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(&registry_count)
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        1
    );
    drop(fetch_harness);
    drop(store);

    let pki = process_pki();
    let authority_identity = transport_identity_digest(&pki.authority.certificate);
    let executor_identity = transport_identity_digest(&pki.executor.certificate);
    let authority_files = write_process_identity(&root, "authority", &pki.authority, &pki.ca);
    let executor_files = write_process_identity(&root, "executor", &pki.executor, &pki.ca);
    let authority_store = SqliteEventStore::open(&database, StoreOptions::default()).unwrap();
    let authority = build_harness(authority_store.clone(), now);
    authority
        .configure_ecosystem_publisher_key(
            publisher.publisher.clone(),
            PublisherPublicKey::from_bytes(key.verifying_key().to_bytes()),
        )
        .unwrap();
    let _installed = apply_lifecycle(
        &authority,
        &package,
        p::CapabilityPackageOperation::Install,
        "install",
        now,
    );
    let (enabled_state, enable_plan, enable_approval) = apply_lifecycle_with_evidence(
        &authority,
        &package,
        p::CapabilityPackageOperation::Enable,
        "enable",
        now,
    );
    assert_eq!(
        CapabilityRegistry::resolve_toolset(
            authority.ecosystem_package_registry().inner().as_ref(),
            &resolve_context(true),
        )
        .unwrap()
        .items
        .len(),
        1
    );
    assert!(CapabilityRegistry::resolve_toolset(
        authority.ecosystem_package_registry().inner().as_ref(),
        &resolve_context(false),
    )
    .unwrap()
    .items
    .is_empty());
    let mut executor = peer_grant(
        "peer:m5-golden-executor",
        vec![p::FederatedPeerRole::Executor],
        1,
        now,
    );
    executor.transport_identity = executor_identity.clone();
    let owner_client = peer_grant(
        "peer:m5-golden-owner-client",
        vec![p::FederatedPeerRole::OwnerClient],
        2,
        now,
    );
    authority
        .register_federated_peer(
            p::RunId("owner-control:m5-golden-executor".into()),
            executor.clone(),
            None,
            federation_version(0),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    authority
        .register_federated_peer(
            p::RunId("owner-control:m5-golden-owner-client".into()),
            owner_client.clone(),
            None,
            federation_version(1),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    let federation = authority
        .federation_snapshot(p::Scope(SCOPE.into()))
        .unwrap();

    let grant_path = root.join("executor-grant.json");
    let publisher_path = root.join("publisher-grant.json");
    let policy_path = root.join("admission-policy.json");
    let public_key_path = root.join("publisher-public-key.bin");
    fs::write(&grant_path, serde_json::to_vec(&executor).unwrap()).unwrap();
    fs::write(&publisher_path, serde_json::to_vec(&publisher).unwrap()).unwrap();
    fs::write(
        &policy_path,
        serde_json::to_vec(&ecosystem_policy()).unwrap(),
    )
    .unwrap();
    fs::write(&public_key_path, key.verifying_key().to_bytes()).unwrap();
    let package_ledger_root = root.join("executor-package-ledger");
    let replay_ledger_root = root.join("executor-dispatch-ledger");
    let wire_replay_root = root.join("executor-wire-replay");
    let executor_binary = built_binary("forme-m5-executord");
    let first_port = free_loopback_port();
    let first_ready = root.join("executor-first.ready");
    let first_config = ExecutorProcessConfig {
        binary: executor_binary.clone(),
        grant_path: &grant_path,
        publisher_path: &publisher_path,
        policy_path: &policy_path,
        public_key_path: &public_key_path,
        package_ledger_root: &package_ledger_root,
        replay_ledger_root: &replay_ledger_root,
        wire_replay_root: &wire_replay_root,
        executor_files: &executor_files,
        authority_identity: &authority_identity,
        peer: &executor.peer,
        epoch: federation.authority_epoch,
        port: first_port,
    };
    let executor_process = spawn_executor(&first_config, &first_ready);
    let first_tls = Arc::new(
        TlsRemoteTransport::new(TlsRemoteClientConfig {
            endpoint: format!("https://localhost:{first_port}"),
            authority: p::AuthorityRef("authority:local".into()),
            peer: executor.peer.clone(),
            expected_peer_identity: executor_identity.clone(),
            identity: authority_files.clone(),
            timeout: p::DurationMs(10_000),
            request_ttl: p::DurationMs(5_000),
            max_body_bytes: 1_048_576,
        })
        .unwrap(),
    );
    authority
        .federation_runtime()
        .configure_remote(Arc::new(FailOnceTlsTransport::new(first_tls.clone())))
        .unwrap();
    let distribution_plan = distribution_plan(&authority, &package);
    let distribution_envelope = authority
        .ecosystem_runtime()
        .prepare_distribution_envelope(
            distribution_plan.clone(),
            executor.peer.clone(),
            federation.authority_epoch,
            p::VerifiedPrincipal(OWNER.into()),
            now,
        )
        .unwrap();
    let remote_placement = placement(
        &executor,
        federation.authority_epoch,
        &distribution_envelope,
    );
    authority
        .federation_runtime()
        .configure_executor_candidate(
            candidate(&executor, &remote_placement, now),
            remote_placement.clone(),
        )
        .unwrap();
    let distribution_run = p::RunId("run:m5-three-process-distribution".into());
    let submission = authority
        .submit_federated_remote_action(
            distribution_run.clone(),
            run_request("three-process"),
            action_intent(remote_placement, now, "three-process"),
        )
        .unwrap();
    let control_nonce = "nonce:m5-three-process-distribution";
    let package_approval = p::CapabilityPackageApproval {
        schema_version: p::M5_SCHEMA_VERSION,
        approval: submission.approval.clone(),
        plan_digest: distribution_plan.digest.clone(),
        principal: p::VerifiedPrincipal(OWNER.into()),
        nonce: p::Nonce(control_nonce.into()),
        expires_at: now.saturating_add(60_000),
    };
    let package_ledger =
        Arc::new(forme_harness::FileCapabilityPackageLedger::open(&package_ledger_root).unwrap());
    authority
        .bind_capability_distribution(
            distribution_run.clone(),
            distribution_plan.clone(),
            package_approval.clone(),
            Arc::new(CapabilityPackageLedgerGroundTruth::new(
                package_ledger.clone(),
            )),
        )
        .unwrap();
    let command = p::FederatedOwnerCommand::ResolveApproval {
        approval: submission.approval,
        plan_digest: submission.plan_digest,
        outcome: p::ApprovalOutcome::Granted,
    };
    assert!(matches!(
        authority
            .apply_federated_owner_command(
                owner_envelope(&owner_client, &command, control_nonce, now),
                command,
                now,
            )
            .unwrap(),
        forme_harness::FederationControlResult::ApprovalResolved {
            terminal: false,
            ..
        }
    ));
    assert_eq!(package_ledger.record_count().unwrap(), 1);
    assert!(event_kinds(&authority_store, &distribution_run)
        .contains(&p::EventKind::ActionOutcomeUnknown));

    drop(executor_process);
    fs::remove_file(&first_ready).unwrap();
    let second_ready = root.join("executor-second.ready");
    let second_executor_process = spawn_executor(&first_config, &second_ready);
    authority
        .federation_runtime()
        .configure_remote(first_tls)
        .unwrap();
    let recovered = authority
        .recover_federated_remote_action(&distribution_run)
        .unwrap();
    assert!(recovered.terminal);
    assert_eq!(recovered.outcome, p::RemoteReceiptOutcome::Completed);
    assert_eq!(package_ledger.record_count().unwrap(), 1);
    let distribution_events = authority_store
        .read_run(distribution_run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    let distribution_event_count = distribution_events
        .iter()
        .filter(|event| event.kind == p::EventKind::CapabilityPackageDistributionRecorded)
        .count() as u64;
    let action_started_count = distribution_events
        .iter()
        .filter(|event| event.kind == p::EventKind::ActionStarted)
        .count() as u64;
    let distribution_snapshot =
        EcosystemProjection::snapshot(&authority_store, p::Scope(SCOPE.into())).unwrap();
    let distribution_ref = distribution_snapshot.distributions.first().unwrap().clone();
    let distribution_receipt =
        EcosystemRuntimeLedger::distribution_receipt(&authority_store, &distribution_ref)
            .unwrap()
            .unwrap();
    let executor_record = package_ledger
        .record_for_digest(&package.package_digest)
        .unwrap()
        .unwrap();
    let admission = EcosystemProjection::admission(&authority_store, &package.manifest.release)
        .unwrap()
        .unwrap();
    assert_eq!(
        fs::read_dir(&replay_ledger_root)
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        event_kinds(&authority_store, &distribution_run)
            .into_iter()
            .filter(|kind| *kind == p::EventKind::ActionStarted)
            .count(),
        1
    );
    assert_eq!(
        event_kinds(&authority_store, &distribution_run)
            .into_iter()
            .filter(|kind| *kind == p::EventKind::CapabilityPackageDistributionRecorded)
            .count(),
        1
    );
    assert_eq!(
        CapabilityRegistry::resolve_toolset(
            authority.ecosystem_package_registry().inner().as_ref(),
            &resolve_context(true),
        )
        .unwrap()
        .items
        .len(),
        1
    );
    apply_lifecycle(
        &authority,
        &package,
        p::CapabilityPackageOperation::Revoke,
        "revoke",
        now,
    );
    assert!(CapabilityRegistry::resolve_toolset(
        authority.ecosystem_package_registry().inner().as_ref(),
        &resolve_context(true),
    )
    .unwrap()
    .items
    .is_empty());
    assert!(authority
        .prepare_capability_change(
            p::CapabilityPackageOperation::Distribute,
            package.manifest.package.clone(),
            package.manifest.release.clone(),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .is_err());
    assert_eq!(package_ledger.record_count().unwrap(), 1);
    assert_eq!(
        fs::read_to_string(&registry_count)
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        1
    );
    let authority_json = serde_json::to_string(&distribution_events).unwrap();
    let private_hex = key
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert!(!authority_json.contains(&private_hex));
    assert!(!authority_json.contains(&root.to_string_lossy().to_string()));

    if let Some(artifact_root) = std::env::var_os("FORME_M5_ARTIFACT_DIR") {
        let registry_fetches = fs::read_to_string(&registry_count)
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let publisher_artifact = M5PublisherArtifact::from_runtime(
            &publisher,
            now,
            p::EcosystemAggregateVersion {
                schema_version: p::M5_SCHEMA_VERSION,
                value: 1,
            },
        )
        .unwrap();
        let admission_artifact = M5AdmissionArtifact::from_runtime(
            &admission,
            &package.manifest,
            p::ContentRef(format!("catalog-content:{}", package.package_digest.0)),
            p::EcosystemAggregateVersion {
                schema_version: p::M5_SCHEMA_VERSION,
                value: 2,
            },
        )
        .unwrap();
        let distribution_artifact = M5DistributionArtifact::from_runtime(
            &distribution_plan,
            &distribution_envelope,
            &executor_record,
            &distribution_receipt,
            distribution_plan.expected_version.next().unwrap(),
        )
        .unwrap();
        let portable_distribution_receipt = distribution_artifact.receipt.reference.clone();
        let artifact = M5ArtifactBundle {
            publisher: publisher_artifact,
            admission: admission_artifact,
            install: M5InstallArtifact {
                schema_version: p::M5_SCHEMA_VERSION,
                plan: enable_plan.clone(),
                approval: M5ApprovalEvidence {
                    schema_version: p::M5_SCHEMA_VERSION,
                    approval: enable_approval.approval.clone(),
                    plan_digest: enable_approval.plan_digest.clone(),
                    principal: enable_approval.principal.clone(),
                },
                state: enabled_state.clone(),
                committed_version: p::EcosystemAggregateVersion {
                    schema_version: p::M5_SCHEMA_VERSION,
                    value: 4,
                },
                registry_digest: p::canonical_digest(&(
                    &enabled_state.package,
                    &enabled_state.release,
                    enabled_state.active_generation,
                ))
                .unwrap(),
            },
            distribution: distribution_artifact,
            trace: M5TraceArtifact {
                schema_version: p::M5_SCHEMA_VERSION,
                scenario: "S98 governed ecosystem golden".into(),
                run: distribution_run.clone(),
                package: package.manifest.package.clone(),
                release: package.manifest.release.clone(),
                package_digest: package.package_digest.clone(),
                publisher_grant: publisher.reference.clone(),
                admission: admission.reference.clone(),
                install_plan: enable_plan.reference.clone(),
                distribution_receipt: portable_distribution_receipt,
                event_kinds: distribution_events.iter().map(|event| event.kind).collect(),
                stream_seq: distribution_events
                    .iter()
                    .map(|event| event.stream_seq)
                    .collect(),
                event_taxonomy: p::EventKind::ALL[..p::M5_EVENT_KIND_COUNT].to_vec(),
                registry_fetches,
                authority_driver_calls: action_started_count,
                executor_installs: package_ledger.record_count().unwrap() as u64,
                distribution_events: distribution_event_count,
                unknown_retry_count: action_started_count.saturating_sub(1),
                post_revoke_visible_contributions: CapabilityRegistry::resolve_toolset(
                    authority.ecosystem_package_registry().inner().as_ref(),
                    &resolve_context(true),
                )
                .unwrap()
                .items
                .len() as u64,
                post_revoke_distribution_attempts: 0,
                restricted_material_matches: 0,
                ground_truth_verified: true,
            },
        };
        let receipt = M5ArtifactStore::new(artifact_root)
            .unwrap()
            .write(&artifact)
            .unwrap();
        eprintln!("M5 artifact receipt {}", receipt.digest.0);
    }

    drop(authority);
    drop(authority_store);
    drop(package_ledger);
    drop(second_executor_process);
    drop(registry_process);
    fs::remove_dir_all(root).unwrap();
}
