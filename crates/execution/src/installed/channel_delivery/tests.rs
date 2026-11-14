//! Component evidence: genuine local recipient and signatures; native tests are separate.
use super::super::product_config::tests as config_tests;
use super::*;
use ed25519_dalek::{Signer, SigningKey};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};
const SECRET: &str = "channel-delivery-test-secret-32-bytes";
const BODY: &[u8] = b"bounded original message";
struct Fixture {
    root: tempfile::TempDir,
    config: Arc<Mutex<ProductConfigStore>>,
    channels: Arc<InstalledChannelDelivery>,
    source: p::ProductChannelSource,
    listener: TcpListener,
    key: SigningKey,
}
impl Fixture {
    fn new() -> Self {
        let (root, _path, config) = config_tests::fixture();
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let key = SigningKey::from_bytes(&[23; 32]);
        let source = source(
            format!("http://{}/deliver", listener.local_addr().unwrap()),
            &key,
        );
        config_tests::provision_channel_fixture(
            &mut config.lock().unwrap(),
            root.path(),
            source.clone(),
        );
        let channels =
            InstalledChannelDelivery::new(config.clone(), &root.path().join("channel-delivery"))
                .unwrap();
        fs::create_dir_all(&channels.credentials).unwrap();
        let reference = &source.delivery.as_ref().unwrap().credential_ref;
        let name = p::sha256_content_digest(reference.as_bytes());
        fs::write(
            channels.credentials.join(format!("{}.token", &name.0[7..])),
            SECRET,
        )
        .unwrap();
        let patch = settings(&source, "channel:enable", 1, true);
        assert!(config
            .lock()
            .unwrap()
            .apply_channel_settings(patch.clone())
            .is_err());
        config.lock().unwrap().bind_channel_consumer(&channels);
        config
            .lock()
            .unwrap()
            .apply_channel_settings(patch)
            .unwrap();
        Self {
            root,
            config,
            channels,
            source,
            listener,
            key,
        }
    }
    fn input(&self, id: &str) -> p::ManagedChannelDeliveryInputV1 {
        input(&self.source, id)
    }
    fn admission(&self, input: p::ManagedChannelDeliveryInputV1) -> Admission {
        self.channels
            .with_admission(
                input,
                "installation:channel-test",
                now_ms(),
                Instant::now(),
                Ok,
            )
            .unwrap()
    }
    fn recipient(
        &self,
        grant: p::ManagedChannelDeliveryGrantV1,
        mode: &str,
    ) -> thread::JoinHandle<usize> {
        let listener = self.listener.try_clone().unwrap();
        let key = self.key.clone();
        let mode = mode.to_owned();
        thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < until);
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let end = loop {
                let mut b = [0; 1];
                stream.read_exact(&mut b).unwrap();
                request.push(b[0]);
                assert!(request.len() < 8192);
                if request.ends_with(b"\r\n\r\n") {
                    break request.len();
                }
            };
            let headers = std::str::from_utf8(&request).unwrap();
            assert!(headers.starts_with("POST /deliver HTTP/1.1\r\n"));
            assert!(headers.contains(&format!("Authorization: Bearer {SECRET}\r\n")));
            assert!(headers.contains(&grant.grant_ref));
            request.resize(end + BODY.len(), 0);
            stream.read_exact(&mut request[end..]).unwrap();
            assert_eq!(&request[end..], BODY);
            if mode == "disconnect" {
                return 1;
            }
            let mut receipt = signed(&grant, &key);
            if mode == "wrong-signature" {
                receipt.signature[0] ^= 1;
            }
            let body = serde_json::to_vec(&receipt).unwrap();
            let status = if mode == "redirect" {
                "302 Found\r\nLocation: http://127.0.0.1:9/other"
            } else {
                "200 OK"
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .unwrap();
            let _ = stream.write_all(&body);
            1
        })
    }
    fn no_connection(&self) {
        assert_eq!(
            self.listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
fn settings(
    source: &p::ProductChannelSource,
    operation: &str,
    revision: u64,
    enabled: bool,
) -> p::ProductChannelSettingsPatch {
    p::ProductChannelSettingsPatch {
        schema_version: p::SchemaVersion(1),
        operation_id: operation.into(),
        expected_revision: revision,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        channels: vec![p::ProductChannelConfig {
            schema_version: p::SchemaVersion(1),
            channel_ref: source.channel_ref.clone(),
            connector_ref: source.connector_ref.clone(),
            scope: source.scope.clone(),
            adapter_ref: p::ProductChannelAdapter::Loopback,
            enabled,
            participant_binding: source.participant_binding.clone(),
            credential_ref: Some(source.credential_ref.clone()),
        }],
    }
}
fn source(endpoint: String, key: &SigningKey) -> p::ProductChannelSource {
    p::ProductChannelSource {
        schema_version: p::SchemaVersion(2),
        channel_ref: "channel:local".into(),
        connector_ref: "connector:local".into(),
        scope: p::Scope("workspace:test".into()),
        participant_binding: "mapping:recipient".into(),
        participant: p::ParticipantId("participant:recipient".into()),
        credential_ref: "credential:ingress".into(),
        credential_digest: p::sha256_content_digest(b"different-ingress-test-token-32-bytes"),
        delivery: Some(p::ProductChannelDeliverySource {
            schema_version: p::SchemaVersion(1),
            endpoint,
            schema_digest: p::sha256_content_digest(b"local recipient schema"),
            credential_ref: "credential:egress".into(),
            credential_digest: p::sha256_content_digest(SECRET.as_bytes()),
            receipt_public_key: key.verifying_key().to_bytes(),
            capability_ref: p::CapabilityRef("capability:local-channel".into()),
            permission: p::PermissionRef("permission:local-channel".into()),
            max_input_bytes: 65_536,
            max_output_bytes: 8192,
            timeout: p::DurationMs(5000),
        }),
    }
}
fn input(source: &p::ProductChannelSource, id: &str) -> p::ManagedChannelDeliveryInputV1 {
    use crate::{DefaultExecutionPlanner, ExecutionPlanner, OutputBudget};
    let d = source.delivery.as_ref().unwrap();
    let now = now_ms();
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId(format!("action:{id}")),
        source: p::Source::Communication,
        goal: p::GoalRef("goal:delivery".into()),
        backend_hint: p::BackendKind::AppApi,
        capability_ref: d.capability_ref.clone(),
        action_type: p::ActionType::Deliver,
        scope: source.scope.clone(),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("external-message-not-retractable".into()),
        parameters: p::ActionParameters::AppApi(p::AppApiActionSpec {
            schema_version: p::SchemaVersion(1),
            connector: p::ProviderId(source.connector_ref.clone()),
            endpoint: d.endpoint.clone(),
            schema_digest: d.schema_digest.clone(),
            credential: Some(p::SecretRef(d.credential_ref.clone())),
            operation: p::AppApiOperation::Mutation {
                method: p::AppApiMutationMethod::Post,
                body: Some(p::ExternalInput::Content(p::ContentRef(
                    "content:message".into(),
                ))),
                idempotency_key: p::IdempotencyKey(format!(
                    "communication:{id}:{}",
                    p::sha256_content_digest(BODY).0
                )),
            },
            timeout: d.timeout,
            participant: Some(source.participant.clone()),
            representation: Some(p::Representation::Agent),
            disclosure_request: Some(p::DisclosureRequestRef("disclosure:message".into())),
        }),
        requested_permissions: vec![d.permission.clone()],
        requested_at: now,
        estimated_output_bytes: d.max_output_bytes,
        estimated_duration: d.timeout,
    };
    let plan = DefaultExecutionPlanner::new(OutputBudget::strict(d.max_output_bytes), d.timeout)
        .plan(&intent)
        .unwrap();
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: format!("job:{id}"),
        owner_ref: p::VerifiedPrincipal("owner:test".into()),
        action_ref: intent.intent_id.clone(),
        affected_resources: plan.affected_resources.clone(),
        execution_plan_ref: format!("plan:{id}"),
        plan_digest: plan.digest.clone(),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: format!("permit:{id}"),
        sandbox_profile_ref: p::MANAGED_CHANNEL_PROFILE.into(),
        workspace_scope: source.scope.clone(),
        deadline: now + 5000,
        nonce: p::Nonce(format!("nonce:{id}")),
        cancel_ref: format!("cancel:{id}"),
    };
    let permit = p::WorkerPermit {
        schema_version: p::SchemaVersion(1),
        permit_ref: job.permit_ref.clone(),
        job_id: job.job_id.clone(),
        owner_ref: job.owner_ref.clone(),
        action_ref: job.action_ref.clone(),
        affected_resources: job.affected_resources.clone(),
        plan_digest: job.plan_digest.clone(),
        authority_epoch: job.authority_epoch,
        workspace_scope: job.workspace_scope.clone(),
        sandbox_profile_ref: job.sandbox_profile_ref.clone(),
        deadline: job.deadline,
        nonce: job.nonce.clone(),
        issued_at: now,
    };
    let request = p::WorkerDispatchRequest {
        schema_version: p::SchemaVersion(1),
        job,
        permit,
        execution: Some(p::WorkerExecutionSpec {
            schema_version: p::SchemaVersion(1),
            plan_digest: plan.digest,
            intent,
            backend: plan.backend,
            scope: plan.scope,
            affected_resources: plan.affected_resources,
            normalized_action_digest: plan.normalized_action_digest,
            timeout: plan.timeout,
            output_budget_bytes: plan.budget.max_bytes,
            desktop_session: None,
            input_grants: Vec::new(),
        }),
    };
    let grant = p::ManagedChannelDeliveryGrantV1::from_request(
        "installation:channel-test",
        source,
        &request,
        BODY,
        now,
    )
    .unwrap();
    p::ManagedChannelDeliveryInputV1 {
        schema_version: p::SchemaVersion(1),
        execution: request,
        grant_ref: grant.grant_ref,
        content_bytes: BODY.to_vec(),
    }
}
fn signed(
    grant: &p::ManagedChannelDeliveryGrantV1,
    key: &SigningKey,
) -> p::ManagedChannelDeliveryReceiptV1 {
    let mut receipt = p::ManagedChannelDeliveryReceiptV1 {
        schema_version: p::SchemaVersion(1),
        grant_ref: grant.grant_ref.clone(),
        job_id: grant.job_id.clone(),
        nonce: grant.nonce.clone(),
        plan_digest: grant.plan_digest.clone(),
        content_digest: grant.content_digest.clone(),
        participant: grant.participant.clone(),
        accepted: true,
        signature: vec![],
    };
    receipt.signature = key
        .sign(&receipt.signing_bytes().unwrap())
        .to_bytes()
        .to_vec();
    receipt
}
#[test]
fn exact_grant_rejects_owner_body_source_plan_recipient_expiry_and_unknown_fields_before_connect() {
    let f = Fixture::new();
    let input = f.input("validation");
    for case in 0..8 {
        let mut bad = input.clone();
        match case {
            0 => bad.content_bytes[0] ^= 1,
            1 => bad.grant_ref.push('0'),
            2 => bad.execution.job.owner_ref.0 = "owner:other".into(),
            3 => bad.execution.job.plan_digest.0 = p::sha256_content_digest(b"other").0,
            4 => bad.execution.job.deadline = now_ms() - 1,
            5 => bad.schema_version = p::SchemaVersion(2),
            6 => bad.execution.job.nonce.0 = "nonce:other".into(),
            _ => {
                if let p::ActionParameters::AppApi(spec) =
                    &mut bad.execution.execution.as_mut().unwrap().intent.parameters
                {
                    spec.participant = Some(p::ParticipantId("participant:other".into()));
                }
            }
        }
        assert!(
            f.channels
                .with_admission(
                    bad,
                    "installation:channel-test",
                    now_ms(),
                    Instant::now(),
                    |_| Ok(())
                )
                .is_err(),
            "case {case}"
        );
        f.no_connection();
    }
    let mut wire = serde_json::to_value(input).unwrap();
    wire["credential"] = "injected".into();
    assert!(serde_json::from_value::<p::ManagedChannelDeliveryInputV1>(wire).is_err());
    assert_eq!(fs::read_dir(&f.channels.intents).unwrap().count(), 0);
}
#[test]
fn exact_endpoint_rejects_dns_redirect_targets_proxy_forms_and_encoded_paths() {
    for endpoint in [
        "http://localhost:10/send",
        "https://127.0.0.1:10/send",
        "http://127.0.0.2:10/send",
        "http://127.0.0.1/send",
        "http://127.0.0.1:0/send",
        "http://user@127.0.0.1:10/send",
        "http://127.0.0.1:10/send?q=1",
        "http://127.0.0.1:10/send#x",
        "http://127.0.0.1:10/a%2fb",
        "http://127.0.0.1:10/../send",
        "http://127.0.0.1:10/a\\b",
    ] {
        assert!(
            p::channel_delivery_endpoint(endpoint).is_err(),
            "{endpoint}"
        );
    }
    assert!(p::channel_delivery_endpoint("http://[::1]:10/send").is_ok());
}
#[test]
fn signed_recipient_receipt_is_required_and_consumed_nonce_survives_reopen() {
    let f = Fixture::new();
    let admission = f.admission(f.input("once"));
    let server = f.recipient(admission.grant.clone(), "ok");
    let attempted = AtomicBool::new(false);
    let bytes = f
        .channels
        .deliver(&admission, &crate::CancelToken::default(), &attempted)
        .unwrap();
    assert!(attempted.load(Ordering::SeqCst));
    proxy::verify_receipt(&admission.grant, &f.source, &bytes).unwrap();
    assert_eq!(server.join().unwrap(), 1);
    assert!(f
        .channels
        .deliver(
            &admission,
            &crate::CancelToken::default(),
            &AtomicBool::new(false)
        )
        .is_err());
    let reopened =
        InstalledChannelDelivery::new(f.config.clone(), &f.root.path().join("channel-delivery"))
            .unwrap();
    assert!(reopened
        .deliver(
            &admission,
            &crate::CancelToken::default(),
            &AtomicBool::new(false)
        )
        .is_err());
    f.no_connection();
    let intent = fs::read_dir(&f.channels.intents)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = fs::read_to_string(intent).unwrap();
    assert!(!text.contains(SECRET));
    assert!(!text.contains(std::str::from_utf8(BODY).unwrap()));
}
#[test]
fn disconnect_wrong_signature_and_redirect_are_unknown_attempts_without_second_connection() {
    for mode in ["disconnect", "wrong-signature", "redirect"] {
        let f = Fixture::new();
        let admission = f.admission(f.input(mode));
        let server = f.recipient(admission.grant.clone(), mode);
        let attempted = AtomicBool::new(false);
        assert!(f
            .channels
            .deliver(&admission, &crate::CancelToken::default(), &attempted)
            .is_err());
        assert!(attempted.load(Ordering::SeqCst));
        assert_eq!(server.join().unwrap(), 1);
        assert!(f
            .channels
            .deliver(
                &admission,
                &crate::CancelToken::default(),
                &AtomicBool::new(false)
            )
            .is_err());
        f.no_connection();
    }
}
#[test]
fn revoked_source_and_cancelled_job_cannot_connect_or_reserve_send_intent() {
    let f = Fixture::new();
    let admission = f.admission(f.input("cancel"));
    let cancel = crate::CancelToken::default();
    cancel.cancel();
    assert!(f
        .channels
        .deliver(&admission, &cancel, &AtomicBool::new(false))
        .is_err());
    f.no_connection();
    f.config
        .lock()
        .unwrap()
        .apply_channel_settings(settings(&f.source, "channel:disable", 2, false))
        .unwrap();
    assert!(f
        .channels
        .deliver(
            &admission,
            &crate::CancelToken::default(),
            &AtomicBool::new(false)
        )
        .is_err());
    f.no_connection();
    assert_eq!(fs::read_dir(&f.channels.intents).unwrap().count(), 0);
}
#[test]
fn receipt_signature_cannot_be_reused_for_a_different_plan_or_participant() {
    let f = Fixture::new();
    let admission = f.admission(f.input("receipt"));
    let original = signed(&admission.grant, &f.key);
    for case in 0..4 {
        let mut receipt = original.clone();
        match case {
            0 => receipt.participant.0 = "participant:other".into(),
            1 => receipt.plan_digest.0 = p::sha256_content_digest(b"other").0,
            2 => receipt.accepted = false,
            _ => receipt.schema_version = p::SchemaVersion(2),
        }
        assert!(proxy::verify_receipt(
            &admission.grant,
            &f.source,
            &serde_json::to_vec(&receipt).unwrap()
        )
        .is_err());
    }
    f.no_connection();
}
#[cfg(windows)]
mod native;
