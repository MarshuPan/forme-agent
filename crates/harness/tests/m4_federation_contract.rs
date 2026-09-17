use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forme_cognition::{CompetenceInputs, InterventionLevel};
use forme_eval::{FederationArtifactBundle, FederationArtifactStore, RemoteGroundTruth};
use forme_execution::{
    transport_identity_digest, ExecutionBackendRegistry, RemoteReceiptSource, RemoteTransport,
    RepositoryMutationState, TlsIdentityFiles, TlsRemoteClientConfig, TlsRemoteTransport,
};
use forme_harness::{
    FederatedHarnessRuntime, FederationActionGateway, FederationGatewayControl,
    FixedCompetenceGate, GovernanceConfig, HarnessConfig, ReactiveHarness, RemoteGroundTruthSource,
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
    EventStore, FederationEventStore, FederationProjection, SqliteEventStore, StoreOptions,
};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair, KeyUsagePurpose,
};

const OWNER: &str = "owner:forme";
const SCOPE: &str = "workspace:m4";
const CAPABILITY: &str = "fixture.mutate";

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

fn version(value: u64) -> p::FederationAggregateVersion {
    p::FederationAggregateVersion {
        schema_version: p::M4_SCHEMA_VERSION,
        aggregate: p::FederationAggregateRef("federation".into()),
        version: value,
    }
}

fn grant(
    peer: &str,
    roles: Vec<p::FederatedPeerRole>,
    epoch: u64,
    now: i64,
) -> p::FederatedPeerGrant {
    p::FederatedPeerGrant {
        schema_version: p::M4_SCHEMA_VERSION,
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

struct RuntimeFixture {
    store: SqliteEventStore,
    runtime: Arc<FederatedHarnessRuntime>,
    executor: p::FederatedPeerGrant,
    owner_client: p::FederatedPeerGrant,
    now: i64,
}

fn runtime_fixture() -> RuntimeFixture {
    let now = now_ms();
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let runtime = Arc::new(FederatedHarnessRuntime::new(store.clone()));
    let executor = grant(
        "peer:executor",
        vec![p::FederatedPeerRole::Executor],
        1,
        now,
    );
    runtime
        .register_peer(
            p::RunId("owner-control:register-executor".into()),
            executor.clone(),
            None,
            version(0),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    let owner_client = grant(
        "peer:owner-client",
        vec![p::FederatedPeerRole::OwnerClient],
        2,
        now,
    );
    runtime
        .register_peer(
            p::RunId("owner-control:register-owner-client".into()),
            owner_client.clone(),
            None,
            version(1),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    RuntimeFixture {
        store,
        runtime,
        executor,
        owner_client,
        now,
    }
}

fn profile() -> ModelProfile {
    ModelProfile {
        schema_version: p::SchemaVersion(1),
        provider: p::ProviderId("m4-test-provider".into()),
        model: "m4-test-model".into(),
        base_url: Url::parse("https://models.invalid/v1").unwrap(),
        capability: ModelCapability {
            schema_version: p::SchemaVersion(1),
            context_window: 16_384,
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
        credential_ref: p::CredentialRef("secret:m4-test-model".into()),
    }
}

fn placement(grant: &p::FederatedPeerGrant, epoch: p::AuthorityEpoch) -> p::RemotePlacementPlan {
    let mut operation = p::RemoteOperation {
        schema_version: p::M4_SCHEMA_VERSION,
        backend: p::BackendKind::File,
        parameters: p::ActionParameters::File {
            operation: p::FileOperation::Write,
            path: "golden/state".into(),
            content: Some(b"mutation".to_vec()),
        },
        capability: p::CapabilityRef(CAPABILITY.into()),
        scope: p::Scope(SCOPE.into()),
        action_type: p::ActionType::ExternalCommit,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_boundary: p::RollbackBoundary("fixture-record-only".into()),
        credential_slot: None,
        digest: p::SchemaDigest(String::new()),
    };
    operation.refresh_digest().unwrap();
    let mut placement = p::RemotePlacementPlan {
        schema_version: p::M4_SCHEMA_VERSION,
        executor: grant.peer.clone(),
        peer_grant: grant.reference().unwrap(),
        grant_version: grant.grant_version,
        authority_epoch: epoch,
        executor_profile: p::ExecutorProfileRef("profile:m4-fixture".into()),
        operation,
        digest: p::SchemaDigest(String::new()),
    };
    placement.refresh_digest().unwrap();
    placement
}

fn candidate(
    grant: &p::FederatedPeerGrant,
    plan: &p::RemotePlacementPlan,
    now: i64,
) -> p::FederatedExecutorCandidate {
    p::FederatedExecutorCandidate {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: grant.peer.clone(),
        grant: grant.reference().unwrap(),
        profile: plan.executor_profile.clone(),
        scope: plan.operation.scope.clone(),
        capability: plan.operation.capability.clone(),
        expires_at: grant.expires_at,
        health: p::ExecutorHealthState::Healthy,
        health_observed_at: now,
        capability_evidence: vec![p::CapabilityEvidenceRef(
            "capability-evidence:fixture-mutation-pass".into(),
        )],
        failure_evidence: Vec::new(),
        managed_policy_allowed: true,
        score_basis_points: 5_000,
    }
}

fn intent(plan: p::RemotePlacementPlan, now: i64) -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::M4_SCHEMA_VERSION,
        intent_id: p::ActionId("intent:m4-remote".into()),
        source: p::Source::UserTurn,
        goal: p::GoalRef("perform one governed remote mutation".into()),
        backend_hint: p::BackendKind::Remote,
        capability_ref: p::CapabilityRef(CAPABILITY.into()),
        action_type: p::ActionType::ExternalCommit,
        scope: p::Scope(SCOPE.into()),
        risk_hint: p::Risk::Medium,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("fixture-record-only".into()),
        parameters: p::ActionParameters::Remote(Box::new(p::RemoteActionSpec {
            schema_version: p::M4_SCHEMA_VERSION,
            placement: plan,
        })),
        requested_permissions: vec![p::PermissionRef("permission:remote-execute".into())],
        requested_at: now,
        estimated_output_bytes: 1_024,
        estimated_duration: p::DurationMs(1_000),
    }
}

fn governance(intent: &p::ActionIntent, now: i64) -> GovernanceConfig {
    let envelope = p::AutonomyEnvelope {
        schema_version: p::M4_SCHEMA_VERSION,
        scope: intent.scope.clone(),
        capability: p::CapabilitySet {
            schema_version: p::M4_SCHEMA_VERSION,
            capabilities: vec![intent.capability_ref.clone()],
            permissions: intent.requested_permissions.clone(),
        },
        action_type: vec![intent.action_type],
        risk_limit: p::Risk::High,
        approval_rule: p::ApprovalRule::Allow,
        budget: p::Budget("units:2".into()),
        timebox: p::Timebox {
            schema_version: p::M4_SCHEMA_VERSION,
            starts_at: now.saturating_sub(1_000),
            expires_at: now.saturating_add(600_000),
            max_turns: 2,
        },
        rollback: p::RollbackReq {
            schema_version: p::M4_SCHEMA_VERSION,
            required: true,
            boundary: Some(intent.rollback_expectation.clone()),
        },
    };
    GovernanceConfig {
        schema_version: p::M4_SCHEMA_VERSION,
        layers: vec![PolicyLayer {
            schema_version: p::M4_SCHEMA_VERSION,
            source: PolicyLayerSource::User,
            rules: vec![PolicyRule {
                schema_version: p::M4_SCHEMA_VERSION,
                matcher: ActionMatcher {
                    backend: Some(p::BackendKind::Remote),
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
        external: forme_policy::ExternalPolicyLimits::default(),
        network_allowed: true,
        sandbox_available: true,
        delegation: Some(DelegationGrant {
            schema_version: p::M4_SCHEMA_VERSION,
            subject: DelegationSubject::Owner,
            envelope: envelope.clone(),
            granted_by: p::Actor::Owner,
            audit_ref: p::EventId("delegation:m4".into()),
        }),
        envelope: Some(envelope),
    }
}

fn request() -> p::RunRequest {
    p::RunRequest {
        schema_version: p::M4_SCHEMA_VERSION,
        source: p::Source::UserTurn,
        session: p::SessionRef("session:m4-remote".into()),
        agent_profile: p::AgentProfileRef("agent:forme".into()),
        input: p::RunInput("perform remote mutation".into()),
        budget: Some(p::Budget("units:2".into())),
        idempotency_key: Some(p::IdempotencyKey("m4-remote-once".into())),
    }
}

fn harness_config() -> HarnessConfig {
    let mut config = HarnessConfig::for_model(&profile());
    config.policy_version = p::Version(7);
    config.tool_schema_version = p::Version(11);
    config.workspace = p::WorkspaceRef(SCOPE.into());
    config.toolset_ref = p::ToolsetRef("toolset:m4".into());
    config
}

fn result_digest() -> p::SchemaDigest {
    p::canonical_digest(&b"mutation".to_vec()).unwrap()
}

#[derive(Default)]
struct RecordingTransport {
    dispatches: AtomicUsize,
    receipts: Mutex<BTreeMap<p::RemoteDriverReceiptRef, p::RemoteDriverReceipt>>,
    by_lease: Mutex<BTreeMap<p::RemoteExecutionLeaseRef, p::RemoteDriverReceiptRef>>,
}

impl RecordingTransport {
    fn dispatches(&self) -> usize {
        self.dispatches.load(Ordering::SeqCst)
    }
}

impl RemoteTransport for RecordingTransport {
    fn dispatch(
        &self,
        plan: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
    ) -> p::Result<p::RemoteDispatchAcceptance> {
        plan.validate()?;
        lease.validate()?;
        let receipt_ref = p::RemoteDriverReceiptRef(format!("receipt:{}", lease.dispatch.0));
        let mut receipts = self
            .receipts
            .lock()
            .map_err(|_| p::Error("recording receipt state is unavailable".into()))?;
        if !receipts.contains_key(&receipt_ref) {
            self.dispatches.fetch_add(1, Ordering::SeqCst);
            receipts.insert(
                receipt_ref.clone(),
                p::RemoteDriverReceipt {
                    schema_version: p::M4_SCHEMA_VERSION,
                    receipt: receipt_ref.clone(),
                    lease: lease.lease.clone(),
                    dispatch: lease.dispatch.clone(),
                    intent: lease.intent.clone(),
                    plan_digest: lease.plan_digest.clone(),
                    operation_digest: plan.operation.digest.clone(),
                    executor: lease.executor.clone(),
                    authority_epoch: lease.authority_epoch,
                    fence: lease.fence,
                    outcome: p::RemoteReceiptOutcome::Completed,
                    result_digest: Some(result_digest()),
                    observations: vec![p::EvidenceRef("mutation:ordinal:1".into())],
                    observed_at: now_ms(),
                },
            );
            self.by_lease
                .lock()
                .map_err(|_| p::Error("recording lease state is unavailable".into()))?
                .insert(lease.lease.clone(), receipt_ref.clone());
        }
        Ok(p::RemoteDispatchAcceptance {
            schema_version: p::M4_SCHEMA_VERSION,
            dispatch: lease.dispatch.clone(),
            lease: lease.lease.clone(),
            accepted: p::RequiredTrue,
            receipt: receipt_ref,
            authority_epoch: lease.authority_epoch,
            fence: lease.fence,
        })
    }

    fn probe(&self, request: p::RemoteProbeRequest) -> p::Result<p::RemoteProbeResult> {
        request.validate()?;
        let receipt = self
            .by_lease
            .lock()
            .map_err(|_| p::Error("recording lease state is unavailable".into()))?
            .get(&request.lease)
            .cloned();
        Ok(p::RemoteProbeResult {
            schema_version: p::M4_SCHEMA_VERSION,
            lease: request.lease,
            outcome: if receipt.is_some() {
                p::RemoteProbeOutcome::Completed
            } else {
                p::RemoteProbeOutcome::NotDispatched
            },
            receipt,
            evidence: Vec::new(),
        })
    }

    fn cancel(&self, request: p::RemoteCancelRequest) -> p::Result<p::RemoteCancelResult> {
        request.validate()?;
        let completed = self
            .by_lease
            .lock()
            .map_err(|_| p::Error("recording lease state is unavailable".into()))?
            .contains_key(&request.lease);
        Ok(p::RemoteCancelResult {
            schema_version: p::M4_SCHEMA_VERSION,
            lease: request.lease,
            outcome: if completed {
                p::RemoteCancelOutcome::AlreadyCompleted
            } else {
                p::RemoteCancelOutcome::NotDispatched
            },
            evidence: Vec::new(),
        })
    }
}

impl RemoteReceiptSource for RecordingTransport {
    fn receipt(&self, request: p::RemoteReceiptRequest) -> p::Result<p::RemoteDriverReceipt> {
        request.validate()?;
        let receipt = self
            .receipts
            .lock()
            .map_err(|_| p::Error("recording receipt state is unavailable".into()))?
            .get(&request.receipt)
            .cloned()
            .ok_or_else(|| p::Error("recording receipt is unavailable".into()))?;
        if receipt.lease != request.lease
            || receipt.dispatch != request.dispatch
            || receipt.authority_epoch != request.authority_epoch
            || receipt.fence != request.fence
        {
            return Err(p::Error("recording receipt request binding changed".into()));
        }
        Ok(receipt)
    }
}

struct MismatchedGroundTruth;

impl RemoteGroundTruthSource for MismatchedGroundTruth {
    fn observe(
        &self,
        _plan: &p::RemotePlacementPlan,
        _lease: &p::RemoteExecutionLease,
        _receipt: &p::RemoteDriverReceipt,
    ) -> p::Result<Option<RemoteGroundTruth>> {
        Ok(Some(RemoteGroundTruth {
            schema_version: p::M4_SCHEMA_VERSION,
            outcome: p::RemoteReceiptOutcome::Completed,
            result_digest: Some(p::SchemaDigest("sha256:independent-mismatch".into())),
            evidence: vec![p::EvidenceRef("ground-truth:mismatch".into())],
            verification: vec![p::EvidenceRef("verification:mismatch".into())],
        }))
    }
}

fn owner_command_envelope(
    owner_client: &p::FederatedPeerGrant,
    command: &p::FederatedOwnerCommand,
    nonce: &str,
    now: i64,
) -> p::FederatedControlEnvelope {
    p::FederatedControlEnvelope {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: owner_client.peer.clone(),
        session: p::FederatedSessionRef("federated-session:owner".into()),
        owner: p::VerifiedPrincipal(OWNER.into()),
        nonce: p::Nonce(nonce.into()),
        expires_at: now.saturating_add(60_000),
        command_digest: p::canonical_digest(command).unwrap(),
    }
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

fn append_test_event(
    store: &SqliteEventStore,
    run: &p::RunId,
    id: &str,
    payload: p::EventPayload,
) -> p::EventId {
    store
        .append(p::Event::new(
            p::EventId(id.into()),
            run.clone(),
            None,
            payload,
            p::M4_SCHEMA_VERSION,
            now_ms(),
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        ))
        .unwrap()
}

fn assert_subsequence(actual: &[p::EventKind], expected: &[p::EventKind]) {
    let mut at = 0;
    for kind in actual {
        if expected.get(at) == Some(kind) {
            at += 1;
        }
    }
    assert_eq!(at, expected.len(), "actual event sequence: {actual:?}");
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

fn process_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "forme-m4-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
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
        "forme-executord" => std::env::var_os("FORME_EXECUTORD_BIN"),
        "forme-replicad" => std::env::var_os("FORME_REPLICAD_BIN"),
        _ => None,
    };
    if let Some(path) = configured {
        return PathBuf::from(path);
    }
    let profile = std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf();
    profile.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_ready(child: &mut Child, marker: &Path) {
    for _ in 0..500 {
        if marker.is_file() {
            return;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("executor process exited before ready: {status}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("executor process did not become ready");
}

fn free_loopback_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn run_replicad_process(
    database: &Path,
    replica: &p::FederatedPeerGrant,
    epoch: p::AuthorityEpoch,
    batch: &p::ReplicationBatch,
) -> Output {
    let mut child = Command::new(built_binary("forme-replicad"))
        .env("FORME_REPLICA_PEER", &replica.peer.0)
        .env("FORME_REPLICA_GRANT", replica.reference().unwrap().0)
        .env("FORME_AUTHORITY_EPOCH", epoch.0.to_string())
        .env("FORME_REPLICA_SCOPES", SCOPE)
        .env("FORME_REPLICA_ALLOW_OWNER_VIEW", "false")
        .env("FORME_REPLICA_DB_PATH", database)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(batch).unwrap())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn authority_events(store: &SqliteEventStore, runs: &[p::RunId]) -> Vec<p::Event> {
    runs.iter()
        .flat_map(|run| {
            store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()
                .unwrap()
        })
        .collect()
}

fn prepare(fixture: &RuntimeFixture, run: &str) -> forme_harness::RemoteActionSubmission {
    let snapshot = fixture.runtime.snapshot(p::Scope(SCOPE.into())).unwrap();
    let plan = placement(&fixture.executor, snapshot.authority_epoch);
    fixture
        .runtime
        .configure_executor_candidate(
            candidate(&fixture.executor, &plan, fixture.now),
            plan.clone(),
        )
        .unwrap();
    let action = intent(plan, fixture.now);
    let mut run_request = request();
    run_request.idempotency_key = Some(p::IdempotencyKey(format!("m4-remote-once:{run}")));
    fixture
        .runtime
        .prepare_remote_action(
            p::RunId(run.into()),
            run_request,
            action.clone(),
            &governance(&action, fixture.now),
            &harness_config(),
            &FixedCompetenceGate::new(InterventionLevel::L5HighImpact),
            &CompetenceInputs::default(),
        )
        .unwrap()
}

fn approve(
    fixture: &RuntimeFixture,
    submission: &forme_harness::RemoteActionSubmission,
    nonce: &str,
) -> forme_harness::FederationControlResult {
    let command = p::FederatedOwnerCommand::ResolveApproval {
        approval: submission.approval.clone(),
        plan_digest: submission.plan_digest.clone(),
        outcome: p::ApprovalOutcome::Granted,
    };
    fixture
        .runtime
        .apply_owner_command(
            owner_command_envelope(&fixture.owner_client, &command, nonce, fixture.now),
            command,
            fixture.now,
        )
        .unwrap()
}

#[test]
fn s70_enrollment_binds_real_snapshot_and_stale_cas_or_revoke_fails_closed() {
    let now = now_ms();
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let runtime = FederatedHarnessRuntime::new(store.clone());
    let before = runtime.snapshot(p::Scope(SCOPE.into())).unwrap();
    let mut wrong_owner = grant(
        "peer:s70-wrong-owner",
        vec![p::FederatedPeerRole::Executor],
        1,
        now,
    );
    wrong_owner.owner = p::VerifiedPrincipal("owner:other".into());
    let wrong_owner_run = p::RunId("run:s70-wrong-owner".into());
    assert!(runtime
        .register_peer(
            wrong_owner_run.clone(),
            wrong_owner,
            None,
            version(0),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .is_err());
    assert!(event_kinds(&store, &wrong_owner_run).is_empty());

    let mut expired = grant(
        "peer:s70-expired",
        vec![p::FederatedPeerRole::Executor],
        1,
        now,
    );
    expired.expires_at = now.saturating_sub(1);
    let expired_run = p::RunId("run:s70-expired".into());
    assert!(runtime
        .register_peer(
            expired_run.clone(),
            expired,
            None,
            version(0),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .is_err());
    assert!(event_kinds(&store, &expired_run).is_empty());

    let mut unbounded = grant(
        "peer:s70-unbounded",
        vec![p::FederatedPeerRole::Executor],
        1,
        now,
    );
    unbounded.expires_at = now.saturating_add(91 * 24 * 60 * 60 * 1_000);
    let unbounded_run = p::RunId("run:s70-unbounded".into());
    assert!(runtime
        .register_peer(
            unbounded_run.clone(),
            unbounded,
            None,
            version(0),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .is_err());
    assert!(event_kinds(&store, &unbounded_run).is_empty());

    let mut duplicate_role = grant(
        "peer:s70-duplicate-role",
        vec![
            p::FederatedPeerRole::Executor,
            p::FederatedPeerRole::Executor,
        ],
        1,
        now,
    );
    duplicate_role.expires_at = now.saturating_add(1_000);
    let duplicate_role_run = p::RunId("run:s70-duplicate-role".into());
    assert!(runtime
        .register_peer(
            duplicate_role_run.clone(),
            duplicate_role,
            None,
            version(0),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .is_err());
    assert!(event_kinds(&store, &duplicate_role_run).is_empty());

    let executor = grant(
        "peer:s70-executor",
        vec![p::FederatedPeerRole::Executor],
        1,
        now,
    );
    let register_run = p::RunId("run:s70-register".into());
    runtime
        .register_peer(
            register_run.clone(),
            executor.clone(),
            None,
            version(0),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    assert_eq!(
        event_kinds(&store, &register_run),
        vec![
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::FederatedPeerRegistered,
            p::EventKind::RunComplete,
        ]
    );
    let events = store
        .read_run(register_run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    let bound = events
        .iter()
        .find_map(|event| match &event.payload {
            p::EventPayload::SessionBound(payload) => payload.federation_snapshot.as_ref(),
            _ => None,
        })
        .unwrap();
    assert_eq!(bound.0, before.digest.0);
    assert!(!bound.0.starts_with("pending:"));

    let mut unauthorized_expansion = executor.clone();
    unauthorized_expansion.roles = vec![
        p::FederatedPeerRole::Executor,
        p::FederatedPeerRole::Replica,
    ];
    unauthorized_expansion.authority_epoch = p::AuthorityEpoch(2);
    unauthorized_expansion.grant_version = p::PeerGrantVersion(2);
    let expansion_run = p::RunId("run:s70-role-expansion".into());
    assert!(runtime
        .register_peer(
            expansion_run.clone(),
            unauthorized_expansion,
            Some(executor.reference().unwrap()),
            version(1),
            p::VerifiedPrincipal("external:worker".into()),
        )
        .is_err());
    assert!(event_kinds(&store, &expansion_run).is_empty());

    let stale_run = p::RunId("run:s70-stale".into());
    assert!(runtime
        .register_peer(
            stale_run.clone(),
            grant(
                "peer:s70-stale",
                vec![p::FederatedPeerRole::Executor],
                2,
                now,
            ),
            None,
            version(0),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .is_err());
    assert!(event_kinds(&store, &stale_run).is_empty());

    let revoke_run = p::RunId("run:s70-revoke".into());
    runtime
        .revoke_peer(
            revoke_run.clone(),
            executor.peer.clone(),
            executor.reference().unwrap(),
            p::InFlightDisposition::Cancel,
            version(1),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    assert_eq!(
        event_kinds(&store, &revoke_run),
        vec![
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::FederatedPeerRevoked,
            p::EventKind::RunComplete,
        ]
    );
    assert!(runtime
        .snapshot(p::Scope(SCOPE.into()))
        .unwrap()
        .grants
        .is_empty());
}

#[test]
fn s71_s72_remote_action_dispatches_once_then_recovers_the_original_receipt() {
    let fixture = runtime_fixture();
    let transport = Arc::new(RecordingTransport::default());
    fixture.runtime.configure_remote(transport.clone()).unwrap();
    let run = p::RunId("run:s71-s72".into());
    let submission = prepare(&fixture, &run.0);
    assert_eq!(transport.dispatches(), 0);
    assert_subsequence(
        &event_kinds(&fixture.store, &run),
        &[
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::ResourcePlanned,
            p::EventKind::DecisionTraceRecorded,
            p::EventKind::ToolCallProposed,
            p::EventKind::ToolPolicyEvaluated,
            p::EventKind::ActionPlanned,
            p::EventKind::ApprovalRequested,
            p::EventKind::RunWaiting,
        ],
    );
    assert_eq!(
        submission.placement_decision.chosen,
        Some(fixture.executor.peer.clone())
    );

    let resolved = approve(&fixture, &submission, "nonce:s71-approve");
    assert!(matches!(
        resolved,
        forme_harness::FederationControlResult::ApprovalResolved {
            terminal: false,
            ..
        }
    ));
    assert_eq!(transport.dispatches(), 1);
    let unknown_kinds = event_kinds(&fixture.store, &run);
    assert_subsequence(
        &unknown_kinds,
        &[
            p::EventKind::ApprovalResolved,
            p::EventKind::RunResumed,
            p::EventKind::CompetenceGateEvaluated,
            p::EventKind::RemoteExecutionLeaseChanged,
            p::EventKind::ActionStarted,
            p::EventKind::ActionOutcomeUnknown,
            p::EventKind::RunWaiting,
        ],
    );

    let recovered = fixture
        .runtime
        .recover_remote_action(
            &run,
            Some(RemoteGroundTruth {
                schema_version: p::M4_SCHEMA_VERSION,
                outcome: p::RemoteReceiptOutcome::Completed,
                result_digest: Some(result_digest()),
                evidence: vec![p::EvidenceRef("ground-truth:ordinal:1".into())],
                verification: vec![p::EvidenceRef("verification:fixture-state".into())],
            }),
        )
        .unwrap();
    assert!(recovered.terminal);
    assert_eq!(transport.dispatches(), 1);
    let terminal_kinds = event_kinds(&fixture.store, &run);
    assert_subsequence(
        &terminal_kinds,
        &[
            p::EventKind::ActionOutcomeUnknown,
            p::EventKind::RunWaiting,
            p::EventKind::RunResumed,
            p::EventKind::ActionCompleted,
            p::EventKind::RemoteExecutionLeaseChanged,
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
            p::EventKind::RunComplete,
        ],
    );
    assert_eq!(
        terminal_kinds
            .iter()
            .filter(|kind| **kind == p::EventKind::ActionStarted)
            .count(),
        1
    );
    fixture.runtime.recover_remote_action(&run, None).unwrap();
    assert_eq!(transport.dispatches(), 1);
}

#[test]
fn s71_plan_schema_grant_and_epoch_drift_stop_before_remote_driver() {
    let fixture = runtime_fixture();
    let transport = Arc::new(RecordingTransport::default());
    fixture.runtime.configure_remote(transport.clone()).unwrap();
    let run = p::RunId("run:s71-drift".into());
    let submission = prepare(&fixture, &run.0);
    let waiting_events = event_kinds(&fixture.store, &run);

    let changed_plan = p::FederatedOwnerCommand::ResolveApproval {
        approval: submission.approval.clone(),
        plan_digest: p::PlanDigest("sha256:changed-plan".into()),
        outcome: p::ApprovalOutcome::Granted,
    };
    assert!(fixture
        .runtime
        .apply_owner_command(
            owner_command_envelope(
                &fixture.owner_client,
                &changed_plan,
                "nonce:s71-plan-drift",
                fixture.now,
            ),
            changed_plan,
            fixture.now,
        )
        .is_err());

    let valid_command = p::FederatedOwnerCommand::ResolveApproval {
        approval: submission.approval.clone(),
        plan_digest: submission.plan_digest.clone(),
        outcome: p::ApprovalOutcome::Granted,
    };
    let mut bad_schema = owner_command_envelope(
        &fixture.owner_client,
        &valid_command,
        "nonce:s71-schema-drift",
        fixture.now,
    );
    bad_schema.schema_version = p::SchemaVersion(0);
    assert!(fixture
        .runtime
        .apply_owner_command(bad_schema, valid_command.clone(), fixture.now)
        .is_err());
    assert_eq!(transport.dispatches(), 0);
    assert_eq!(event_kinds(&fixture.store, &run), waiting_events);

    let mut updated_executor = fixture.executor.clone();
    updated_executor.authority_epoch = p::AuthorityEpoch(3);
    updated_executor.grant_version = p::PeerGrantVersion(2);
    updated_executor.created_by = p::OwnerControlRef("owner-control:s71-refresh".into());
    fixture
        .runtime
        .register_peer(
            p::RunId("owner-control:s71-refresh".into()),
            updated_executor,
            Some(fixture.executor.reference().unwrap()),
            version(2),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    assert!(fixture
        .runtime
        .apply_owner_command(
            owner_command_envelope(
                &fixture.owner_client,
                &valid_command,
                "nonce:s71-after-grant-drift",
                fixture.now,
            ),
            valid_command,
            fixture.now,
        )
        .is_err());
    assert_eq!(transport.dispatches(), 0);
    assert_eq!(event_kinds(&fixture.store, &run), waiting_events);
}

#[test]
fn s80_authority_filters_candidates_and_records_selection_before_action_plan() {
    let fixture = runtime_fixture();
    let denied = grant(
        "peer:s80-denied",
        vec![p::FederatedPeerRole::Executor],
        3,
        fixture.now,
    );
    fixture
        .runtime
        .register_peer(
            p::RunId("owner-control:s80-denied".into()),
            denied.clone(),
            None,
            version(2),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    let snapshot = fixture.runtime.snapshot(p::Scope(SCOPE.into())).unwrap();
    let allowed_plan = placement(&fixture.executor, snapshot.authority_epoch);
    let denied_plan = placement(&denied, snapshot.authority_epoch);
    let mut allowed_candidate = candidate(&fixture.executor, &allowed_plan, fixture.now);
    allowed_candidate.score_basis_points = 100;
    let mut denied_candidate = candidate(&denied, &denied_plan, fixture.now);
    denied_candidate.score_basis_points = 10_000;
    denied_candidate.managed_policy_allowed = false;
    fixture
        .runtime
        .configure_executor_candidate(allowed_candidate, allowed_plan.clone())
        .unwrap();
    fixture
        .runtime
        .configure_executor_candidate(denied_candidate, denied_plan)
        .unwrap();
    fixture
        .runtime
        .configure_remote(Arc::new(RecordingTransport::default()))
        .unwrap();
    let run = p::RunId("run:s80-authority-placement".into());
    let action = intent(allowed_plan, fixture.now);
    let submission = fixture
        .runtime
        .prepare_remote_action(
            run.clone(),
            request(),
            action.clone(),
            &governance(&action, fixture.now),
            &harness_config(),
            &FixedCompetenceGate::new(InterventionLevel::L5HighImpact),
            &CompetenceInputs::default(),
        )
        .unwrap();
    assert_eq!(
        submission.placement_decision.chosen,
        Some(fixture.executor.peer.clone())
    );
    let denied_trace = submission
        .placement_decision
        .candidates
        .iter()
        .find(|trace| trace.candidate.peer == denied.peer)
        .unwrap();
    assert!(!denied_trace.eligible);
    assert!(denied_trace
        .reasons
        .contains(&p::PlacementFilterReason::ManagedPolicyDenied));
    assert_subsequence(
        &event_kinds(&fixture.store, &run),
        &[
            p::EventKind::SessionBound,
            p::EventKind::ResourcePlanned,
            p::EventKind::DecisionTraceRecorded,
            p::EventKind::ActionPlanned,
            p::EventKind::ApprovalRequested,
            p::EventKind::RunWaiting,
        ],
    );
}

#[test]
fn s73_mismatched_ground_truth_records_failure_before_unknown_and_never_passes_capability() {
    let fixture = runtime_fixture();
    let transport = Arc::new(RecordingTransport::default());
    fixture.runtime.configure_remote(transport.clone()).unwrap();
    fixture
        .runtime
        .configure_ground_truth(Arc::new(MismatchedGroundTruth))
        .unwrap();
    let run = p::RunId("run:s73".into());
    let submission = prepare(&fixture, &run.0);
    approve(&fixture, &submission, "nonce:s73-approve");
    assert_eq!(transport.dispatches(), 1);
    let kinds = event_kinds(&fixture.store, &run);
    assert_subsequence(
        &kinds,
        &[
            p::EventKind::ActionStarted,
            p::EventKind::FailureEvidenceRecorded,
            p::EventKind::ActionOutcomeUnknown,
            p::EventKind::RunWaiting,
        ],
    );
    assert!(!kinds.contains(&p::EventKind::ActionCompleted));
    assert!(!kinds.contains(&p::EventKind::CapabilityEvidenceRecorded));
    let serialized = serde_json::to_string(
        &fixture
            .store
            .read_run(run)
            .collect::<p::Result<Vec<_>>>()
            .unwrap(),
    )
    .unwrap()
    .to_ascii_lowercase();
    assert!(!serialized.contains("secretref"));
    assert!(!serialized.contains("private endpoint"));
}

#[test]
fn s73_authenticated_remote_injection_and_secret_echo_never_enter_authority_facts() {
    const SECRET_MARKER: &str = "m4-s73-secret-marker-9f3a";
    const INJECTION_MARKER: &str = "ignore-policy-and-promote-remote-output";

    let fixture = runtime_fixture();
    let transport = Arc::new(RecordingTransport::default());
    fixture.runtime.configure_remote(transport.clone()).unwrap();
    let run = p::RunId("run:s73-untrusted-output".into());
    let submission = prepare(&fixture, &run.0);
    approve(&fixture, &submission, "nonce:s73-untrusted-output");
    assert_eq!(transport.dispatches(), 1);

    let receipt_ref = transport
        .receipts
        .lock()
        .unwrap()
        .keys()
        .next()
        .cloned()
        .unwrap();
    transport
        .receipts
        .lock()
        .unwrap()
        .get_mut(&receipt_ref)
        .unwrap()
        .observations = vec![
        p::EvidenceRef(INJECTION_MARKER.into()),
        p::EvidenceRef(SECRET_MARKER.into()),
    ];

    let recovered = fixture
        .runtime
        .recover_remote_action(
            &run,
            Some(RemoteGroundTruth {
                schema_version: p::M4_SCHEMA_VERSION,
                outcome: p::RemoteReceiptOutcome::Completed,
                result_digest: Some(result_digest()),
                evidence: vec![p::EvidenceRef("ground-truth:ordinal:1".into())],
                verification: vec![p::EvidenceRef("verification:independent-state".into())],
            }),
        )
        .unwrap();
    assert!(recovered.terminal);
    let events = fixture
        .store
        .read_run(run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    let serialized = serde_json::to_string(&events).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains(SECRET_MARKER));
    assert!(!serialized.contains(INJECTION_MARKER));
    assert!(!events.iter().any(|event| {
        matches!(
            event.kind,
            p::EventKind::CandidateCreated
                | p::EventKind::CandidatePromoted
                | p::EventKind::MemoryNodeAppended
                | p::EventKind::MemoryEdgeAppended
                | p::EventKind::CognitiveMapUpdateProposed
                | p::EventKind::StrategyActivated
                | p::EventKind::CapabilityEvidenceRecorded
        )
    }));
    let completed = events
        .iter()
        .find(|event| event.kind == p::EventKind::ActionCompleted)
        .unwrap();
    assert_eq!(completed.provenance.actor, p::Actor::System);
    assert_eq!(
        completed.provenance.trust_tier,
        p::TrustTier::VerifiedProcess
    );
}

#[test]
fn s78_s82_retention_is_honest_and_device_signals_have_one_authority_decision() {
    let fixture = runtime_fixture();
    let replica = grant(
        "peer:replica",
        vec![p::FederatedPeerRole::Replica],
        3,
        fixture.now,
    );
    fixture
        .runtime
        .register_peer(
            p::RunId("owner-control:register-replica".into()),
            replica.clone(),
            None,
            version(2),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    fixture
        .runtime
        .revoke_peer(
            p::RunId("owner-control:revoke-replica".into()),
            replica.peer.clone(),
            replica.reference().unwrap(),
            p::InFlightDisposition::KeepPinned,
            version(3),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    let epoch = fixture
        .runtime
        .snapshot(p::Scope(SCOPE.into()))
        .unwrap()
        .authority_epoch;
    let mut retention = p::FederatedRetentionRequest {
        schema_version: p::M4_SCHEMA_VERSION,
        request: p::RetentionRequestRef("retention:s78".into()),
        peer: replica.peer.clone(),
        scope: p::Scope(SCOPE.into()),
        authority_epoch: epoch,
        requested_by: p::OwnerControlRef("owner-control:s78".into()),
        expires_at: fixture.now.saturating_add(60_000),
        digest: p::SchemaDigest(String::new()),
    };
    retention.refresh_digest().unwrap();
    let command = p::FederatedOwnerCommand::RequestRetention(retention.clone());
    fixture
        .runtime
        .apply_owner_command(
            owner_command_envelope(
                &fixture.owner_client,
                &command,
                "nonce:s78-retention",
                fixture.now,
            ),
            command,
            fixture.now,
        )
        .unwrap();
    assert_eq!(
        fixture
            .runtime
            .retention_state(&retention.request)
            .unwrap()
            .unwrap()
            .status,
        p::RetentionStatus::Requested
    );
    let mut receipt = p::FederatedRetentionReceipt {
        schema_version: p::M4_SCHEMA_VERSION,
        receipt: p::RetentionReceiptRef("retention-receipt:s78".into()),
        request: retention.request.clone(),
        peer: replica.peer.clone(),
        authority_epoch: epoch,
        deleted_projection: p::ReplicaProjectionDigestRef("sha256:deleted-projection".into()),
        evidence: vec![p::EvidenceRef("remote-delete:verified".into())],
        observed_at: fixture.now,
        digest: p::SchemaDigest(String::new()),
    };
    receipt.refresh_digest().unwrap();
    assert!(fixture
        .runtime
        .accept_retention_receipt(&fixture.owner_client.peer, receipt.clone(), fixture.now,)
        .is_err());
    assert_eq!(
        fixture
            .runtime
            .retention_state(&retention.request)
            .unwrap()
            .unwrap()
            .status,
        p::RetentionStatus::Requested
    );
    fixture
        .runtime
        .accept_retention_receipt(&replica.peer, receipt, fixture.now)
        .unwrap();
    assert_eq!(
        fixture
            .runtime
            .retention_state(&retention.request)
            .unwrap()
            .unwrap()
            .status,
        p::RetentionStatus::Verified
    );

    let mut signal = p::FederatedDeviceSignal {
        schema_version: p::M4_SCHEMA_VERSION,
        signal: p::FederatedDeviceSignalRef("signal:s82".into()),
        peer: fixture.owner_client.peer.clone(),
        session: p::FederatedSessionRef("session:device".into()),
        kind: p::FederatedSignalKind::Tick,
        nonce: p::Nonce("nonce:s82".into()),
        observed_at: fixture.now,
        expires_at: fixture.now.saturating_add(60_000),
        digest: p::SchemaDigest(String::new()),
    };
    signal.refresh_digest().unwrap();
    assert!(fixture
        .runtime
        .accept_device_signal(&fixture.executor.peer, signal.clone(), fixture.now)
        .is_err());
    assert!(fixture
        .runtime
        .accept_device_signal(&signal.peer, signal.clone(), fixture.now)
        .unwrap());
    assert!(!fixture
        .runtime
        .accept_device_signal(&signal.peer, signal.clone(), fixture.now)
        .unwrap());
    signal.kind = p::FederatedSignalKind::Foreground;
    signal.refresh_digest().unwrap();
    assert!(fixture
        .runtime
        .accept_device_signal(&fixture.owner_client.peer, signal, fixture.now)
        .is_err());
}

#[test]
fn s72_s78_s79_s82_authority_restart_preserves_recovery_and_replay_ledgers() {
    let root = process_root("authority-restart");
    fs::create_dir_all(&root).unwrap();
    let database = root.join("authority.sqlite3");
    let now = now_ms();
    let executor = grant(
        "peer:restart-executor",
        vec![
            p::FederatedPeerRole::Executor,
            p::FederatedPeerRole::Replica,
        ],
        1,
        now,
    );
    let owner_client = grant(
        "peer:restart-owner-client",
        vec![p::FederatedPeerRole::OwnerClient],
        2,
        now,
    );
    let transport = Arc::new(RecordingTransport::default());
    let run = p::RunId("run:authority-restart".into());

    let (
        approval_envelope,
        approval_command,
        preapproval_envelope,
        preapproval_command,
        retention,
        signal,
    ) = {
        let store = SqliteEventStore::open(&database, StoreOptions::default()).unwrap();
        let runtime = Arc::new(FederatedHarnessRuntime::new(store.clone()));
        runtime
            .register_peer(
                p::RunId("owner-control:restart-executor".into()),
                executor.clone(),
                None,
                version(0),
                p::VerifiedPrincipal(OWNER.into()),
            )
            .unwrap();
        runtime
            .register_peer(
                p::RunId("owner-control:restart-owner-client".into()),
                owner_client.clone(),
                None,
                version(1),
                p::VerifiedPrincipal(OWNER.into()),
            )
            .unwrap();
        runtime.configure_remote(transport.clone()).unwrap();
        let fixture = RuntimeFixture {
            store,
            runtime: runtime.clone(),
            executor: executor.clone(),
            owner_client: owner_client.clone(),
            now,
        };
        let submission = prepare(&fixture, &run.0);
        let approval_command = p::FederatedOwnerCommand::ResolveApproval {
            approval: submission.approval.clone(),
            plan_digest: submission.plan_digest.clone(),
            outcome: p::ApprovalOutcome::Granted,
        };
        let approval_envelope = owner_command_envelope(
            &owner_client,
            &approval_command,
            "nonce:authority-restart-approval",
            now,
        );
        runtime
            .apply_owner_command(approval_envelope.clone(), approval_command.clone(), now)
            .unwrap();
        assert_eq!(transport.dispatches(), 1);

        let preapproval = prepare(&fixture, "run:preapproval-restart");
        let preapproval_command = p::FederatedOwnerCommand::ResolveApproval {
            approval: preapproval.approval,
            plan_digest: preapproval.plan_digest,
            outcome: p::ApprovalOutcome::Granted,
        };
        let preapproval_envelope = owner_command_envelope(
            &owner_client,
            &preapproval_command,
            "nonce:preapproval-restart",
            now,
        );

        let mut retention = p::FederatedRetentionRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            request: p::RetentionRequestRef("retention:authority-restart".into()),
            peer: executor.peer.clone(),
            scope: p::Scope(SCOPE.into()),
            authority_epoch: runtime
                .snapshot(p::Scope(SCOPE.into()))
                .unwrap()
                .authority_epoch,
            requested_by: p::OwnerControlRef("owner-control:authority-restart".into()),
            expires_at: now.saturating_add(60_000),
            digest: p::SchemaDigest(String::new()),
        };
        retention.refresh_digest().unwrap();
        let retention_command = p::FederatedOwnerCommand::RequestRetention(retention.clone());
        runtime
            .apply_owner_command(
                owner_command_envelope(
                    &owner_client,
                    &retention_command,
                    "nonce:authority-restart-retention",
                    now,
                ),
                retention_command,
                now,
            )
            .unwrap();

        let mut signal = p::FederatedDeviceSignal {
            schema_version: p::M4_SCHEMA_VERSION,
            signal: p::FederatedDeviceSignalRef("signal:authority-restart".into()),
            peer: owner_client.peer.clone(),
            session: p::FederatedSessionRef("session:authority-restart".into()),
            kind: p::FederatedSignalKind::Tick,
            nonce: p::Nonce("nonce:authority-restart-signal".into()),
            observed_at: now,
            expires_at: now.saturating_add(60_000),
            digest: p::SchemaDigest(String::new()),
        };
        signal.refresh_digest().unwrap();
        assert!(runtime
            .accept_device_signal(&signal.peer, signal.clone(), now)
            .unwrap());
        (
            approval_envelope,
            approval_command,
            preapproval_envelope,
            preapproval_command,
            retention,
            signal,
        )
    };

    let store = SqliteEventStore::open(&database, StoreOptions::default()).unwrap();
    let runtime = Arc::new(FederatedHarnessRuntime::new(store.clone()));
    runtime.configure_remote(transport.clone()).unwrap();
    assert!(runtime
        .apply_owner_command(approval_envelope, approval_command, now)
        .is_err());
    assert!(runtime
        .apply_owner_command(
            preapproval_envelope.clone(),
            preapproval_command.clone(),
            now,
        )
        .is_err());
    assert!(runtime
        .apply_owner_command(preapproval_envelope, preapproval_command, now)
        .is_err());
    assert_eq!(transport.dispatches(), 1);
    assert!(!store
        .read_run(p::RunId("run:preapproval-restart".into()))
        .filter_map(Result::ok)
        .any(|event| event.kind == p::EventKind::ActionStarted));
    assert!(!runtime
        .accept_device_signal(&signal.peer, signal.clone(), now)
        .unwrap());
    assert_eq!(
        runtime
            .retention_state(&retention.request)
            .unwrap()
            .unwrap()
            .status,
        p::RetentionStatus::Requested
    );
    let recovered = runtime
        .recover_remote_action(
            &run,
            Some(RemoteGroundTruth {
                schema_version: p::M4_SCHEMA_VERSION,
                outcome: p::RemoteReceiptOutcome::Completed,
                result_digest: Some(result_digest()),
                evidence: vec![p::EvidenceRef("ground-truth:authority-restart".into())],
                verification: vec![p::EvidenceRef("verification:authority-restart".into())],
            }),
        )
        .unwrap();
    assert!(recovered.terminal);
    assert_eq!(recovered.outcome, p::RemoteReceiptOutcome::Completed);
    assert_eq!(transport.dispatches(), 1);

    let mut receipt = p::FederatedRetentionReceipt {
        schema_version: p::M4_SCHEMA_VERSION,
        receipt: p::RetentionReceiptRef("retention-receipt:authority-restart".into()),
        request: retention.request.clone(),
        peer: executor.peer,
        authority_epoch: retention.authority_epoch,
        deleted_projection: p::ReplicaProjectionDigestRef(
            "sha256:authority-restart-deleted".into(),
        ),
        evidence: vec![p::EvidenceRef("remote-delete:authority-restart".into())],
        observed_at: now,
        digest: p::SchemaDigest(String::new()),
    };
    receipt.refresh_digest().unwrap();
    let receipt_peer = receipt.peer.clone();
    runtime
        .accept_retention_receipt(&receipt_peer, receipt, now)
        .unwrap();
    drop(runtime);
    drop(store);

    let store = SqliteEventStore::open(&database, StoreOptions::default()).unwrap();
    let runtime = FederatedHarnessRuntime::new(store.clone());
    assert_eq!(
        runtime
            .retention_state(&retention.request)
            .unwrap()
            .unwrap()
            .status,
        p::RetentionStatus::Verified
    );
    let signal_peer = signal.peer.clone();
    assert!(!runtime
        .accept_device_signal(&signal_peer, signal, now)
        .unwrap());
    drop(runtime);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn s79_cross_device_owner_control_is_dual_bound_one_shot_and_fences_late_receipts() {
    let fixture = runtime_fixture();
    let second_owner_client = grant(
        "peer:owner-client-b",
        vec![p::FederatedPeerRole::OwnerClient],
        3,
        fixture.now,
    );
    fixture
        .runtime
        .register_peer(
            p::RunId("owner-control:register-owner-client-b".into()),
            second_owner_client.clone(),
            None,
            version(2),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    let transport = Arc::new(RecordingTransport::default());
    fixture.runtime.configure_remote(transport.clone()).unwrap();
    let run = p::RunId("run:s79".into());
    let submission = prepare(&fixture, &run.0);
    let command = p::FederatedOwnerCommand::ResolveApproval {
        approval: submission.approval.clone(),
        plan_digest: submission.plan_digest.clone(),
        outcome: p::ApprovalOutcome::Granted,
    };

    let waiting_events = event_kinds(&fixture.store, &run).len();
    let mut wrong_role = owner_command_envelope(
        &second_owner_client,
        &command,
        "nonce:s79-wrong-role",
        fixture.now,
    );
    wrong_role.peer = fixture.executor.peer.clone();
    assert!(fixture
        .runtime
        .apply_owner_command(wrong_role, command.clone(), fixture.now)
        .is_err());
    assert_eq!(transport.dispatches(), 0);
    assert_eq!(event_kinds(&fixture.store, &run).len(), waiting_events);

    let mut expired = owner_command_envelope(
        &second_owner_client,
        &command,
        "nonce:s79-expired",
        fixture.now,
    );
    expired.expires_at = fixture.now.saturating_sub(1);
    assert!(fixture
        .runtime
        .apply_owner_command(expired, command.clone(), fixture.now)
        .is_err());
    let mut changed = owner_command_envelope(
        &second_owner_client,
        &command,
        "nonce:s79-changed",
        fixture.now,
    );
    changed.command_digest = p::SchemaDigest("sha256:changed".into());
    assert!(fixture
        .runtime
        .apply_owner_command(changed, command.clone(), fixture.now)
        .is_err());
    assert_eq!(transport.dispatches(), 0);
    assert_eq!(event_kinds(&fixture.store, &run).len(), waiting_events);

    let valid = owner_command_envelope(
        &second_owner_client,
        &command,
        "nonce:s79-approve-b",
        fixture.now,
    );
    assert!(matches!(
        fixture
            .runtime
            .apply_owner_command(valid.clone(), command.clone(), fixture.now)
            .unwrap(),
        forme_harness::FederationControlResult::ApprovalResolved {
            terminal: false,
            ..
        }
    ));
    assert_eq!(transport.dispatches(), 1);
    let after_approval = event_kinds(&fixture.store, &run).len();
    assert!(fixture
        .runtime
        .apply_owner_command(valid, command.clone(), fixture.now)
        .is_err());
    let second_response = owner_command_envelope(
        &fixture.owner_client,
        &command,
        "nonce:s79-second-response",
        fixture.now,
    );
    assert!(fixture
        .runtime
        .apply_owner_command(second_response, command, fixture.now)
        .is_err());
    assert_eq!(transport.dispatches(), 1);
    assert_eq!(event_kinds(&fixture.store, &run).len(), after_approval);

    let lease = fixture
        .store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .into_iter()
        .find_map(|event| match event.payload {
            p::EventPayload::RemoteExecutionLeaseChanged(payload)
                if payload.lease.state == p::RemoteLeaseState::Acquired =>
            {
                Some(payload.lease)
            }
            _ => None,
        })
        .unwrap();
    let cancel = p::FederatedOwnerCommand::Cancel {
        lease: lease.lease.clone(),
        reason: p::ReasonRef("owner cancelled from device A".into()),
    };
    let cancelled = fixture
        .runtime
        .apply_owner_command(
            owner_command_envelope(
                &fixture.owner_client,
                &cancel,
                "nonce:s79-cancel-a",
                fixture.now,
            ),
            cancel,
            fixture.now,
        )
        .unwrap();
    assert!(matches!(
        cancelled,
        forme_harness::FederationControlResult::Cancelled {
            terminal: false,
            ..
        }
    ));
    assert_eq!(
        FederationProjection::lease(&fixture.store, &lease.lease)
            .unwrap()
            .unwrap()
            .state,
        p::RemoteLeaseState::Fenced
    );
    let before_late_receipt = event_kinds(&fixture.store, &run);
    assert!(fixture
        .runtime
        .recover_remote_action(
            &run,
            Some(RemoteGroundTruth {
                schema_version: p::M4_SCHEMA_VERSION,
                outcome: p::RemoteReceiptOutcome::Completed,
                result_digest: Some(result_digest()),
                evidence: vec![p::EvidenceRef("ground-truth:late".into())],
                verification: vec![p::EvidenceRef("verification:late".into())],
            }),
        )
        .is_err());
    assert_eq!(event_kinds(&fixture.store, &run), before_late_receipt);
    assert!(!before_late_receipt.contains(&p::EventKind::ActionCompleted));
    assert_eq!(transport.dispatches(), 1);
}

#[test]
fn s81_handoff_requires_a_verified_checkpoint_current_snapshot_and_new_segment() {
    let fixture = runtime_fixture();
    let source_run = p::RunId("run:segment-a".into());
    let source_federation = fixture.runtime.snapshot(p::Scope(SCOPE.into())).unwrap();
    let source_evolution = p::EvolutionSnapshotRef("evolution:s81-source".into());
    append_test_event(
        &fixture.store,
        &source_run,
        "event:s81-run-accepted",
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId("session:s81-segment-a".into()),
            input_ref: p::InputRef("input:s81-segment-a".into()),
            idempotency_key: Some(p::IdempotencyKey("s81-segment-a".into())),
        }),
    );
    append_test_event(
        &fixture.store,
        &source_run,
        "event:s81-session-bound",
        p::EventPayload::SessionBound(p::SessionBoundPayload {
            policy_profile: p::PolicyProfileRef("policy:v1".into()),
            model_profile: p::ModelProfileRef("model:v1".into()),
            toolset_ref: p::ToolsetRef("toolset:v1".into()),
            workspace: p::WorkspaceRef(SCOPE.into()),
            effect_mode: Some(p::EffectMode::LiveGoverned),
            evolution_snapshot: Some(source_evolution.clone()),
            federation_snapshot: Some(p::FederationSnapshotRef(source_federation.digest.0.clone())),
        }),
    );
    let verification_event = append_test_event(
        &fixture.store,
        &source_run,
        "event:s81-verification-pass",
        p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
            verifier_kind: p::VerifierKind("repository-checkpoint".into()),
            outcome: p::VerificationOutcome::Pass,
            against: p::DoneContractRef("done:segment-a".into()),
        }),
    );
    append_test_event(
        &fixture.store,
        &source_run,
        "event:s81-checkpoint-memory",
        p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
            node_id: p::NodeId("checkpoint-node:s81".into()),
            kind: p::MemoryNodeType("checkpoint".into()),
            content_ref: p::ContentRef("artifact:checkpoint:s81".into()),
            tier: p::StabilityTier::Working,
            confidence: p::Confidence(1.0),
            scope: p::Scope(SCOPE.into()),
            resting_activation: p::RestingActivation(0.5),
            recency: p::Recency(fixture.now),
        }),
    );
    let mut checkpoint = p::FederatedCheckpointArtifact {
        schema_version: p::M4_SCHEMA_VERSION,
        reference: p::FederatedCheckpointArtifactRef("checkpoint-artifact:s81".into()),
        checkpoint: p::GoalCheckpoint {
            schema_version: p::M4_SCHEMA_VERSION,
            reference: p::GoalCheckpointRef("checkpoint:s81".into()),
            goal_frame: p::GoalFrameRef("goal-frame:s81".into()),
            intention: p::IntentionId("intention:s81".into()),
            route: p::ExecutionRouteRef("route:segment-a".into()),
            artifact: p::ContentRef("artifact:checkpoint:s81".into()),
            situation_digest: p::SchemaDigest("sha256:situation:s81".into()),
            evidence_refs: vec![verification_event.clone()],
            created_at: fixture.now,
        },
        done_contract: p::DoneContractRef("done:segment-a".into()),
        verification_outcome: p::VerificationOutcome::Pass,
        verification_events: vec![verification_event],
        artifacts: vec![p::ContentRef("artifact:checkpoint:s81".into())],
        spent_budget: p::Budget("units:1".into()),
        remaining_budget: p::Budget("units:1".into()),
        external_effects: vec![p::EvidenceRef("effect:ordinal:1".into())],
        scope: p::Scope(SCOPE.into()),
        policy: p::PolicyProfileRef("policy:v1".into()),
        toolset: p::ToolsetRef("toolset:v1".into()),
        model: p::ModelProfileRef("model:v1".into()),
        evolution_snapshot: source_evolution.clone(),
        federation_snapshot: p::FederationSnapshotRef(source_federation.digest.0),
        digest: p::SchemaDigest(String::new()),
    };
    checkpoint.refresh_digest().unwrap();
    fixture
        .runtime
        .record_checkpoint(&source_run, checkpoint.clone())
        .unwrap();

    let handoff_marker = grant(
        "peer:s81-handoff-marker",
        vec![p::FederatedPeerRole::Replica],
        3,
        fixture.now,
    );
    fixture
        .runtime
        .register_peer(
            p::RunId("owner-control:s81-handoff-marker".into()),
            handoff_marker,
            None,
            version(2),
            p::VerifiedPrincipal(OWNER.into()),
        )
        .unwrap();
    let current = fixture.runtime.snapshot(p::Scope(SCOPE.into())).unwrap();
    let next_plan = placement(&fixture.executor, current.authority_epoch);
    let handoff = fixture
        .runtime
        .plan_handoff(
            &checkpoint.reference,
            source_run,
            p::RunId("run:segment-b".into()),
            fixture.executor.peer.clone(),
            next_plan.reference().unwrap(),
            source_evolution.clone(),
            p::FederationSnapshotRef(current.digest.0.clone()),
            p::Budget("units:1".into()),
        )
        .unwrap();
    assert_eq!(handoff.next_run.0, "run:segment-b");
    assert_eq!(handoff.evolution_snapshot, source_evolution);
    assert!(fixture
        .runtime
        .plan_handoff(
            &checkpoint.reference,
            p::RunId("run:segment-a".into()),
            p::RunId("run:segment-c".into()),
            fixture.executor.peer.clone(),
            p::RemotePlacementPlanRef("placement:segment-c".into()),
            handoff.evolution_snapshot.clone(),
            p::FederationSnapshotRef("federation:stale".into()),
            p::Budget("units:1".into()),
        )
        .is_err());

    let resumed_runtime = Arc::new(FederatedHarnessRuntime::new(fixture.store.clone()));
    resumed_runtime
        .configure_executor_candidate(
            candidate(&fixture.executor, &next_plan, fixture.now),
            next_plan.clone(),
        )
        .unwrap();
    let transport = Arc::new(RecordingTransport::default());
    resumed_runtime.configure_remote(transport.clone()).unwrap();
    let mut next_request = request();
    next_request.session = p::SessionRef("session:s81-segment-b".into());
    next_request.budget = Some(handoff.budget.clone());
    next_request.idempotency_key = Some(p::IdempotencyKey("s81-segment-b".into()));
    let submission = resumed_runtime
        .prepare_remote_action(
            handoff.next_run.clone(),
            next_request,
            intent(next_plan, fixture.now),
            &governance(
                &intent(
                    placement(&fixture.executor, current.authority_epoch),
                    fixture.now,
                ),
                fixture.now,
            ),
            &harness_config(),
            &FixedCompetenceGate::new(InterventionLevel::L5HighImpact),
            &CompetenceInputs::default(),
        )
        .unwrap();
    let approval = p::FederatedOwnerCommand::ResolveApproval {
        approval: submission.approval.clone(),
        plan_digest: submission.plan_digest.clone(),
        outcome: p::ApprovalOutcome::Granted,
    };
    resumed_runtime
        .apply_owner_command(
            owner_command_envelope(
                &fixture.owner_client,
                &approval,
                "nonce:s81-segment-b",
                fixture.now,
            ),
            approval,
            fixture.now,
        )
        .unwrap();
    assert_eq!(transport.dispatches(), 1);
    let segment_events = fixture
        .store
        .read_run(handoff.next_run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_subsequence(
        &segment_events
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        &[
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::ResourcePlanned,
            p::EventKind::DecisionTraceRecorded,
            p::EventKind::ActionPlanned,
            p::EventKind::ApprovalRequested,
            p::EventKind::ApprovalResolved,
            p::EventKind::RemoteExecutionLeaseChanged,
            p::EventKind::ActionStarted,
        ],
    );
    let bound = segment_events
        .iter()
        .find_map(|event| match &event.payload {
            p::EventPayload::SessionBound(payload) => Some(payload),
            _ => None,
        });
    assert_eq!(
        bound.and_then(|payload| payload.evolution_snapshot.as_ref()),
        Some(&handoff.evolution_snapshot)
    );
}

#[test]
#[ignore = "requires explicitly built M4 executor and replica process binaries"]
fn s74_s83_three_process_tls_unknown_recovery_replication_revoke_and_artifacts() {
    let root = process_root("federated-golden");
    fs::create_dir_all(&root).unwrap();
    let artifact_root = std::env::var_os("FORME_M4_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("artifacts"));
    let pki = process_pki();
    let authority_identity = transport_identity_digest(&pki.authority.certificate);
    let executor_identity = transport_identity_digest(&pki.executor.certificate);
    let authority_files = write_process_identity(&root, "authority", &pki.authority, &pki.ca);
    let executor_files = write_process_identity(&root, "executor", &pki.executor, &pki.ca);
    let now = now_ms();

    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let mut executor = grant(
        "peer:golden-executor",
        vec![p::FederatedPeerRole::Executor],
        1,
        now,
    );
    executor.transport_identity = executor_identity.clone();
    let owner_client = grant(
        "peer:golden-owner-client",
        vec![p::FederatedPeerRole::OwnerClient],
        2,
        now,
    );
    let replica = grant(
        "peer:golden-replica",
        vec![p::FederatedPeerRole::Replica],
        3,
        now,
    );
    let bootstrap_action = intent(placement(&executor, p::AuthorityEpoch(3)), now);
    let harness = Arc::new(
        ReactiveHarness::new(
            store.clone(),
            Arc::new(ScriptedModelProvider::new(profile(), Vec::new()).unwrap()),
            Arc::new(ExecutionBackendRegistry::default()),
            governance(&bootstrap_action, now),
            harness_config(),
        )
        .unwrap(),
    );
    let runtime = harness.federation_runtime();
    let register_executor = p::RunId("owner-control:golden-executor".into());
    let register_owner = p::RunId("owner-control:golden-owner-client".into());
    let register_replica = p::RunId("owner-control:golden-replica".into());
    FederationGatewayControl::register_federated_peer(
        harness.as_ref(),
        register_executor.clone(),
        executor.clone(),
        None,
        version(0),
        p::VerifiedPrincipal(OWNER.into()),
    )
    .unwrap();
    FederationGatewayControl::register_federated_peer(
        harness.as_ref(),
        register_owner.clone(),
        owner_client.clone(),
        None,
        version(1),
        p::VerifiedPrincipal(OWNER.into()),
    )
    .unwrap();
    FederationGatewayControl::register_federated_peer(
        harness.as_ref(),
        register_replica.clone(),
        replica.clone(),
        None,
        version(2),
        p::VerifiedPrincipal(OWNER.into()),
    )
    .unwrap();
    let current =
        FederationGatewayControl::federation_snapshot(harness.as_ref(), p::Scope(SCOPE.into()))
            .unwrap();
    assert_eq!(current.authority_epoch, p::AuthorityEpoch(3));

    let grant_path = root.join("executor-grant.json");
    fs::write(&grant_path, serde_json::to_vec(&executor).unwrap()).unwrap();
    let state_path = root.join("mutation-state.json");
    let ledger_root = root.join("executor-ledger");
    let replay_root = root.join("executor-wire-replay");
    let ready_path = root.join("executor.ready");
    let port = free_loopback_port();
    let mut executor_process = ChildGuard(
        Command::new(built_binary("forme-executord"))
            .env("FORME_EXECUTOR_GRANT_PATH", &grant_path)
            .env("FORME_EXECUTOR_PEER", &executor.peer.0)
            .env("FORME_EXECUTOR_LOGICAL_PATH", "golden/state")
            .env("FORME_EXECUTOR_STATE_PATH", &state_path)
            .env("FORME_EXECUTOR_PROFILE", "profile:m4-fixture")
            .env(
                "FORME_AUTHORITY_EPOCH",
                current.authority_epoch.0.to_string(),
            )
            .env("FORME_EXECUTOR_LEDGER_ROOT", &ledger_root)
            .env("FORME_EXECUTOR_BIND", format!("127.0.0.1:{port}"))
            .env("FORME_AUTHORITY_ID", "authority:local")
            .env("FORME_EXPECTED_AUTHORITY_IDENTITY", &authority_identity.0)
            .env("FORME_EXECUTOR_CERT_DER", &executor_files.certificate_der)
            .env("FORME_EXECUTOR_KEY_DER", &executor_files.private_key_der)
            .env("FORME_AUTHORITY_CA_DER", &executor_files.trust_anchor_der)
            .env("FORME_EXECUTOR_REPLAY_ROOT", &replay_root)
            .env("FORME_EXECUTOR_TIMEOUT_MS", "10000")
            .env("FORME_EXECUTOR_MAX_BODY_BYTES", "1048576")
            .env("FORME_EXECUTOR_READY_PATH", &ready_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    wait_for_ready(&mut executor_process.0, &ready_path);

    let transport = Arc::new(
        TlsRemoteTransport::new(TlsRemoteClientConfig {
            endpoint: format!("https://localhost:{port}"),
            authority: p::AuthorityRef("authority:local".into()),
            peer: executor.peer.clone(),
            expected_peer_identity: executor_identity,
            identity: authority_files,
            timeout: p::DurationMs(10_000),
            request_ttl: p::DurationMs(5_000),
            max_body_bytes: 1_048_576,
        })
        .unwrap(),
    );
    runtime.configure_remote(transport).unwrap();
    let action_run = p::RunId("run:s83-federated-golden".into());
    let action_plan = placement(&executor, current.authority_epoch);
    runtime
        .configure_executor_candidate(candidate(&executor, &action_plan, now), action_plan.clone())
        .unwrap();
    let action = intent(action_plan, now);
    let submission = FederationActionGateway::submit_federated_remote_action(
        harness.as_ref(),
        action_run.clone(),
        request(),
        action,
    )
    .unwrap();
    let approval = p::FederatedOwnerCommand::ResolveApproval {
        approval: submission.approval.clone(),
        plan_digest: submission.plan_digest.clone(),
        outcome: p::ApprovalOutcome::Granted,
    };
    assert!(matches!(
        FederationGatewayControl::apply_federated_owner_command(
            harness.as_ref(),
            owner_command_envelope(&owner_client, &approval, "nonce:s83-approve", now,),
            approval,
            now,
        )
        .unwrap(),
        forme_harness::FederationControlResult::ApprovalResolved {
            terminal: false,
            ..
        }
    ));
    assert_subsequence(
        &event_kinds(&store, &action_run),
        &[
            p::EventKind::ApprovalResolved,
            p::EventKind::RemoteExecutionLeaseChanged,
            p::EventKind::ActionStarted,
            p::EventKind::ActionOutcomeUnknown,
            p::EventKind::RunWaiting,
        ],
    );
    let mutation: RepositoryMutationState =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(mutation.ordinal, 1);
    assert_eq!(
        fs::read_dir(&ledger_root)
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap()
            .len(),
        2
    );

    let recovered = runtime
        .recover_remote_action(
            &action_run,
            Some(RemoteGroundTruth {
                schema_version: p::M4_SCHEMA_VERSION,
                outcome: p::RemoteReceiptOutcome::Completed,
                result_digest: Some(mutation.value_digest.clone()),
                evidence: vec![p::EvidenceRef("ground-truth:mutation-ordinal:1".into())],
                verification: vec![p::EvidenceRef("verification:repository-state".into())],
            }),
        )
        .unwrap();
    assert!(recovered.terminal);
    let receipt = recovered.receipt.unwrap();

    let from = store.checkpoint(&replica.peer, &action_run).unwrap();
    let batch = store
        .export_replication(p::ReplicationExportRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: replica.peer.clone(),
            grant: replica.reference().unwrap(),
            aggregate: action_run.clone(),
            after: from.clone(),
            limit: 256,
            redaction: p::RedactionPolicyRef("redaction:m4-golden".into()),
        })
        .unwrap();
    let replica_db = root.join("replica.sqlite3");
    let replica_output =
        run_replicad_process(&replica_db, &replica, current.authority_epoch, &batch);
    assert!(
        replica_output.status.success(),
        "replicad stderr: {}",
        String::from_utf8_lossy(&replica_output.stderr)
    );
    let replica_report: p::ReplicaApplyReport =
        serde_json::from_slice(&replica_output.stdout).unwrap();
    assert_eq!(replica_report.status, p::ReplicaApplyStatus::Applied);
    assert_eq!(replica_report.cursor, batch.to);
    let ack = p::ReplicationAck {
        schema_version: p::M4_SCHEMA_VERSION,
        batch: batch.batch.clone(),
        peer: batch.peer.clone(),
        aggregate: batch.aggregate.clone(),
        applied: replica_report.cursor.clone(),
        projection_digest: replica_report.projection_digest.clone(),
    };
    let federation = p::FederationAggregateRef("federation".into());
    let expected = store.federation_version(&federation).unwrap();
    assert_eq!(
        runtime
            .acknowledge_replication(&replica.peer, &batch, ack, expected, now_ms())
            .unwrap()
            .status,
        p::ExpectedAppendStatus::Applied
    );

    let revoke_run = p::RunId("owner-control:s83-revoke-executor".into());
    let expected = store.federation_version(&federation).unwrap();
    FederationGatewayControl::revoke_federated_peer(
        harness.as_ref(),
        revoke_run.clone(),
        executor.peer.clone(),
        executor.reference().unwrap(),
        p::InFlightDisposition::KeepPinned,
        expected,
        p::VerifiedPrincipal(OWNER.into()),
    )
    .unwrap();
    let revoked_snapshot =
        FederationGatewayControl::federation_snapshot(harness.as_ref(), p::Scope(SCOPE.into()))
            .unwrap();
    let second_action = intent(
        placement(&executor, revoked_snapshot.authority_epoch),
        now_ms(),
    );
    let second_run = p::RunId("run:s83-after-revoke".into());
    assert!(FederationActionGateway::submit_federated_remote_action(
        harness.as_ref(),
        second_run.clone(),
        request(),
        second_action.clone(),
    )
    .is_err());
    assert!(event_kinds(&store, &second_run).is_empty());
    let final_mutation: RepositoryMutationState =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(final_mutation.ordinal, 1);

    let all_events = authority_events(
        &store,
        &[
            register_executor,
            register_owner,
            register_replica,
            action_run.clone(),
            revoke_run,
        ],
    );
    let ids = |kinds: &[p::EventKind]| {
        all_events
            .iter()
            .filter(|event| kinds.contains(&event.kind))
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>()
    };
    let trace = p::FederatedTraceManifest {
        schema_version: p::M4_SCHEMA_VERSION,
        run: action_run.clone(),
        owner_commands: ids(&[p::EventKind::FederatedPeerRegistered]),
        approvals: ids(&[
            p::EventKind::ApprovalRequested,
            p::EventKind::ApprovalResolved,
        ]),
        leases: ids(&[p::EventKind::RemoteExecutionLeaseChanged]),
        actions: ids(&[
            p::EventKind::ActionStarted,
            p::EventKind::ActionOutcomeUnknown,
            p::EventKind::ActionCompleted,
        ]),
        verifications: ids(&[
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
        ]),
        replication: ids(&[p::EventKind::ReplicationCheckpointAdvanced]),
        recovery: ids(&[p::EventKind::RunResumed]),
        revocations: ids(&[p::EventKind::FederatedPeerRevoked]),
    };
    let trace_digest = p::canonical_digest(&trace).unwrap();
    let artifact_bundle = FederationArtifactBundle {
        peer: p::FederatedPeerManifest {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: executor.peer.clone(),
            roles: executor.roles.clone(),
            scopes: executor.scopes.clone(),
            transport_identity: executor.transport_identity.clone(),
            authority_epoch: current.authority_epoch,
            grant_version: executor.grant_version,
            expires_at: executor.expires_at,
            grant_ref: executor.reference().unwrap(),
        },
        receipt,
        replication: p::ReplicationManifest {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: batch.peer.clone(),
            aggregate: batch.aggregate.clone(),
            from: batch.from.clone(),
            to: batch.to.clone(),
            batch: batch.batch.clone(),
            batch_digest: batch.content_digest.clone(),
            event_digests: batch
                .events
                .iter()
                .map(p::canonical_digest)
                .collect::<p::Result<Vec<_>>>()
                .unwrap(),
            redaction: batch.redaction.clone(),
            authority_epoch: batch.to.authority_epoch,
        },
        trace,
        report: p::FederationGoldenReport {
            schema_version: p::M4_SCHEMA_VERSION,
            scenario: "S83 three-process governed federation".into(),
            event_kinds: all_events.iter().map(|event| event.kind).collect(),
            authority_driver_calls: 1,
            mutation_ordinal: final_mutation.ordinal,
            replica_cursor: Some(replica_report.cursor),
            secret_scan_matches: 0,
            negative_assertions: vec![
                "approval preceded the only remote dispatch".into(),
                "unknown outcome recovered only from the original receipt".into(),
                "revoked executor produced no second mutation".into(),
                "replica received only scoped redacted envelopes".into(),
            ],
            trace_digest,
        },
    };
    let artifact_store = FederationArtifactStore::new(&artifact_root).unwrap();
    let artifact_receipt = artifact_store.write(&artifact_bundle).unwrap();
    assert_eq!(
        artifact_store.verify_complete_set().unwrap().digest,
        artifact_receipt.digest
    );
    println!(
        "M4_FEDERATION_ARTIFACT_DIGEST={}",
        artifact_receipt.digest.0
    );

    drop(executor_process);
    fs::remove_dir_all(root).unwrap();
}
