use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use forme_execution::{
    transport_identity_digest, ExecutorAdmissionProfile, ExecutorCredentialBinding,
    FileExecutorLedger, GuardedRemoteExecutor, InMemorySecretResolver, RemoteClock,
    RemoteExecutorDriver, RemoteInnerDriver, RemoteInnerOutcome, RemoteReceiptSource,
    RemoteTransport, RepositoryMutationDriver, ResolvedSecret, ScopedExecutorCredentialResolver,
    TlsIdentityFiles, TlsRemoteClientConfig, TlsRemoteExecutorServer, TlsRemoteServerConfig,
    TlsRemoteTransport,
};
use forme_protocol as p;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair, KeyUsagePurpose,
};

struct FixedClock(i64);

impl RemoteClock for FixedClock {
    fn now_ms(&self) -> p::Timestamp {
        self.0
    }
}

struct TestIdentity {
    certificate: Vec<u8>,
    key: Vec<u8>,
}

struct TestPki {
    ca: Vec<u8>,
    authority: TestIdentity,
    executor: TestIdentity,
}

fn pki() -> TestPki {
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
    TestPki {
        ca: ca.der().to_vec(),
        authority: leaf(
            "authority.local",
            ExtendedKeyUsagePurpose::ClientAuth,
            &issuer,
        ),
        executor: leaf("localhost", ExtendedKeyUsagePurpose::ServerAuth, &issuer),
    }
}

fn leaf(name: &str, usage: ExtendedKeyUsagePurpose, issuer: &Issuer<'_, KeyPair>) -> TestIdentity {
    let mut params = CertificateParams::new(vec![name.to_owned()]).unwrap();
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![usage];
    let key = KeyPair::generate().unwrap();
    let certificate: Certificate = params.signed_by(&key, issuer).unwrap();
    TestIdentity {
        certificate: certificate.der().to_vec(),
        key: key.serialize_der(),
    }
}

fn unique_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "forme-m4-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn write_identity(
    root: &Path,
    prefix: &str,
    identity: &TestIdentity,
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

fn grant(executor_identity: p::TransportIdentityDigest) -> p::FederatedPeerGrant {
    p::FederatedPeerGrant {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: p::FederatedPeerRef("peer:executor".into()),
        owner: p::VerifiedPrincipal("owner:forme".into()),
        roles: vec![p::FederatedPeerRole::Executor],
        scopes: vec![p::Scope("workspace:alpha".into())],
        capabilities: vec![p::CapabilityRef("fixture.mutate".into())],
        transport_identity: executor_identity,
        authority_epoch: p::AuthorityEpoch(1),
        grant_version: p::PeerGrantVersion(1),
        expires_at: 100_000,
        created_by: p::OwnerControlRef("owner-control:executor".into()),
    }
}

fn placement(grant: &p::FederatedPeerGrant) -> p::RemotePlacementPlan {
    let mut operation = p::RemoteOperation {
        schema_version: p::M4_SCHEMA_VERSION,
        backend: p::BackendKind::File,
        parameters: p::ActionParameters::File {
            operation: p::FileOperation::Write,
            path: "golden/state".into(),
            content: Some(b"real mutation".to_vec()),
        },
        capability: p::CapabilityRef("fixture.mutate".into()),
        scope: p::Scope("workspace:alpha".into()),
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
        authority_epoch: grant.authority_epoch,
        executor_profile: p::ExecutorProfileRef("profile:fixture-v1".into()),
        operation,
        digest: p::SchemaDigest(String::new()),
    };
    placement.refresh_digest().unwrap();
    placement
}

fn lease(plan: &p::RemotePlacementPlan) -> p::RemoteExecutionLease {
    p::RemoteExecutionLease {
        schema_version: p::M4_SCHEMA_VERSION,
        lease: p::RemoteExecutionLeaseRef("lease:tls".into()),
        dispatch: p::RemoteDispatchId("dispatch:tls".into()),
        intent: p::ActionId("intent:tls".into()),
        plan_digest: p::PlanDigest("sha256:approved-plan".into()),
        placement: plan.reference().unwrap(),
        executor: plan.executor.clone(),
        peer_grant: plan.peer_grant.clone(),
        grant_version: plan.grant_version,
        authority_epoch: plan.authority_epoch,
        fence: p::FenceToken(1),
        expires_at: 10_000,
        state: p::RemoteLeaseState::Acquired,
    }
}

#[test]
fn s72_s74_real_mutual_tls_dispatch_is_durable_and_exactly_once_at_the_driver() {
    let root = unique_root("tls");
    fs::create_dir_all(&root).unwrap();
    let pki = pki();
    let executor_identity = transport_identity_digest(&pki.executor.certificate);
    let authority_identity = transport_identity_digest(&pki.authority.certificate);
    let authority_files = write_identity(&root, "authority", &pki.authority, &pki.ca);
    let executor_files = write_identity(&root, "executor", &pki.executor, &pki.ca);
    let grant = grant(executor_identity.clone());
    let state_path = root.join("mutation-state.json");
    let driver = Arc::new(RepositoryMutationDriver::new("golden/state", &state_path).unwrap());
    let inner: Arc<dyn RemoteInnerDriver> = driver.clone();
    let service = Arc::new(
        GuardedRemoteExecutor::new(
            ExecutorAdmissionProfile {
                schema_version: p::M4_SCHEMA_VERSION,
                peer: grant.peer.clone(),
                grant: grant.clone(),
                profile: p::ExecutorProfileRef("profile:fixture-v1".into()),
                authority_epoch: grant.authority_epoch,
                minimum_fence: p::FenceToken(1),
                enabled_backends: BTreeSet::from([p::BackendKind::File]),
            },
            inner,
            Arc::new(FixedClock(1_000)),
        )
        .unwrap()
        .with_ledger(Arc::new(
            FileExecutorLedger::open(root.join("dispatch-ledger")).unwrap(),
        )),
    );
    let server = Arc::new(
        TlsRemoteExecutorServer::bind_with_clock(
            TlsRemoteServerConfig {
                bind: "127.0.0.1:0".parse().unwrap(),
                authority: p::AuthorityRef("authority:local".into()),
                peer: grant.peer.clone(),
                expected_authority_identity: authority_identity,
                identity: executor_files,
                replay_root: root.join("wire-replay"),
                timeout: p::DurationMs(5_000),
                max_body_bytes: 1_048_576,
            },
            service,
            Arc::new(FixedClock(1_000)),
        )
        .unwrap(),
    );
    let address = server.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_thread = {
        let server = server.clone();
        let stop = stop.clone();
        std::thread::spawn(move || server.serve_until(&stop))
    };
    assert!(TlsRemoteTransport::with_clock(
        TlsRemoteClientConfig {
            endpoint: format!("http://localhost:{}", address.port()),
            authority: p::AuthorityRef("authority:local".into()),
            peer: grant.peer.clone(),
            expected_peer_identity: executor_identity.clone(),
            identity: authority_files.clone(),
            timeout: p::DurationMs(5_000),
            request_ttl: p::DurationMs(1_000),
            max_body_bytes: 1_048_576,
        },
        Arc::new(FixedClock(1_000)),
    )
    .is_err());
    assert!(TlsRemoteTransport::with_clock(
        TlsRemoteClientConfig {
            endpoint: format!("https://localhost:{}", address.port()),
            authority: p::AuthorityRef("authority:local".into()),
            peer: grant.peer.clone(),
            expected_peer_identity: p::TransportIdentityDigest(String::new()),
            identity: authority_files.clone(),
            timeout: p::DurationMs(5_000),
            request_ttl: p::DurationMs(1_000),
            max_body_bytes: 1_048_576,
        },
        Arc::new(FixedClock(1_000)),
    )
    .is_err());
    let transport = TlsRemoteTransport::with_clock(
        TlsRemoteClientConfig {
            endpoint: format!("https://localhost:{}", address.port()),
            authority: p::AuthorityRef("authority:local".into()),
            peer: grant.peer.clone(),
            expected_peer_identity: executor_identity,
            identity: authority_files.clone(),
            timeout: p::DurationMs(5_000),
            request_ttl: p::DurationMs(1_000),
            max_body_bytes: 1_048_576,
        },
        Arc::new(FixedClock(1_000)),
    )
    .unwrap();
    let plan = placement(&grant);
    let lease = lease(&plan);
    let acceptance = match transport.dispatch(&plan, &lease) {
        Ok(acceptance) => acceptance,
        Err(error) => {
            stop.store(true, Ordering::SeqCst);
            let server_error = server_thread.join().unwrap();
            panic!("client={error:?}; server={server_error:?}");
        }
    };
    let receipt = transport
        .receipt(p::RemoteReceiptRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            receipt: acceptance.receipt.clone(),
            lease: lease.lease.clone(),
            dispatch: lease.dispatch.clone(),
            authority_epoch: lease.authority_epoch,
            fence: lease.fence,
            nonce: p::Nonce("receipt:1".into()),
        })
        .unwrap();
    assert_eq!(receipt.outcome, p::RemoteReceiptOutcome::Completed);
    assert_eq!(driver.calls(), 1);
    assert_eq!(driver.state().unwrap().unwrap().ordinal, 1);

    let duplicate = transport.dispatch(&plan, &lease).unwrap();
    assert_eq!(duplicate.receipt, receipt.receipt);
    assert_eq!(driver.calls(), 1);
    assert_eq!(driver.state().unwrap().unwrap().ordinal, 1);
    let probe = transport
        .probe(p::RemoteProbeRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            lease: lease.lease.clone(),
            dispatch: lease.dispatch.clone(),
            authority_epoch: lease.authority_epoch,
            fence: lease.fence,
            nonce: p::Nonce("probe:1".into()),
        })
        .unwrap();
    assert_eq!(probe.outcome, p::RemoteProbeOutcome::Completed);
    assert_eq!(probe.receipt, Some(receipt.receipt));

    let wrong_identity = TlsRemoteTransport::with_clock(
        TlsRemoteClientConfig {
            endpoint: format!("https://localhost:{}", address.port()),
            authority: p::AuthorityRef("authority:local".into()),
            peer: grant.peer.clone(),
            expected_peer_identity: p::TransportIdentityDigest("sha256:not-the-executor".into()),
            identity: authority_files,
            timeout: p::DurationMs(5_000),
            request_ttl: p::DurationMs(1_000),
            max_body_bytes: 1_048_576,
        },
        Arc::new(FixedClock(1_000)),
    )
    .unwrap();
    assert!(wrong_identity.dispatch(&plan, &lease).is_err());
    assert_eq!(driver.calls(), 1);
    assert_eq!(driver.state().unwrap().unwrap().ordinal, 1);

    stop.store(true, Ordering::SeqCst);
    assert!(server_thread.join().unwrap().is_err());
    drop(server);
    fs::remove_dir_all(root).unwrap();
}

struct CredentialRecordingDriver {
    calls: AtomicUsize,
    expected: String,
}

impl RemoteInnerDriver for CredentialRecordingDriver {
    fn execute(
        &self,
        operation: &p::RemoteOperation,
        credential: Option<&ResolvedSecret>,
    ) -> p::Result<RemoteInnerOutcome> {
        operation.validate()?;
        if credential.map(ResolvedSecret::expose) != Some(self.expected.as_str()) {
            return Err(p::Error(
                "executor-local credential was not resolved at the driver boundary".into(),
            ));
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(RemoteInnerOutcome {
            outcome: p::RemoteReceiptOutcome::Completed,
            result_digest: Some(p::SchemaDigest("sha256:credential-operation".into())),
            observations: vec![p::EvidenceRef("credential-operation:observed".into())],
        })
    }
}

#[test]
fn s71_s73_stale_fence_and_unscoped_credentials_stop_before_the_inner_driver() {
    const SECRET: &str = "m4-s73-local-secret-2c81";

    let mut peer_grant = grant(p::TransportIdentityDigest("sha256:executor".into()));
    peer_grant.transport_identity = p::TransportIdentityDigest("sha256:executor".into());
    let mut plan = placement(&peer_grant);
    let slot = p::ExecutorCredentialSlotRef("credential-slot:fixture".into());
    plan.operation.credential_slot = Some(slot.clone());
    plan.operation.refresh_digest().unwrap();
    plan.refresh_digest().unwrap();
    let mut current_lease = lease(&plan);
    current_lease.fence = p::FenceToken(2);

    let secrets = Arc::new(InMemorySecretResolver::default());
    let secret_ref = p::SecretRef("executor-local:fixture-secret".into());
    secrets.insert(secret_ref.clone(), SECRET).unwrap();
    let credentials = Arc::new(
        ScopedExecutorCredentialResolver::new(
            vec![ExecutorCredentialBinding {
                peer: peer_grant.peer.clone(),
                scope: plan.operation.scope.clone(),
                slot,
                secret: secret_ref,
            }],
            secrets,
        )
        .unwrap(),
    );
    let driver = Arc::new(CredentialRecordingDriver {
        calls: AtomicUsize::new(0),
        expected: SECRET.into(),
    });
    let service = GuardedRemoteExecutor::new(
        ExecutorAdmissionProfile {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: peer_grant.peer.clone(),
            grant: peer_grant,
            profile: plan.executor_profile.clone(),
            authority_epoch: plan.authority_epoch,
            minimum_fence: p::FenceToken(2),
            enabled_backends: BTreeSet::from([p::BackendKind::File]),
        },
        driver.clone(),
        Arc::new(FixedClock(1_000)),
    )
    .unwrap()
    .with_credentials(credentials);

    let mut stale_lease = current_lease.clone();
    stale_lease.fence = p::FenceToken(1);
    assert!(service.admit(&plan, &stale_lease).is_err());
    assert_eq!(driver.calls.load(Ordering::SeqCst), 0);

    let admission = service.admit(&plan, &current_lease).unwrap();
    let receipt = service.execute(admission).unwrap();
    assert_eq!(driver.calls.load(Ordering::SeqCst), 1);
    let serialized = serde_json::to_string(&receipt).unwrap();
    assert!(!serialized.contains(SECRET));
    assert!(!serialized.contains("executor-local:fixture-secret"));
    assert!(!serialized.contains("credential-slot:fixture"));
}
