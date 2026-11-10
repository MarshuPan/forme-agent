//! Authenticated, finite owner discovery. Public Source claims confer no authority.
use crate::*;
use forme_execution::installed::ManagedWorkerBinding;
use serde::{Deserialize, Serialize};

pub(crate) mod original_job;
mod persistence;
mod policy;
mod publication;
mod recovery;
#[cfg(test)]
mod tests;
mod verification;
use persistence::{read, write, PendingDiscovery};

pub(super) struct DiscoveryRuntime {
    pub(super) settings: Vec<p::ProductMcpServerConfig>,
    pub(super) worker_digest: p::SchemaDigest,
    pub(super) authority: Arc<dyn forme_execution::installed::ManagedMcpSourceVerifier>,
    pub(super) backend: Arc<forme_execution::installed::WorkerActionBackend>,
}

impl DiscoveryRuntime {
    pub(crate) fn new(
        settings: Vec<p::ProductMcpServerConfig>,
        binding: &ManagedWorkerBinding,
        backend: Arc<forme_execution::installed::WorkerActionBackend>,
    ) -> p::Result<Self> {
        binding.validate()?;
        if !binding.supports(p::BackendKind::Mcp)
            || binding.sandbox_profile_ref != p::MANAGED_MCP_PROGRAM_PROFILE
        {
            return Err(invalid());
        }
        for entry in &settings {
            entry.validate()?;
        }
        Ok(Self {
            settings,
            worker_digest: p::canonical_digest(binding)?,
            authority: binding.connect_mcp_source()?,
            backend,
        })
    }

    fn settings(&self, spec: &p::ManagedMcpDiscoverySpec) -> p::Result<&p::ProductMcpServerConfig> {
        let mut matches = self
            .settings
            .iter()
            .filter(|v| v.server_ref == spec.server.0);
        let settings = matches.next().ok_or_else(invalid)?;
        if matches.next().is_some() {
            return Err(invalid());
        }
        spec.validate_settings(settings, &settings.scope)?;
        Ok(settings)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerRequest {
    schema_version: p::SchemaVersion,
    owner: p::VerifiedPrincipal,
    run: p::RunId,
    request: p::RunRequest,
    composition: p::SchemaDigest,
    worker: p::SchemaDigest,
    intent: p::ActionIntent,
    envelope: p::AutonomyEnvelope,
}

impl ReactiveHarness {
    /// Only authenticated owner control may call this entry. The specification
    /// cannot select a tool, command line, environment, credential or endpoint.
    pub fn discover_managed_mcp(
        &self,
        owner: &p::VerifiedPrincipal,
        session: p::SessionId,
        command: p::IdempotencyKey,
        spec: p::ManagedMcpDiscoverySpec,
    ) -> p::Result<p::RunId> {
        let _access = self.data_read_guard()?;
        if self.authority_guard.is_none()
            || self.configured_owner.as_ref() != Some(owner)
            || session.0.trim().is_empty()
            || session.0.len() > 512
            || command.0.trim().is_empty()
            || command.0.len() > 512
        {
            return Err(invalid());
        }
        let runtime = self.owner_discovery.as_ref().ok_or_else(invalid)?;
        let settings = runtime.settings(&spec)?;
        if settings.scope.0 != self.config.workspace.0 {
            return Err(invalid());
        }
        let digest = p::canonical_digest(&(owner, &session, &command))?;
        let run = p::RunId(format!("run:owner-mcp:{}", digest.0));
        let request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::OwnerControl,
            session: p::SessionRef(session.0),
            agent_profile: p::AgentProfileRef("agent:owner-mcp-discovery".into()),
            input: p::RunInput(format!(
                "managed-mcp-discovery:{}",
                p::canonical_digest(&spec)?.0
            )),
            budget: Some(p::Budget("units:1".into())),
            idempotency_key: Some(p::IdempotencyKey(format!("owner-mcp:{}", digest.0))),
        };
        // Check the original before constructing timestamps. An exact replay
        // returns the first run; it never refreshes its timebox or dispatches.
        if let Some(original) = read::<OwnerRequest>(self, &run, "request")? {
            if original.owner != *owner || original.request != request {
                return Err(invalid());
            }
            self.store.require_current_execution(&run)?;
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            if !events.iter().any(|e| matches!(&e.payload, p::EventPayload::RunAccepted(v)
                if v.source == request.source && v.input_ref.0 == request.input.0
                    && v.session_ref.0 == request.session.0 && v.idempotency_key == request.idempotency_key)) {
                return Err(invalid());
            }
            return Ok(run);
        }
        let now = now_ms();
        let expires_at = now
            .checked_add(
                self.config
                    .approval_ttl_ms
                    .try_into()
                    .map_err(|_| invalid())?,
            )
            .ok_or_else(invalid)?;
        let intent = p::ActionIntent {
            schema_version: p::SchemaVersion(1),
            intent_id: p::ActionId(format!("action:owner-mcp:{}", digest.0)),
            source: p::Source::OwnerControl,
            goal: p::GoalRef(format!("goal:{}", run.0)),
            backend_hint: p::BackendKind::Mcp,
            capability_ref: p::CapabilityRef(format!("mcp-discovery:{}", spec.server.0)),
            action_type: p::ActionType::Execute,
            scope: settings.scope.clone(),
            risk_hint: p::Risk::High,
            expected_effect: p::ExpectedEffect::Outward,
            rollback_expectation: p::RollbackBoundary("none".into()),
            requested_permissions: vec![p::PermissionRef("execute".into())],
            requested_at: now,
            estimated_output_bytes: spec.output_budget_bytes,
            estimated_duration: spec.timeout,
            parameters: p::ActionParameters::McpDiscovery(spec),
        };
        let envelope = p::AutonomyEnvelope {
            schema_version: p::SchemaVersion(1),
            scope: settings.scope.clone(),
            capability: p::CapabilitySet {
                schema_version: p::SchemaVersion(1),
                capabilities: vec![intent.capability_ref.clone()],
                permissions: intent.requested_permissions.clone(),
            },
            action_type: vec![p::ActionType::Execute],
            risk_limit: p::Risk::High,
            approval_rule: p::ApprovalRule::Ask,
            budget: p::Budget("units:1".into()),
            timebox: p::Timebox {
                schema_version: p::SchemaVersion(1),
                starts_at: now,
                expires_at,
                max_turns: 1,
            },
            rollback: p::RollbackReq {
                schema_version: p::SchemaVersion(1),
                required: false,
                boundary: None,
            },
        };
        let original = OwnerRequest {
            schema_version: p::SchemaVersion(1),
            owner: owner.clone(),
            run: run.clone(),
            request: request.clone(),
            composition: self.composition_manifest.clone().ok_or_else(invalid)?,
            worker: runtime.worker_digest.clone(),
            intent: intent.clone(),
            envelope: envelope.clone(),
        };
        self.check_discovery_request(&original)?;
        match self.prepare_ingress_internal_as(request, Vec::new(), Some(run.clone()))? {
            PreparedSubmission::Existing(_) => Err(invalid()),
            PreparedSubmission::New {
                run,
                handle,
                session,
            } => {
                // Only this entry writes the private request. A crash before
                // this write leaves an inspectable run, never a model loop.
                write(self, &run, "request", &intent.scope, &original)?;
                {
                    let mut record = handle.record.lock().map_err(|_| invalid())?;
                    record.mode = RunMode::DirectAction;
                    record.owner_discovery_bound = true;
                    record.direct_intent = Some(intent);
                    record.bound_envelope = Some(envelope);
                    record.remaining_action_budget = Some(1);
                }
                self.drive_prepared_run(&run, &handle, &session)?;
                Ok(run)
            }
        }
    }

    fn check_discovery_request(&self, original: &OwnerRequest) -> p::Result<()> {
        let runtime = self.owner_discovery.as_ref().ok_or_else(invalid)?;
        let p::ActionParameters::McpDiscovery(spec) = &original.intent.parameters else {
            return Err(invalid());
        };
        spec.validate_intent(&original.intent)?;
        let settings = runtime.settings(spec)?;
        if original.schema_version != p::SchemaVersion(1)
            || self.configured_owner.as_ref() != Some(&original.owner)
            || self.composition_manifest.as_ref() != Some(&original.composition)
            || runtime.worker_digest != original.worker
            || original.request.source != p::Source::OwnerControl
            || original.request.idempotency_key.is_none()
            || original.intent.scope != settings.scope
            || original.envelope.scope != settings.scope
            || original.envelope.approval_rule != p::ApprovalRule::Ask
            || original.envelope.timebox.expires_at <= now_ms()
            || original.envelope.timebox.starts_at > now_ms()
        {
            return Err(invalid());
        }
        self.store.require_current_execution(&original.run)?;
        Ok(())
    }

    fn bound_discovery(
        &self,
        run: &p::RunId,
        record: &RunRecord,
        intent: &p::ActionIntent,
    ) -> p::Result<Option<OwnerRequest>> {
        if !matches!(intent.parameters, p::ActionParameters::McpDiscovery(_)) {
            return Ok(None);
        }
        let original: OwnerRequest = read(self, run, "request")?.ok_or_else(invalid)?;
        self.check_discovery_request(&original)?;
        if original.run != *run
            || original.request != record.request
            || original.intent != *intent
            || !record.owner_discovery_bound
            || record.mode != RunMode::DirectAction
            || record.bound_envelope.as_ref() != Some(&original.envelope)
            || record.effect_mode == Some(p::EffectMode::CounterfactualDeny)
        {
            return Err(invalid());
        }
        Ok(Some(original))
    }
}

fn invalid() -> p::Error {
    p::Error("owner discovery binding is unavailable, expired or inconsistent".into())
}
