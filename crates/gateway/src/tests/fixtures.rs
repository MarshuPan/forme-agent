//! Repository-owned shared gateway test double.
use super::*;

#[derive(Clone)]
pub(super) struct FakeRun {
    pub(super) events: Vec<p::Event>,
    pub(super) result: p::RunResult,
    pub(super) output: String,
}

type EcosystemDistributionCall = (
    p::CapabilityInstallPlan,
    p::FederatedPeerRef,
    p::AuthorityEpoch,
    p::VerifiedPrincipal,
    p::Timestamp,
);

pub(super) struct FakeHarness {
    pub(super) sequence: AtomicU64,
    pub(super) ingress_authority: IngressAuthority,
    pub(super) responses: Mutex<VecDeque<String>>,
    pub(super) runs: Mutex<BTreeMap<p::RunId, FakeRun>>,
    pub(super) requests: Mutex<Vec<p::RunRequest>>,
    pub(super) evolution_paused: AtomicBool,
    pub(super) federation_commands: Mutex<
        Vec<(
            p::FederatedControlEnvelope,
            p::FederatedOwnerCommand,
            p::Timestamp,
        )>,
    >,
    pub(super) federation_actions: Mutex<Vec<(p::RunId, p::RunRequest, p::ActionIntent)>>,
    pub(super) federation_acks: Mutex<Vec<(p::FederatedPeerRef, p::ReplicationAck)>>,
    pub(super) federation_retention_receipts:
        Mutex<Vec<(p::FederatedPeerRef, p::FederatedRetentionReceipt)>>,
    pub(super) federation_signals: Mutex<Vec<(p::FederatedPeerRef, p::FederatedDeviceSignal)>>,
    pub(super) ecosystem_distributions: Mutex<Vec<EcosystemDistributionCall>>,
}

impl FakeHarness {
    pub(super) fn new(responses: Vec<String>) -> Self {
        Self {
            sequence: AtomicU64::new(1),
            ingress_authority: IngressAuthority::default(),
            responses: Mutex::new(responses.into()),
            runs: Mutex::new(BTreeMap::new()),
            requests: Mutex::new(Vec::new()),
            evolution_paused: AtomicBool::new(false),
            federation_commands: Mutex::new(Vec::new()),
            federation_actions: Mutex::new(Vec::new()),
            federation_acks: Mutex::new(Vec::new()),
            federation_retention_receipts: Mutex::new(Vec::new()),
            federation_signals: Mutex::new(Vec::new()),
            ecosystem_distributions: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn federated_grant(peer: &p::FederatedPeerRef) -> Option<p::FederatedPeerGrant> {
        let (roles, identity, version) = match peer.0.as_str() {
            "peer:owner-device-b" => (
                vec![p::FederatedPeerRole::OwnerClient],
                "sha256:owner-device-b",
                1,
            ),
            "peer:replica-b" => (vec![p::FederatedPeerRole::Replica], "sha256:replica-b", 2),
            _ => return None,
        };
        Some(p::FederatedPeerGrant {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: peer.clone(),
            owner: p::VerifiedPrincipal("owner:test".into()),
            roles,
            scopes: vec![p::Scope("workspace:m4".into())],
            capabilities: Vec::new(),
            transport_identity: p::TransportIdentityDigest(identity.into()),
            authority_epoch: p::AuthorityEpoch(1),
            grant_version: p::PeerGrantVersion(version),
            expires_at: i64::MAX,
            created_by: p::OwnerControlRef(format!("owner-control:{}", peer.0)),
        })
    }

    pub(super) fn event(
        run: &p::RunId,
        sequence: u64,
        payload: p::EventPayload,
        provenance: p::Provenance,
    ) -> p::Event {
        let mut event = p::Event::new(
            p::EventId(format!("fake-event:{sequence}")),
            run.clone(),
            None,
            payload,
            p::SchemaVersion(1),
            now_ms(),
            provenance,
        );
        event.stream_seq = sequence;
        event
    }
}

pub(super) fn m5_gateway_package() -> p::SignedCapabilityPackage {
    let content = "gateway-owned declarative skill".to_owned();
    let payload_digest = p::sha256_content_digest(content.as_bytes());
    let mut package = p::SignedCapabilityPackage {
        schema_version: p::M5_SCHEMA_VERSION,
        manifest: p::CapabilityPackageManifest {
            schema_version: p::M5_SCHEMA_VERSION,
            package: p::CapabilityPackageRef("package:m5-gateway".into()),
            release: p::CapabilityReleaseRef("release:m5-gateway:v1".into()),
            version: p::Version(1),
            kind: p::CapabilityPackageKind::Skill,
            publisher: p::CapabilityPublisherRef("publisher:m5-gateway".into()),
            scope: p::Scope("workspace:m5-gateway".into()),
            contributions: vec![p::CapabilityContributionDescriptor {
                schema_version: p::M5_SCHEMA_VERSION,
                kind: p::CapabilityPackageKind::Skill,
                capability: p::CapabilityRef("skill:m5-gateway".into()),
                payload_digest: payload_digest.clone(),
                required_permissions: vec![p::PermissionRef("permission:read".into())],
                risk: p::Risk::Low,
                network: false,
                hook: false,
            }],
            dependencies: Vec::new(),
            sbom_digest: p::sha256_content_digest(b"m5 gateway SBOM"),
            license_expression: "MIT".into(),
            body_digest: p::SchemaDigest(String::new()),
            max_unpacked_bytes: 1024,
            contains_executable: false,
        },
        resources: vec![p::CapabilityPackageResource {
            schema_version: p::M5_SCHEMA_VERSION,
            relative_path: "skill.txt".into(),
            content,
            digest: payload_digest,
        }],
        package_digest: p::SchemaDigest(String::new()),
        signature: p::PackageSignature(format!("ed25519:{}", "00".repeat(64))),
    };
    package.refresh_digests().unwrap();
    package.validate().unwrap();
    package
}

pub(super) fn m5_gateway_admission(
    package: &p::SignedCapabilityPackage,
) -> p::CapabilityPackageAdmission {
    p::CapabilityPackageAdmission {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityAdmissionRef("admission:m5-gateway:v1".into()),
        package: package.manifest.package.clone(),
        release: package.manifest.release.clone(),
        package_digest: package.package_digest.clone(),
        publisher_grant: p::CapabilityPublisherGrantRef("grant:m5-gateway:v1".into()),
        publisher_version: p::Version(1),
        policy: p::CapabilityPolicyRef("policy:m5-gateway".into()),
        policy_version: p::Version(1),
        checks: p::CapabilityAdmissionCheckKind::ALL
            .into_iter()
            .map(|kind| p::CapabilityAdmissionCheck {
                schema_version: p::M5_SCHEMA_VERSION,
                kind,
                verdict: p::CapabilityAdmissionVerdict::Pass,
                evidence: p::EvidenceRef(format!("evidence:m5-gateway:{kind:?}")),
            })
            .collect(),
        dependencies: Vec::new(),
        admitted_at: 1,
    }
}

pub(super) fn m5_gateway_publisher() -> p::CapabilityPublisherGrant {
    p::CapabilityPublisherGrant {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPublisherGrantRef("grant:m5-gateway:v1".into()),
        publisher: p::CapabilityPublisherRef("publisher:m5-gateway".into()),
        public_key_digest: p::sha256_content_digest(b"m5 gateway publisher key"),
        allowed_kinds: vec![p::CapabilityPackageKind::Skill],
        scope: p::Scope("workspace:m5-gateway".into()),
        expires_at: i64::MAX,
        version: p::Version(1),
        status: p::CapabilityPublisherStatus::Active,
    }
}

pub(super) fn m5_gateway_policy() -> p::CapabilityAdmissionPolicy {
    p::CapabilityAdmissionPolicy {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPolicyRef("policy:m5-gateway".into()),
        version: p::Version(1),
        allowed_kinds: vec![p::CapabilityPackageKind::Skill],
        allowed_licenses: vec!["MIT".into()],
        max_package_bytes: 1024,
        max_dependencies: 1,
        max_depth: 1,
        allow_network: false,
        allow_hooks: false,
    }
}

pub(super) fn m5_gateway_distribution_envelope(
    plan: p::CapabilityInstallPlan,
    target_peer: p::FederatedPeerRef,
    authority_epoch: p::AuthorityEpoch,
) -> p::Result<p::CapabilityPackageDistributionEnvelope> {
    let package = m5_gateway_package();
    let mut envelope = p::CapabilityPackageDistributionEnvelope {
        schema_version: p::M5_SCHEMA_VERSION,
        install_plan: plan,
        admission: m5_gateway_admission(&package),
        package,
        dependency_admissions: Vec::new(),
        publisher_grant: m5_gateway_publisher(),
        authority_policy: m5_gateway_policy(),
        target_peer,
        authority_epoch,
        content_digest: p::SchemaDigest(String::new()),
    };
    envelope.refresh_digest()?;
    envelope.validate()?;
    Ok(envelope)
}

impl AgentHarness for FakeHarness {
    fn submit_run(&self, request: p::RunRequest) -> p::Result<p::RunId> {
        self.submit_ingress(request, Vec::new())
    }

    fn stream_events(&self, run: p::RunId) -> EventStream {
        EventStream::new(
            self.runs
                .lock()
                .ok()
                .and_then(|runs| runs.get(&run).cloned())
                .map(|run| run.events)
                .unwrap_or_default(),
        )
    }

    fn wait(&self, run: p::RunId) -> p::Result<p::RunResult> {
        self.runs
            .lock()
            .map_err(|_| p::Error("fake run state unavailable".into()))?
            .get(&run)
            .map(|run| run.result.clone())
            .ok_or_else(|| p::Error("fake run not found".into()))
    }

    fn cancel(&self, _run: p::RunId) -> p::Result<()> {
        Ok(())
    }

    fn resume(&self, _run: p::RunId, _input: ResumeInput) -> p::Result<()> {
        Ok(())
    }

    fn drain(&self, _session: p::SessionId) -> p::Result<()> {
        Ok(())
    }
}

impl HarnessIngress for FakeHarness {
    fn ingress_authority(&self) -> IngressAuthority {
        self.ingress_authority.clone()
    }

    fn submit_ingress(
        &self,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId> {
        self.requests
            .lock()
            .map_err(|_| p::Error("fake request capture unavailable".into()))?
            .push(request.clone());
        let id = self.sequence.fetch_add(1, Ordering::SeqCst);
        let run = p::RunId(format!("fake-run:{id}"));
        let mut events = prelude
            .into_iter()
            .enumerate()
            .map(|(index, event)| {
                if !event.is_authorized_by(&self.ingress_authority) {
                    return Err(p::Error("fake harness rejected an ingress stamp".into()));
                }
                let (payload, provenance) = event.into_parts();
                Ok(Self::event(&run, index as u64 + 1, payload, provenance))
            })
            .collect::<p::Result<Vec<_>>>()?;
        let provenance = if request.source == p::Source::UserTurn {
            p::Provenance {
                source: request.source,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            }
        } else {
            p::Provenance {
                source: request.source,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::Untrusted,
                caused_by: None,
            }
        };
        let final_path = vec![
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: request.source,
                session_ref: p::SessionId(request.session.0),
                input_ref: p::InputRef(request.input.0),
                idempotency_key: request.idempotency_key,
            }),
            p::EventPayload::SessionBound(p::SessionBoundPayload {
                policy_profile: p::PolicyProfileRef("policy:fake".into()),
                model_profile: p::ModelProfileRef("model:fake".into()),
                toolset_ref: p::ToolsetRef("toolset:fake".into()),
                workspace: p::WorkspaceRef("workspace:fake".into()),
                effect_mode: None,
                evolution_snapshot: None,
                federation_snapshot: None,
                composition_manifest: None,
            }),
            p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 0 }),
            p::EventPayload::ContextBuildStarted(p::ContextBuildStartedPayload {
                sources: Vec::new(),
                slice_refs: Vec::new(),
            }),
            p::EventPayload::ContextBuildFinished(p::ContextBuildFinishedPayload {
                sources: Vec::new(),
                slice_refs: Vec::new(),
            }),
            p::EventPayload::ModelCallStarted(p::ModelCallStartedPayload {
                call_id: p::ModelCallId("model-call:fake".into()),
                model_profile: p::ModelProfileRef("model:fake".into()),
            }),
            p::EventPayload::ModelCallFinished(p::ModelCallFinishedPayload {
                call_id: p::ModelCallId("model-call:fake".into()),
                model_profile: p::ModelProfileRef("model:fake".into()),
                usage: p::ModelUsage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                finish_reason: p::FinishReason("stop".into()),
            }),
            p::EventPayload::OutputClassified(p::OutputClassifiedPayload {
                kind: p::OutputKind::Final,
            }),
            p::EventPayload::VerificationStarted(p::VerificationStartedPayload {
                verifier_kind: p::VerifierKind("deterministic".into()),
                against: p::DoneContractRef("done:fake".into()),
            }),
            p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                verifier_kind: p::VerifierKind("deterministic".into()),
                outcome: p::VerificationOutcome::Pass,
                against: p::DoneContractRef("done:fake".into()),
            }),
            p::EventPayload::TurnComplete(p::TurnCompletePayload { turn_index: 0 }),
            p::EventPayload::RunComplete(p::RunCompletePayload {
                stop_reason: p::StopReason("final_output".into()),
                result_ref: None,
            }),
        ];
        for payload in final_path {
            let sequence = events.len() as u64 + 1;
            events.push(Self::event(&run, sequence, payload, provenance.clone()));
        }
        let output = self
            .responses
            .lock()
            .map_err(|_| p::Error("fake responses unavailable".into()))?
            .pop_front()
            .ok_or_else(|| p::Error("fake response missing".into()))?;
        self.runs
            .lock()
            .map_err(|_| p::Error("fake run state unavailable".into()))?
            .insert(
                run.clone(),
                FakeRun {
                    events,
                    result: p::RunResult {
                        schema_version: p::SchemaVersion(1),
                        status: p::RunStatus::Complete,
                        stop_reason: p::StopReason("final_output".into()),
                        outputs: Vec::new(),
                        evidence_refs: Vec::new(),
                    },
                    output,
                },
            );
        Ok(run)
    }

    fn resolve_approval(&self, _ticket: ApprovalTicket, _grant: ApprovalGrant) -> p::Result<()> {
        Ok(())
    }

    fn append_ingress_events(
        &self,
        run: p::RunId,
        events: Vec<IngressEvent>,
    ) -> p::Result<Vec<p::EventId>> {
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| p::Error("fake run state unavailable".into()))?;
        let record = runs.entry(run.clone()).or_insert_with(|| FakeRun {
            events: Vec::new(),
            result: p::RunResult {
                schema_version: p::SchemaVersion(1),
                status: p::RunStatus::Complete,
                stop_reason: p::StopReason("audit_only".into()),
                outputs: Vec::new(),
                evidence_refs: Vec::new(),
            },
            output: String::new(),
        });
        let mut ids = Vec::new();
        for event in events {
            if !event.is_authorized_by(&self.ingress_authority) {
                return Err(p::Error("fake harness rejected an ingress stamp".into()));
            }
            let sequence = record.events.len() as u64 + 1;
            let (payload, provenance) = event.into_parts();
            let stored = Self::event(&run, sequence, payload, provenance);
            ids.push(stored.event_id.clone());
            record.events.push(stored);
        }
        Ok(ids)
    }

    fn result_text(&self, run: p::RunId) -> p::Result<Option<String>> {
        Ok(self
            .runs
            .lock()
            .map_err(|_| p::Error("fake run state unavailable".into()))?
            .get(&run)
            .map(|run| run.output.clone()))
    }
}

impl HarnessGatewayControl for FakeHarness {
    fn start_run(self: Arc<Self>, request: p::RunRequest) -> p::Result<p::RunId> {
        HarnessIngress::submit_ingress(self.as_ref(), request, Vec::new())
    }

    fn gateway_profile(&self, surface: p::SurfaceRef) -> p::Result<p::GatewayProfile> {
        Ok(p::GatewayProfile {
            schema_version: p::SchemaVersion(1),
            surface,
            policy: p::PolicyProfileRef("policy:fake".into()),
            model: p::ModelProfileRef("model:fake".into()),
            toolset: p::ToolsetRef("toolset:fake".into()),
            workspace: p::WorkspaceRef("workspace:fake".into()),
        })
    }

    fn list_runs(&self) -> p::Result<Vec<p::RunSummary>> {
        let run_ids = self
            .runs
            .lock()
            .map_err(|_| p::Error("fake run state unavailable".into()))?
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        run_ids
            .into_iter()
            .map(|run| HarnessGatewayControl::run_summary(self, run))
            .collect()
    }

    fn stream_event_page(&self, cursor: p::EventCursor) -> p::Result<p::EventPage> {
        cursor.validate()?;
        let events = self
            .runs
            .lock()
            .map_err(|_| p::Error("fake run state unavailable".into()))?
            .get(&cursor.run)
            .map(|run| run.events.clone())
            .ok_or_else(|| p::Error("fake run not found".into()))?;
        let snapshot_upper_bound = events.last().map(|event| event.stream_seq).unwrap_or(0);
        let page = p::EventPage {
            schema_version: p::SchemaVersion(1),
            run: cursor.run,
            after_stream_seq: cursor.after_stream_seq,
            snapshot_upper_bound,
            events: events
                .into_iter()
                .filter(|event| event.stream_seq > cursor.after_stream_seq)
                .collect(),
        };
        page.validate()?;
        Ok(page)
    }

    fn run_summary(&self, run: p::RunId) -> p::Result<p::RunSummary> {
        let runs = self
            .runs
            .lock()
            .map_err(|_| p::Error("fake run state unavailable".into()))?;
        let record = runs
            .get(&run)
            .ok_or_else(|| p::Error("fake run not found".into()))?;
        let (source, session) = record
            .events
            .iter()
            .find_map(|event| match &event.payload {
                p::EventPayload::RunAccepted(payload) => {
                    Some((payload.source, payload.session_ref.clone()))
                }
                _ => None,
            })
            .ok_or_else(|| p::Error("fake run acceptance missing".into()))?;
        Ok(p::RunSummary {
            schema_version: p::SchemaVersion(1),
            run,
            source,
            session,
            workspace: Some(p::WorkspaceRef("workspace:fake".into())),
            status: record.result.status,
            last_stream_seq: record
                .events
                .last()
                .map(|event| event.stream_seq)
                .unwrap_or(0),
            result: Some(record.result.clone()),
        })
    }

    fn pending_approvals(&self, _session: p::SessionId) -> p::Result<Vec<p::PendingApproval>> {
        Ok(Vec::new())
    }

    fn control(&self, _run: p::RunId, _control: p::RunControl) -> p::Result<()> {
        Ok(())
    }

    fn trace_view(&self, run: p::RunId) -> p::Result<p::TraceView> {
        let events = self.stream_events(run.clone()).events();
        Ok(p::TraceView {
            schema_version: p::SchemaVersion(1),
            run,
            snapshot_upper_bound: events.last().map(|event| event.stream_seq).unwrap_or(0),
            failure_refs: Vec::new(),
            verification_outcomes: events
                .iter()
                .filter_map(|event| match &event.payload {
                    p::EventPayload::VerificationFinished(payload) => Some(payload.outcome.clone()),
                    _ => None,
                })
                .collect(),
            events,
        })
    }

    fn review_candidate(&self, _command: p::CandidateReviewCommand) -> p::Result<()> {
        Ok(())
    }
}

impl EcosystemGatewayControl for FakeHarness {
    fn provision_capability_publisher(
        &self,
        _run: p::RunId,
        _grant: p::CapabilityPublisherGrant,
        _previous: Option<p::CapabilityPublisherGrantRef>,
        _expected: p::EcosystemAggregateVersion,
        _owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend> {
        Err(p::Error(
            "fake ecosystem publisher mutation is unsupported".into(),
        ))
    }

    fn admit_capability_package(
        &self,
        _run: p::RunId,
        _catalog_run: p::RunId,
        _package: p::SignedCapabilityPackage,
        _now: p::Timestamp,
        _owner: p::VerifiedPrincipal,
    ) -> p::Result<p::CapabilityPackageAdmission> {
        Err(p::Error(
            "fake ecosystem admission mutation is unsupported".into(),
        ))
    }

    fn prepare_capability_change(
        &self,
        _operation: p::CapabilityPackageOperation,
        _package: p::CapabilityPackageRef,
        _release: p::CapabilityReleaseRef,
        _owner: p::VerifiedPrincipal,
    ) -> p::Result<p::CapabilityInstallPlan> {
        Err(p::Error(
            "fake ecosystem lifecycle planning is unsupported".into(),
        ))
    }

    fn apply_capability_change(
        &self,
        _run: p::RunId,
        _plan: p::CapabilityInstallPlan,
        _approval: p::CapabilityPackageApproval,
        _reason: p::ReasonRef,
        _now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageState> {
        Err(p::Error(
            "fake ecosystem lifecycle mutation is unsupported".into(),
        ))
    }

    fn prepare_capability_distribution(
        &self,
        plan: p::CapabilityInstallPlan,
        target_peer: p::FederatedPeerRef,
        authority_epoch: p::AuthorityEpoch,
        owner: p::VerifiedPrincipal,
        now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageDistributionEnvelope> {
        self.ecosystem_distributions
            .lock()
            .map_err(|_| p::Error("fake ecosystem capture is unavailable".into()))?
            .push((
                plan.clone(),
                target_peer.clone(),
                authority_epoch,
                owner,
                now,
            ));
        m5_gateway_distribution_envelope(plan, target_peer, authority_epoch)
    }

    fn capability_ecosystem_snapshot(
        &self,
        scope: p::Scope,
    ) -> p::Result<p::CapabilityEcosystemSnapshot> {
        Ok(p::CapabilityEcosystemSnapshot::empty(scope))
    }
}

impl EvolutionGatewayControl for FakeHarness {
    fn evolution_snapshot(&self, _scope: p::Scope) -> p::Result<p::EvolutionSnapshot> {
        Err(p::Error("fake evolution projection is empty".into()))
    }

    fn record_strategy_candidate(
        &self,
        _run: p::RunId,
        _candidate: p::StrategyCandidate,
    ) -> p::Result<p::EventId> {
        Err(p::Error("fake evolution candidate is unsupported".into()))
    }

    fn evaluate_strategy(
        &self,
        _run: p::RunId,
        _candidate: &p::StrategyCandidate,
        _comparison: p::EvolutionComparison,
    ) -> p::Result<EvolutionEvaluationResult> {
        Err(p::Error("fake evolution evaluation is unsupported".into()))
    }

    fn promote_strategy(
        &self,
        _run: p::RunId,
        _candidate: &p::StrategyCandidate,
        _evaluation: &p::EvolutionEvaluation,
    ) -> p::Result<p::EventId> {
        Err(p::Error("fake evolution promotion is unsupported".into()))
    }

    fn activate_strategy(
        &self,
        _run: p::RunId,
        _aggregate: p::EvolutionAggregateRef,
        _candidate: &p::StrategyCandidate,
        _evaluation: &p::EvolutionEvaluation,
        _promotion: p::EventId,
        _owner_confirmation: Option<p::OwnerControlRef>,
    ) -> p::Result<EvolutionActivationResult> {
        Err(p::Error("fake evolution activation is unsupported".into()))
    }

    fn rollback_strategy(
        &self,
        _run: p::RunId,
        _aggregate: p::EvolutionAggregateRef,
        _domain: p::StrategyDomain,
        _scope: p::Scope,
        _restored: p::StrategyVersionRef,
        _triggers: Vec<p::EvidenceRef>,
        _in_flight: p::InFlightDisposition,
        _owner_confirmation: Option<p::OwnerControlRef>,
    ) -> p::Result<EvolutionActivationResult> {
        Err(p::Error("fake evolution rollback is unsupported".into()))
    }

    fn set_auto_activation_paused(&self, paused: bool) {
        self.evolution_paused.store(paused, Ordering::SeqCst);
    }

    fn auto_activation_paused(&self) -> bool {
        self.evolution_paused.load(Ordering::SeqCst)
    }
}

impl HarnessFederationGatewayControl for FakeHarness {
    fn federation_snapshot(&self, _scope: p::Scope) -> p::Result<p::FederationSnapshot> {
        let mut grants = ["peer:owner-device-b", "peer:replica-b"]
            .into_iter()
            .map(|peer| {
                Self::federated_grant(&p::FederatedPeerRef(peer.into()))
                    .expect("fake grant")
                    .reference()
            })
            .collect::<p::Result<Vec<_>>>()?;
        grants.sort_by(|left, right| left.0.cmp(&right.0));
        let authority = p::AuthorityRef("authority:fake".into());
        let authority_epoch = p::AuthorityEpoch(1);
        let registry_version = p::FederationAggregateVersion {
            schema_version: p::M4_SCHEMA_VERSION,
            aggregate: p::FederationAggregateRef("federation".into()),
            version: 2,
        };
        let digest =
            p::canonical_digest(&(&authority, authority_epoch, &registry_version, &grants))?;
        Ok(p::FederationSnapshot {
            schema_version: p::M4_SCHEMA_VERSION,
            authority,
            authority_epoch,
            registry_version,
            grants,
            digest,
        })
    }

    fn federated_peer(
        &self,
        peer: &p::FederatedPeerRef,
    ) -> p::Result<Option<p::FederatedPeerState>> {
        Ok(
            Self::federated_grant(peer).map(|grant| p::FederatedPeerState {
                schema_version: p::M4_SCHEMA_VERSION,
                grant,
                revoked: false,
                last_seen_at: None,
            }),
        )
    }

    fn register_federated_peer(
        &self,
        _run: p::RunId,
        _grant: p::FederatedPeerGrant,
        _previous: Option<p::FederatedPeerGrantRef>,
        _expected: p::FederationAggregateVersion,
        _owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend> {
        Err(p::Error("fake federation enrollment is unsupported".into()))
    }

    fn revoke_federated_peer(
        &self,
        _run: p::RunId,
        _peer: p::FederatedPeerRef,
        _grant: p::FederatedPeerGrantRef,
        _in_flight: p::InFlightDisposition,
        _expected: p::FederationAggregateVersion,
        _owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend> {
        Err(p::Error("fake federation revocation is unsupported".into()))
    }

    fn apply_federated_owner_command(
        &self,
        envelope: p::FederatedControlEnvelope,
        command: p::FederatedOwnerCommand,
        now: p::Timestamp,
    ) -> p::Result<FederationControlResult> {
        let result = match &command {
            p::FederatedOwnerCommand::ResolveApproval { .. } => {
                FederationControlResult::ApprovalResolved {
                    run: p::RunId("run:fake-federation".into()),
                    terminal: false,
                }
            }
            p::FederatedOwnerCommand::Cancel { lease, .. } => FederationControlResult::Cancelled {
                lease: lease.clone(),
                terminal: false,
            },
            p::FederatedOwnerCommand::RequestRetention(request) => {
                FederationControlResult::RetentionRequested(request.request.clone())
            }
            p::FederatedOwnerCommand::Register(_) | p::FederatedOwnerCommand::Revoke { .. } => {
                return Err(p::Error(
                    "fake owner-client enrollment is unsupported".into(),
                ));
            }
        };
        self.federation_commands
            .lock()
            .map_err(|_| p::Error("fake federation capture is unavailable".into()))?
            .push((envelope, command, now));
        Ok(result)
    }

    fn federated_lease(
        &self,
        _lease: &p::RemoteExecutionLeaseRef,
    ) -> p::Result<Option<p::RemoteExecutionLease>> {
        Ok(None)
    }

    fn federated_retention_state(
        &self,
        _request: &p::RetentionRequestRef,
    ) -> p::Result<Option<forme_harness::RetentionState>> {
        Ok(None)
    }

    fn recover_federated_remote_action(
        &self,
        _run: &p::RunId,
    ) -> p::Result<forme_harness::RemoteRecoveryResult> {
        Err(p::Error("fake remote recovery is unsupported".into()))
    }

    fn acknowledge_federated_replication(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        batch: &p::ReplicationBatch,
        ack: p::ReplicationAck,
        expected: p::FederationAggregateVersion,
        _now: p::Timestamp,
    ) -> p::Result<p::ExpectedAppend> {
        batch.validate()?;
        ack.validate()?;
        let committed = expected.next()?;
        self.federation_acks
            .lock()
            .map_err(|_| p::Error("fake federation ack capture is unavailable".into()))?
            .push((authenticated_peer.clone(), ack));
        Ok(p::ExpectedAppend {
            schema_version: p::M4_SCHEMA_VERSION,
            status: p::ExpectedAppendStatus::Applied,
            expected_version: expected.version,
            actual_version: expected.version,
            resulting_version: committed.version,
            event_id: Some(p::EventId("fake:replication-checkpoint".into())),
        })
    }

    fn accept_federated_retention_receipt(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        receipt: p::FederatedRetentionReceipt,
        _now: p::Timestamp,
    ) -> p::Result<()> {
        self.federation_retention_receipts
            .lock()
            .map_err(|_| p::Error("fake retention capture is unavailable".into()))?
            .push((authenticated_peer.clone(), receipt));
        Ok(())
    }

    fn accept_federated_device_signal(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        signal: p::FederatedDeviceSignal,
        _now: p::Timestamp,
    ) -> p::Result<bool> {
        self.federation_signals
            .lock()
            .map_err(|_| p::Error("fake signal capture is unavailable".into()))?
            .push((authenticated_peer.clone(), signal));
        Ok(true)
    }
}

impl HarnessFederationActionGateway for FakeHarness {
    fn submit_federated_remote_action(
        &self,
        run: p::RunId,
        request: p::RunRequest,
        intent: p::ActionIntent,
    ) -> p::Result<forme_harness::RemoteActionSubmission> {
        self.federation_actions
            .lock()
            .map_err(|_| p::Error("fake federation action capture is unavailable".into()))?
            .push((run.clone(), request, intent));
        Ok(forme_harness::RemoteActionSubmission {
            run,
            approval: p::ApprovalId("approval:fake-federation".into()),
            plan_digest: p::PlanDigest("sha256:fake-federation-plan".into()),
            federation_snapshot: p::FederationSnapshotRef("sha256:fake-federation-snapshot".into()),
            placement_decision: p::FederatedPlacementDecision {
                schema_version: p::M4_SCHEMA_VERSION,
                decision: p::PlacementDecisionRef("decision:fake-federation".into()),
                federation_snapshot: p::FederationSnapshotRef(
                    "sha256:fake-federation-snapshot".into(),
                ),
                scope: p::Scope("workspace:m4".into()),
                capability: p::CapabilityRef("fixture.mutate".into()),
                evaluated_at: 1,
                candidates: Vec::new(),
                chosen: None,
                placement: None,
                digest: p::SchemaDigest("sha256:fake-placement-decision".into()),
            },
        })
    }
}

pub(super) fn response(text: &str) -> String {
    text.into()
}

pub(super) fn gateway(responses: Vec<String>) -> LocalGateway<FakeHarness> {
    let harness = Arc::new(FakeHarness::new(responses));
    LocalGateway::new(
        harness,
        p::VerifiedPrincipal("owner:test".into()),
        p::AgentProfileRef("agent:test".into()),
    )
    .unwrap()
}
