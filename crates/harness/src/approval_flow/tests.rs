//! Real Harness/approval/backend/Host journal ordering; the OS effect is counted.
use super::*;
use forme_execution::installed::{
    DesktopAuthority, DesktopSessionCatalogProvider, ObservedDesktopSession, WorkerActionBackend,
    WorkerBrokerTransport,
};
use forme_models::{ModelOutput, ModelResponse, ModelToolCall};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Directory;
impl DesktopSessionCatalogProvider for Directory {
    fn sessions(&self) -> p::Result<Vec<ObservedDesktopSession>> {
        Ok(vec![ObservedDesktopSession {
            schema_version: p::SchemaVersion(1),
            helper_ref: "helper:harness-fixture".into(),
            platform: p::DesktopPlatform::Windows,
            label: "Harness fixture desktop".into(),
            os_user_ref: "os-user:fixture".into(),
            os_session_ref: "os-session:fixture".into(),
            session_generation: 1,
            desktop_ref: "desktop:fixture".into(),
            capabilities: [p::DesktopCapability::PointerClick].into_iter().collect(),
        }])
    }
    fn recheck(&self, original: &ObservedDesktopSession) -> p::Result<()> {
        if self.sessions()?.contains(original) {
            Ok(())
        } else {
            Err(p::Error("session changed".into()))
        }
    }
}
struct Transport {
    authority: Arc<DesktopAuthority>,
    issues: AtomicUsize,
    effects: AtomicUsize,
}
impl WorkerBrokerTransport for Transport {
    fn register_desktop_pending(
        &self,
        request: p::DesktopPendingAction,
    ) -> p::Result<p::DesktopPendingAction> {
        self.authority.register_pending(request, now_ms())
    }
    fn desktop_pending_consent(&self, pending_ref: &str) -> p::Result<p::DesktopConsentView> {
        self.authority.pending_consent(pending_ref, now_ms())
    }
    fn acquire_desktop_grant(
        &self,
        request: p::DesktopGrantRequest,
    ) -> p::Result<p::DesktopGrantView> {
        self.issues.fetch_add(1, Ordering::SeqCst);
        self.authority.issue(request, now_ms())
    }
    fn desktop_grant(&self, operation_id: &str) -> p::Result<p::DesktopGrantView> {
        self.authority.grant_operation(operation_id, now_ms())
    }
    fn dispatch(&self, request: p::WorkerDispatchRequest) -> p::Result<p::WorkerObservation> {
        request.validate_at(now_ms())?;
        let view = self.authority.grant_operation(
            &format!("desktop-issue:{}", request.job.plan_digest.0),
            now_ms(),
        )?;
        if view.status != p::DesktopGrantStatus::Issued
            || request
                .execution
                .as_ref()
                .and_then(|s| s.desktop_session.as_ref())
                != Some(&view.binding)
        {
            return Err(p::Error("fixture rejects inactive grant".into()));
        }
        self.effects.fetch_add(1, Ordering::SeqCst);
        Err(p::Error(
            "fixture loses response after actual effect".into(),
        ))
    }

    fn probe(&self, _: &str) -> p::Result<p::WorkerObservation> {
        Err(p::Error("unknown original job".into()))
    }
    fn cancel(&self, _: &str) -> p::Result<p::WorkerCancelReceipt> {
        Err(p::Error("unknown original job".into()))
    }
    fn result_manifest(&self, _: &str) -> p::Result<p::WorkerResultManifest> {
        Err(p::Error("no result".into()))
    }
    fn read_result(&self, _: p::WorkerResultRead) -> p::Result<p::WorkerResultChunk> {
        Err(p::Error("no result".into()))
    }
}
struct Fixture {
    harness: ReactiveHarness,
    transport: Arc<Transport>,
    run: p::RunId,
    request: ApprovalRequest,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forme-desktop-harness-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let authority = Arc::new(
            DesktopAuthority::open(
                &path,
                "installation:harness".into(),
                p::VerifiedPrincipal("owner:desktop".into()),
                p::AuthorityEpoch(1),
                Arc::new(Directory),
            )
            .unwrap(),
        );
        let transport = Arc::new(Transport {
            authority,
            issues: AtomicUsize::new(0),
            effects: AtomicUsize::new(0),
        });
        let backend = Arc::new(
            WorkerActionBackend::new(
                p::BackendKind::Computer,
                transport.clone(),
                p::VerifiedPrincipal("owner:desktop".into()),
                p::AuthorityEpoch(1),
                "sandbox:desktop",
                OutputBudget::strict(8192),
                p::DurationMs(10_000),
            )
            .unwrap(),
        );
        let bounds = p::CoordinateBounds {
            schema_version: p::SchemaVersion(1),
            min_x: 0,
            min_y: 0,
            max_x_exclusive: 800,
            max_y_exclusive: 600,
        };
        let intent = p::ActionIntent {
            schema_version: p::SchemaVersion(1),
            intent_id: p::ActionId("action:desktop".into()),
            source: p::Source::UserTurn,
            goal: p::GoalRef("fixture desktop click".into()),
            backend_hint: p::BackendKind::Computer,
            capability_ref: p::CapabilityRef("capability:computer".into()),
            action_type: p::ActionType::Execute,
            scope: p::Scope("workspace:default".into()),
            risk_hint: p::Risk::Low,
            expected_effect: p::ExpectedEffect::Outward,
            rollback_expectation: p::RollbackBoundary("none".into()),
            parameters: p::ActionParameters::Computer(p::ComputerActionSpec {
                schema_version: p::SchemaVersion(1),
                driver: p::ProviderId("computer:native".into()),
                surface: p::SurfaceRef("surface:fixture".into()),
                bounds,
                operation: p::ComputerOperation::Click {
                    x: 20,
                    y: 30,
                    button: p::PointerButton::Primary,
                },
                artifact_scope: p::Scope("workspace:default".into()),
            }),
            requested_permissions: vec![],
            requested_at: now_ms(),
            estimated_output_bytes: 128,
            estimated_duration: p::DurationMs(100),
        };
        let registry = Arc::new(ExecutionBackendRegistry::default());
        registry.register(backend.clone()).unwrap();
        let envelope = p::AutonomyEnvelope {
            schema_version: p::SchemaVersion(1),
            scope: intent.scope.clone(),
            capability: p::CapabilitySet {
                schema_version: p::SchemaVersion(1),
                capabilities: vec![intent.capability_ref.clone()],
                permissions: vec![],
            },
            action_type: vec![intent.action_type],
            risk_limit: p::Risk::High,
            approval_rule: p::ApprovalRule::Allow,
            budget: p::Budget("units:4".into()),
            timebox: p::Timebox {
                schema_version: p::SchemaVersion(1),
                starts_at: now_ms() - 1000,
                expires_at: now_ms() + 60_000,
                max_turns: 4,
            },
            rollback: p::RollbackReq {
                schema_version: p::SchemaVersion(1),
                required: false,
                boundary: None,
            },
        };
        let governance = GovernanceConfig {
            layers: vec![PolicyLayer {
                schema_version: p::SchemaVersion(1),
                source: PolicyLayerSource::User,
                rules: vec![PolicyRule {
                    schema_version: p::SchemaVersion(1),
                    matcher: ActionMatcher {
                        backend: Some(p::BackendKind::Computer),
                        capability: Some(intent.capability_ref.clone()),
                        action_type: Some(intent.action_type),
                        parameters: ArgMatcher::Any,
                    },
                    effect: p::PolicyDecision::Allow,
                    scope: intent.scope.clone(),
                }],
            }],
            visible_capabilities: vec![intent.capability_ref.clone()],
            allowed_scopes: vec![intent.scope.clone()],
            sandbox_available: true,
            external: forme_policy::ExternalPolicyLimits {
                computer_surfaces: vec![forme_policy::ComputerSurfaceLimit {
                    schema_version: p::SchemaVersion(1),
                    surface: p::SurfaceRef("surface:fixture".into()),
                    bounds,
                }],
                ..Default::default()
            },
            delegation: Some(DelegationGrant {
                schema_version: p::SchemaVersion(1),
                subject: DelegationSubject::Owner,
                envelope: envelope.clone(),
                granted_by: p::Actor::Owner,
                audit_ref: p::EventId("delegation:fixture".into()),
            }),
            envelope: Some(envelope),
            ..Default::default()
        };
        let mut profile = action_only_model_profile().unwrap();
        profile.capability.tool_use = true;
        let model = Arc::new(
            ScriptedModelProvider::new(
                profile.clone(),
                vec![ModelResponse {
                    schema_version: p::SchemaVersion(1),
                    output: ModelOutput::Tool(Box::new(ModelToolCall {
                        schema_version: p::SchemaVersion(1),
                        call_id: p::ToolCallId("call:desktop".into()),
                        tool: p::ToolRef("tool:computer".into()),
                        arguments: serde_json::json!({}),
                        intent: Some(intent),
                    })),
                    usage: p::ModelUsage {
                        input_tokens: 4,
                        output_tokens: 2,
                    },
                    finish_reason: p::FinishReason("tool_calls".into()),
                }],
            )
            .unwrap(),
        );
        let mut harness = ReactiveHarness::new(
            SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap(),
            model,
            registry,
            governance,
            HarnessConfig::for_model(&profile),
        )
        .unwrap()
        .with_competence_gate(Arc::new(FixedCompetenceGate::new(
            cognition::InterventionLevel::L5HighImpact,
        )));
        harness.desktop_backend = Some(backend);
        harness.configured_owner = Some(p::VerifiedPrincipal("owner:desktop".into()));
        let run = harness
            .submit_run(p::RunRequest {
                schema_version: p::SchemaVersion(1),
                source: p::Source::UserTurn,
                session: p::SessionRef("session:desktop".into()),
                agent_profile: p::AgentProfileRef("agent:desktop".into()),
                input: p::RunInput("fixture desktop action".into()),
                budget: None,
                idempotency_key: Some(p::IdempotencyKey("request:desktop".into())),
            })
            .unwrap();
        let pending = harness
            .pending_approvals(p::SessionId("session:desktop".into()))
            .unwrap();
        assert_eq!(
            pending.len(),
            1,
            "events: {:?}",
            harness
                .stream_events(run.clone())
                .map(|e| e.payload)
                .collect::<Vec<_>>()
        );
        Self {
            harness,
            transport,
            run,
            request: pending[0].clone(),
            path,
        }
    }
    fn grant(&self) -> ApprovalGrant {
        ApprovalGrant {
            schema_version: p::SchemaVersion(1),
            approval_id: self.request.approval_id.clone(),
            outcome: p::ApprovalOutcome::Granted,
            granted_scope: GrantScope::OneShot,
            approver: p::VerifiedPrincipal("owner:desktop".into()),
            bound_plan_digest: self.request.plan_digest.clone(),
            normalized_action_digest: self.request.normalized_action_digest.clone(),
            affected_resources: self.request.affected_resources.clone(),
            policy_version: self.request.policy_version,
            tool_schema_version: self.request.tool_schema_version,
            nonce: p::Nonce("approval-nonce:desktop".into()),
            use_by: self.request.expires_at - 1,
        }
    }
    fn consent(&self) -> p::DesktopConsentView {
        let catalog = self.transport.authority.sessions(now_ms()).unwrap();
        self.transport
            .authority
            .consent(
                p::DesktopConsentRequest {
                    schema_version: p::SchemaVersion(1),
                    operation_id: "consent:harness".into(),
                    expected_revision: catalog.catalog_revision,
                    pending_ref: p::desktop_pending_ref(
                        &self.request.approval_id,
                        &self.request.plan_digest,
                    )
                    .unwrap(),
                    catalog_revision: catalog.catalog_revision,
                    catalog_digest: catalog.catalog_digest,
                    session_ref: catalog.entries[0].session_ref.clone(),
                    expires_at: now_ms() + 30_000,
                },
                now_ms(),
            )
            .unwrap()
    }
    fn kinds(&self) -> Vec<p::EventKind> {
        self.harness
            .stream_events(self.run.clone())
            .map(|e| e.kind)
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn managed_desktop_approval_waits_without_consuming_nonce_and_unknown_never_replays() {
    let f = Fixture::new();
    assert!(f
        .harness
        .resume(f.run.clone(), ResumeInput::Approval(f.grant()))
        .is_err());
    assert!(!f.kinds().iter().any(|kind| matches!(
        kind,
        p::EventKind::ApprovalResolved | p::EventKind::ActionPlanned | p::EventKind::ActionStarted
    )));
    assert_eq!(f.transport.issues.load(Ordering::SeqCst), 0);
    f.consent();
    // The exact same approval nonce remains usable after the missing-consent attempt.
    f.harness
        .resume(f.run.clone(), ResumeInput::Approval(f.grant()))
        .unwrap();
    let kinds = f.kinds();
    assert!(kinds.contains(&p::EventKind::ActionOutcomeUnknown));
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == p::EventKind::ActionStarted)
            .count(),
        1
    );
    assert_eq!(f.transport.issues.load(Ordering::SeqCst), 1);
    assert_eq!(f.transport.effects.load(Ordering::SeqCst), 1);
    assert!(f
        .harness
        .resume(f.run.clone(), ResumeInput::Approval(f.grant()))
        .is_err());
    assert_eq!(f.transport.effects.load(Ordering::SeqCst), 1);
}

#[test]
fn managed_desktop_revoked_consent_leaves_original_approval_pending() {
    let f = Fixture::new();
    let consent = f.consent();
    f.transport
        .authority
        .revoke(
            p::DesktopRevokeRequest {
                schema_version: p::SchemaVersion(1),
                operation_id: "revoke:harness".into(),
                expected_revision: consent.revision,
                consent_ref: consent.consent_ref,
            },
            now_ms(),
        )
        .unwrap();
    assert!(f
        .harness
        .resume(f.run.clone(), ResumeInput::Approval(f.grant()))
        .is_err());
    assert_eq!(
        f.harness
            .pending_approvals(p::SessionId("session:desktop".into()))
            .unwrap()
            .len(),
        1
    );
    assert!(!f.kinds().contains(&p::EventKind::ApprovalResolved));
    assert_eq!(f.transport.issues.load(Ordering::SeqCst), 0);
    assert_eq!(f.transport.effects.load(Ordering::SeqCst), 0);
}
