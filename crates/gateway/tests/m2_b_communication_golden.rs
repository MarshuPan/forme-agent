use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forme_communication::{
    AuthContext, CommunicationBudget, CommunicationPurpose, DisclosureOutcome, DisclosurePolicy,
    DisclosureRule, ExternalCommunicationGrant, InMemoryCommunicationFabric,
    LoopbackChannelAdapter, OutboundMessage, ParticipantProfile, RepresentationRule,
    TerminationPolicy, TranscriptPolicy,
};
use forme_gateway::{Gateway, LocalGateway};
use forme_harness::{ProjectAppApiRuntimeConfig, ReactiveHarness};
use forme_protocol as p;

struct LoopbackParticipant {
    address: SocketAddr,
    connections: Arc<AtomicUsize>,
    deliveries: Arc<AtomicUsize>,
    representation: Arc<Mutex<Option<String>>>,
    participant: Arc<Mutex<Option<String>>>,
    body: Arc<Mutex<Vec<u8>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl LoopbackParticipant {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let connection_count = connections.clone();
        let deliveries = Arc::new(AtomicUsize::new(0));
        let delivery_count = deliveries.clone();
        let representation = Arc::new(Mutex::new(None));
        let captured_representation = representation.clone();
        let participant = Arc::new(Mutex::new(None));
        let captured_participant = participant.clone();
        let body = Arc::new(Mutex::new(Vec::new()));
        let captured_body = body.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let thread = thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if stop.load(Ordering::SeqCst) {
                            break;
                        }
                        connection_count.fetch_add(1, Ordering::SeqCst);
                        if let Some((headers, request_body)) = read_request(&mut stream) {
                            delivery_count.fetch_add(1, Ordering::SeqCst);
                            *captured_representation.lock().unwrap() =
                                header(&headers, "x-forme-representation");
                            *captured_participant.lock().unwrap() =
                                header(&headers, "x-forme-participant");
                            *captured_body.lock().unwrap() = request_body;
                            let _ = stream.write_all(
                                b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            );
                            let _ = stream.flush();
                            let _ = stream.shutdown(Shutdown::Write);
                        }
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
            connections,
            deliveries,
            representation,
            participant,
            body,
            stopped,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}/api/", self.address)
    }

    fn endpoint(&self) -> String {
        format!("{}messages", self.base_url())
    }
}

impl Drop for LoopbackParticipant {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 2_048];
    let header_end = loop {
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let headers = String::from_utf8(bytes[..header_end].to_vec()).ok()?;
    let content_length = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Some((
        headers,
        bytes[header_end..bytes.len().min(header_end + content_length)].to_vec(),
    ))
}

fn header(headers: &str, expected: &str) -> Option<String> {
    headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case(expected))
        .map(|(_, value)| value.trim().to_owned())
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn envelope(now: p::Timestamp) -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("communication:project".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef("connector:project:deliver".into())],
            permissions: vec![p::PermissionRef("connector:project:use".into())],
        },
        action_type: vec![p::ActionType::Deliver],
        risk_limit: p::Risk::High,
        approval_rule: p::ApprovalRule::Ask,
        budget: p::Budget("units:2".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: now.saturating_sub(1_000),
            expires_at: now.saturating_add(60_000),
            max_turns: 2,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}

fn grant(now: p::Timestamp, envelope: p::AutonomyEnvelope) -> ExternalCommunicationGrant {
    ExternalCommunicationGrant {
        schema_version: p::SchemaVersion(1),
        grant_ref: p::GrantRef("external-grant:project".into()),
        participant: ParticipantProfile::External(p::ParticipantId("external:person-a".into())),
        purpose: CommunicationPurpose {
            schema_version: p::SchemaVersion(1),
            id: p::PurposeRef("purpose:one-project-response".into()),
            description: "Send one scoped project response".into(),
        },
        modality_set: vec![p::Modality::Text],
        disclosure: DisclosurePolicy {
            schema_version: p::SchemaVersion(1),
            id: p::DisclosurePolicyRef("disclosure:project".into()),
            allow: vec![DisclosureRule {
                schema_version: p::SchemaVersion(1),
                category: "public".into(),
                outcome: DisclosureOutcome::Answer,
                high_impact_requires_approval: true,
            }],
            representation: RepresentationRule::AgentOnly,
        },
        termination: TerminationPolicy {
            schema_version: p::SchemaVersion(1),
            max_rounds: 2,
            terminate_on_purpose_drift: true,
            terminate_on_risk_escalation: true,
        },
        autonomy_envelope: envelope,
        ttl: p::DurationMs(30_000),
        budget: CommunicationBudget {
            schema_version: p::SchemaVersion(1),
            max_messages: 1,
            max_tokens: 128,
        },
        transcript_policy: TranscriptPolicy {
            schema_version: p::SchemaVersion(1),
            id: p::TranscriptPolicyRef("transcript:candidate-only".into()),
            retain: true,
            candidate_only: true,
        },
        issued_at: now,
    }
}

fn message(session: p::CommunicationSessionId, id: &str, now: p::Timestamp) -> OutboundMessage {
    OutboundMessage {
        schema_version: p::SchemaVersion(1),
        message_id: id.into(),
        session,
        request_ref: p::DisclosureRequestRef(format!("disclosure:{id}")),
        category: "public".into(),
        content_ref: p::ContentRef("content:project-message".into()),
        sensitive: false,
        confirmed: true,
        high_impact: false,
        tokens: 8,
        purpose_aligned: true,
        risk_escalated: false,
        requested_at: now,
        timeout: p::DurationMs(2_000),
        estimated_output_bytes: 4_096,
    }
}

#[test]
fn s44_s45_real_loopback_delivery_stays_inside_disclosure_and_harness_governance() {
    let participant = LoopbackParticipant::start();
    let now = now_ms();
    let envelope = envelope(now);
    let harness = Arc::new(
        ReactiveHarness::project_owned_app_api(ProjectAppApiRuntimeConfig {
            schema_version: p::SchemaVersion(1),
            connector: p::ProviderId("connector:project".into()),
            base_url: participant.base_url(),
            schema_digest: p::SchemaDigest("sha256:project-v1".into()),
            credential_ref: None,
            scope: p::Scope("communication:project".into()),
            capability: p::CapabilityRef("connector:project:deliver".into()),
            permission: p::PermissionRef("connector:project:use".into()),
            allowed_mutations: vec![p::AppApiMutationMethod::Post],
            requests_per_minute: 4,
            timeout: p::DurationMs(2_000),
            max_response_bytes: 64 * 1024,
            envelope: envelope.clone(),
            contents: vec![(
                p::ContentRef("content:project-message".into()),
                b"approved project response".to_vec(),
            )],
        })
        .unwrap(),
    );
    let owner = p::VerifiedPrincipal("owner:local".into());
    let gateway = LocalGateway::new(
        harness,
        owner.clone(),
        p::AgentProfileRef("agent:forme".into()),
    )
    .unwrap();
    let auth = AuthContext::local_owner(owner.clone(), now.saturating_add(60_000));
    let fabric = InMemoryCommunicationFabric::with_clock(move || now);
    let session = gateway
        .open_external_session(&fabric, grant(now, envelope), auth.clone())
        .unwrap();
    let adapter = LoopbackChannelAdapter::new(
        p::ProviderId("channel:project".into()),
        ParticipantProfile::External(p::ParticipantId("external:person-a".into())),
        p::Scope("communication:project".into()),
        p::ProviderId("connector:project".into()),
        participant.endpoint(),
        p::SchemaDigest("sha256:project-v1".into()),
        None,
        p::CapabilityRef("connector:project:deliver".into()),
        p::PermissionRef("connector:project:use".into()),
    )
    .unwrap();

    let run = gateway
        .deliver_external_message(
            &fabric,
            &adapter,
            message(session.clone(), "message-1", now),
            auth.clone(),
        )
        .unwrap();
    assert_eq!(participant.deliveries.load(Ordering::SeqCst), 0);
    let pending = gateway
        .pending_approvals(p::SessionId(session.0.clone()), auth.clone())
        .unwrap();
    assert_eq!(pending.len(), 1);
    let approval = &pending[0];
    gateway
        .control_run(
            run.clone(),
            p::RunControl::ResolveApproval(p::ApprovalDecision {
                schema_version: p::SchemaVersion(1),
                approval_id: approval.approval_id.clone(),
                outcome: p::ApprovalOutcome::Granted,
                approver: owner,
                bound_plan_digest: approval.plan_digest.clone(),
                policy_version: approval.policy_version,
                tool_schema_version: approval.tool_schema_version,
                nonce: p::Nonce("nonce:m2-b-real-delivery".into()),
                use_by: approval.expires_at.saturating_sub(1),
            }),
            auth.clone(),
        )
        .unwrap();
    let result = gateway.wait(run.clone()).unwrap_or_else(|error| {
        let kinds = Gateway::stream_events(&gateway, run.clone())
            .map(|event| event.kind)
            .collect::<Vec<_>>();
        panic!(
            "real delivery did not reach a terminal result: {error}; connections={}; deliveries={}; events={kinds:?}",
            participant.connections.load(Ordering::SeqCst),
            participant.deliveries.load(Ordering::SeqCst)
        )
    });
    assert_eq!(result.status, p::RunStatus::Complete);
    assert_eq!(participant.deliveries.load(Ordering::SeqCst), 1);
    assert_eq!(
        participant.representation.lock().unwrap().as_deref(),
        Some("agent")
    );
    assert_eq!(
        participant.participant.lock().unwrap().as_deref(),
        Some("external:person-a")
    );
    assert_eq!(
        &*participant.body.lock().unwrap(),
        b"approved project response"
    );

    let events = Gateway::stream_events(&gateway, run).collect::<Vec<_>>();
    let kinds = events.iter().map(|event| event.kind).collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            p::EventKind::DisclosurePolicyApplied,
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::ToolCallProposed,
            p::EventKind::ToolPolicyEvaluated,
            p::EventKind::ApprovalRequested,
            p::EventKind::RunWaiting,
            p::EventKind::ApprovalResolved,
            p::EventKind::RunResumed,
            p::EventKind::ToolPolicyEvaluated,
            p::EventKind::CompetenceGateEvaluated,
            p::EventKind::ActionPlanned,
            p::EventKind::ActionStarted,
            p::EventKind::ActionOutputDelta,
            p::EventKind::ActionCompleted,
            p::EventKind::CapabilityEvidenceRecorded,
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
            p::EventKind::RunComplete,
        ]
    );
    let disclosure = kinds
        .iter()
        .position(|kind| *kind == p::EventKind::DisclosurePolicyApplied)
        .unwrap();
    let planned = kinds
        .iter()
        .position(|kind| *kind == p::EventKind::ActionPlanned)
        .unwrap();
    let started = kinds
        .iter()
        .position(|kind| *kind == p::EventKind::ActionStarted)
        .unwrap();
    assert!(disclosure < planned && planned < started);
    assert!(events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                p::EventKind::ActionOutputDelta | p::EventKind::ActionCompleted
            )
        })
        .all(|event| event.provenance.trust_tier == p::TrustTier::Untrusted));
    assert!(!kinds.iter().any(|kind| {
        matches!(
            kind,
            p::EventKind::CandidatePromoted
                | p::EventKind::MemoryNodeAppended
                | p::EventKind::MemoryEdgeAppended
        )
    }));

    assert!(gateway
        .deliver_external_message(
            &fabric,
            &adapter,
            message(session.clone(), "message-2", now),
            auth,
        )
        .is_err());
    assert_eq!(participant.deliveries.load(Ordering::SeqCst), 1);
    let session_events =
        Gateway::stream_events(&gateway, p::RunId(format!("communication:{}", session.0)))
            .map(|event| event.kind)
            .collect::<Vec<_>>();
    assert_eq!(
        session_events,
        vec![
            p::EventKind::ExternalCommunicationGranted,
            p::EventKind::CommunicationSessionOpened,
            p::EventKind::DisclosurePolicyApplied,
            p::EventKind::CommunicationSessionTerminated,
        ]
    );
}
