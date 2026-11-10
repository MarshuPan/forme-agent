//! Reconstruct managed metadata from Core originals and the live Host proof.
use super::*;
use forme_capabilities::ManagedMcpCatalogSource;
use forme_execution::installed::ManagedMcpSourceVerifier;
use forme_store::{BrainAuthorityStore, EventStore};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) struct ProtectedMcpCatalogSource {
    pub(crate) store: SqliteEventStore,
    pub(crate) owner: p::VerifiedPrincipal,
    pub(crate) authority: Arc<dyn ManagedMcpSourceVerifier>,
}

impl ManagedMcpCatalogSource for ProtectedMcpCatalogSource {
    fn read_catalog(
        &self,
        server: &p::McpServerRef,
    ) -> p::Result<(p::ProductMcpServerConfig, p::ManagedMcpDiscoveryCatalogV1)> {
        p::validate_product_id(&server.0)?;
        let identity = self.store.brain_authority()?.ok_or_else(invalid)?;
        if identity.owner_digest != p::sha256_content_digest(self.owner.0.as_bytes()) {
            return Err(invalid());
        }
        let heads = self
            .store
            .run_heads_with_kind(p::EventKind::McpDiscovered)?;
        if heads.0.len() > 20_000 {
            return Err(invalid());
        }
        let mut count = 0;
        let mut selected = None;
        for (run, head) in &heads.0 {
            let mut events = Vec::new();
            for event in self.store.read_run(run.clone()) {
                count += 1;
                if count > 100_000 {
                    return Err(invalid());
                }
                events.push(event?);
            }
            for event in &events {
                let p::EventPayload::ActionCompleted(completed) = &event.payload else {
                    continue;
                };
                let Some(id) = completed
                    .receipt
                    .as_ref()
                    .and_then(|r| r.content_ref.as_ref())
                else {
                    continue;
                };
                let Some((material, bytes)) = read_material_original(&self.store, id, &self.owner)?
                else {
                    continue;
                };
                let catalog = validate_managed_mcp_material(
                    &material.request,
                    &material.observation,
                    &material.manifest,
                    &bytes,
                )?;
                if catalog.spec.server != *server {
                    continue;
                }
                // IDs and timestamps do not order discoveries across runs.
                // Only the Host's admission-ordered original can be current.
                let Ok(settings) = self.authority.verify_source(&material.manifest) else {
                    continue;
                };
                let retained = crate::owner_discovery::original_job::matches_retained(
                    &self.store,
                    &self.owner,
                    run,
                    &material.request,
                )?;
                verify_run(
                    run,
                    *head,
                    &events,
                    &material,
                    &settings,
                    &catalog,
                    Checks {
                        published: true,
                        retained_dispatch: retained,
                    },
                )?;
                catalog
                    .spec
                    .validate_settings(&settings, &material.request.job.workspace_scope)?;
                if selected
                    .replace((id.clone(), material, settings, catalog))
                    .is_some()
                {
                    return Err(invalid());
                }
            }
        }
        let (id, material, settings, catalog) = selected.ok_or_else(invalid)?;
        // Read Host first: it may block while a concurrent lifecycle operation
        // fences the Core material. Check that fence after the remote read.
        if self.authority.verify_source(&material.manifest)? != settings {
            return Err(invalid());
        }
        let (again, _) =
            read_material_original(&self.store, &id, &self.owner)?.ok_or_else(invalid)?;
        if again != material
            || self
                .store
                .run_heads_with_kind(p::EventKind::McpDiscovered)?
                .0
                != heads.0
        {
            return Err(invalid());
        }
        Ok((settings, catalog))
    }
}

pub(super) struct Checks {
    pub(super) published: bool,
    pub(super) retained_dispatch: bool,
}

pub(super) fn verify_run(
    run: &p::RunId,
    head: u64,
    events: &[p::Event],
    original: &DiscoveryMaterial,
    settings: &p::ProductMcpServerConfig,
    catalog: &p::ManagedMcpDiscoveryCatalogV1,
    checks: Checks,
) -> p::Result<()> {
    let job = &original.request.job;
    let execution = original.request.execution.as_ref().ok_or_else(invalid)?;
    let mut accepted = false;
    let mut bound = false;
    let mut planned = false;
    let mut started = false;
    let mut completed = false;
    let mut published = false;
    let mut last = 0;
    let mut approvals = BTreeMap::new();
    let mut granted = BTreeMap::new();
    for event in events {
        if event.run_id != *run
            || event.stream_seq != last + 1
            || event.stream_seq > head
            || event.kind != event.payload.kind()
            || event.schema_version != p::SchemaVersion(1)
        {
            return Err(invalid());
        }
        if matches!(
            event.kind,
            p::EventKind::RunAccepted
                | p::EventKind::SessionBound
                | p::EventKind::ApprovalRequested
                | p::EventKind::ApprovalResolved
                | p::EventKind::ActionPlanned
                | p::EventKind::ActionStarted
                | p::EventKind::McpDiscovered
        ) && (event.provenance.source != p::Source::OwnerControl
            || event.provenance.actor != p::Actor::System
            || event.provenance.trust_tier != p::TrustTier::VerifiedProcess)
        {
            return Err(invalid());
        }
        // The Harness stamps external completions as untrusted observations.
        // Integrity comes from the original receipt, never elevated provenance.
        if event.kind == p::EventKind::ActionCompleted
            && event.provenance != crate::untrusted_action_provenance()
        {
            return Err(invalid());
        }
        last = event.stream_seq;
        match &event.payload {
            p::EventPayload::RunAccepted(value) => {
                if accepted
                    || value.source != p::Source::OwnerControl
                    || value.session_ref.0.is_empty()
                    || value.idempotency_key.is_none()
                {
                    return Err(invalid());
                }
                accepted = true;
            }
            p::EventPayload::SessionBound(value) => {
                if !accepted
                    || bound
                    || value.workspace.0 != job.workspace_scope.0
                    || value
                        .composition_manifest
                        .as_ref()
                        .is_none_or(|d| p::validate_product_digest(d).is_err())
                {
                    return Err(invalid());
                }
                bound = true;
            }
            p::EventPayload::ApprovalRequested(value) => {
                if !bound || approvals.insert(value.approval_id.clone(), value).is_some() {
                    return Err(invalid());
                }
            }
            p::EventPayload::ApprovalResolved(value) => {
                if !approvals.contains_key(&value.approval_id)
                    || granted.insert(value.approval_id.clone(), value).is_some()
                {
                    return Err(invalid());
                }
            }
            p::EventPayload::ActionPlanned(value) if value.intent_id == job.action_ref => {
                let approval = value.approval_ref.as_ref().ok_or_else(invalid)?;
                let request = approvals.get(approval).ok_or_else(invalid)?;
                let resolution = granted.get(approval).ok_or_else(invalid)?;
                if !bound
                    || planned
                    || value.backend != p::BackendKind::Mcp
                    || value.plan_digest != job.plan_digest
                    || value.scope != job.workspace_scope
                    || value.expected_effect != p::ExpectedEffect::Outward
                    || value.source != p::Source::OwnerControl
                    || value.remote_placement.is_some()
                    || resolution.outcome != p::ApprovalOutcome::Granted
                    || resolution.grant_ref.is_none()
                    || request.scope != job.workspace_scope
                    || request.plan_digest.as_ref() != Some(&job.plan_digest)
                    || request.normalized_action_digest.as_ref()
                        != Some(&execution.normalized_action_digest)
                    || request.affected_resources != job.affected_resources
                    || request.risk != p::Risk::High
                    || request.expires_at <= original.request.permit.issued_at
                    || request.policy_version.is_none()
                    || request.tool_schema_version.is_none()
                {
                    return Err(invalid());
                }
                planned = true;
            }
            p::EventPayload::ActionStarted(value) if value.intent_id == job.action_ref => {
                if !planned
                    || started
                    || value.backend != p::BackendKind::Mcp
                    || value.scope != job.workspace_scope
                    || value.remote_lease.is_some()
                {
                    return Err(invalid());
                }
                started = true;
            }
            p::EventPayload::ActionCompleted(value) if value.intent_id == job.action_ref => {
                let receipt = value.receipt.as_ref().ok_or_else(invalid)?;
                if !started
                    || completed
                    || value.remote_receipt.is_some()
                    || receipt.schema_version != p::SchemaVersion(1)
                    || receipt.action != job.action_ref
                    || receipt.effect != p::EffectStatus::Observed
                    || receipt.trust != p::TrustTier::Untrusted
                    || receipt.content_ref.as_ref() != Some(&original.content_ref)
                    || receipt.content_digest != original.manifest.content_digest
                    || receipt.probe_hint != Some(p::ProbeHintRef(format!("worker:{}", job.job_id)))
                {
                    return Err(invalid());
                }
                completed = true;
            }
            p::EventPayload::McpDiscovered(v) if v.server == catalog.spec.server => {
                let tools = catalog
                    .tools
                    .iter()
                    .filter(|t| settings.allowlist.tools.contains(&t.name))
                    .map(|t| p::ToolRef(t.name.clone()))
                    .collect::<Vec<_>>();
                let resources = catalog
                    .resources
                    .iter()
                    .filter(|r| settings.allowlist.resources.contains(&r.uri))
                    .map(|r| p::ResourceRef(r.uri.clone()))
                    .collect::<Vec<_>>();
                if !completed || published || v.tools != tools || v.resources != resources {
                    return Err(invalid());
                }
                published = true;
            }
            p::EventPayload::ActionDenied(v) if v.intent_id == job.action_ref => {
                return Err(invalid())
            }
            p::EventPayload::ActionFailed(v) if v.intent_id == job.action_ref => {
                return Err(invalid())
            }
            p::EventPayload::ActionCancelled(v) if v.intent_id == job.action_ref => {
                return Err(invalid())
            }
            p::EventPayload::ActionOutcomeUnknown(v)
                if v.intent_id == job.action_ref
                    && (!started
                        || completed
                        || !checks.retained_dispatch
                        || v.remote_lease.is_some()
                        || (v.probe_hint.0 != format!("worker:{}", job.job_id)
                            && v.probe_hint.0
                                != "manual adjudication required; action was not retried")) =>
            {
                return Err(invalid());
            }
            _ => (),
        }
    }
    if !completed || (checks.published && !published) || last != head {
        return Err(invalid());
    }
    Ok(())
}
