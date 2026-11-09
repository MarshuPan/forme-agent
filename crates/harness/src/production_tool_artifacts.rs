//! Read-only tool output projection from original run facts and protected material.
use crate::{
    production_artifact_store::{verified_worker_material, verified_worker_metadata},
    ProductionRuntime,
};
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Action {
    plan: Option<p::ActionPlannedPayload>,
    started: bool,
    completed: Option<p::ExternalActionReceipt>,
}
struct OriginalRun {
    scope: p::Scope,
    head: u64,
    actions: BTreeMap<p::ActionId, Action>,
}
#[cfg(test)]
#[path = "production_tool_artifacts_tests.rs"]
mod tests;
impl ProductionRuntime {
    pub fn query_tool_artifacts(
        &self,
        owner: &p::VerifiedPrincipal,
        query: p::ToolArtifactCatalogQuery,
    ) -> p::Result<p::ToolArtifactCatalogView> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        query_original(&self.services().store, owner, query)
    }
    pub fn read_tool_artifact(
        &self,
        owner: &p::VerifiedPrincipal,
        request: p::ToolArtifactRead,
    ) -> p::Result<p::ToolArtifactChunk> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        read_original(&self.services().store, owner, request)
    }
}
fn query_original(
    store: &SqliteEventStore,
    owner: &p::VerifiedPrincipal,
    query: p::ToolArtifactCatalogQuery,
) -> p::Result<p::ToolArtifactCatalogView> {
    query.validate()?;
    let original = original_run(store, &query.session, &query.run)?;
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();
    for (id, action) in &original.actions {
        if let Some(entry) = entry(store, owner, &original.scope, id, action)? {
            if !ids.insert(entry.content_ref.clone()) {
                return Err(invalid());
            }
            entries.push(entry);
        }
    }
    check_head(store, &query.run, original.head)?;
    let digest = p::canonical_digest(&(
        "forme.tool-artifact-catalog.v2",
        owner,
        &query.session,
        &query.run,
        &original.scope,
        original.head,
        &entries,
    ))?;
    if query
        .page
        .snapshot
        .as_ref()
        .is_some_and(|expected| *expected != digest)
    {
        return Err(conflict());
    }
    let total = entries.len();
    let end = query
        .page
        .offset
        .saturating_add(query.page.limit)
        .min(total);
    let view = p::ToolArtifactCatalogView {
        schema_version: p::TOOL_ARTIFACT_SCHEMA_VERSION,
        session: query.session.clone(),
        run: query.run.clone(),
        digest,
        total,
        next_offset: (end < total).then_some(end),
        artifacts: entries
            .into_iter()
            .skip(query.page.offset)
            .take(query.page.limit)
            .collect(),
    };
    view.validate_for(&query)?;
    Ok(view)
}
fn read_original(
    store: &SqliteEventStore,
    owner: &p::VerifiedPrincipal,
    request: p::ToolArtifactRead,
) -> p::Result<p::ToolArtifactChunk> {
    request.validate()?;
    let original = original_run(store, &request.session, &request.run)?;
    let mut selected = None;
    for (id, action) in &original.actions {
        if action
            .completed
            .as_ref()
            .and_then(|r| r.content_ref.as_ref())
            != Some(&request.content_ref)
        {
            continue;
        }
        let Some(value) = entry(store, owner, &original.scope, id, action)? else {
            return Err(invalid());
        };
        if selected.replace(value).is_some() {
            return Err(invalid());
        }
    }
    let selected = selected.ok_or_else(invalid)?;
    if selected.content_digest != request.content_digest || request.offset >= selected.bytes {
        return Err(invalid());
    }
    let (material, record) =
        verified_worker_material(store, &request.content_ref, owner)?.ok_or_else(invalid)?;
    if material.descriptor.content_digest != selected.content_digest
        || material.descriptor.bytes != selected.bytes
    {
        return Err(invalid());
    }
    let protection = store.content_protection()?.ok_or_else(invalid)?;
    let bytes = protection
        .content_service()
        .decrypt(&record.envelope.binding(), &record.envelope)?;
    if bytes.len() as u64 != selected.bytes
        || p::sha256_content_digest(&bytes) != selected.content_digest
    {
        return Err(invalid());
    }
    // Every body disclosure revalidates the original encoded object; no
    // plaintext cache survives logout, deletion or a different owner.
    forme_execution::validate_desktop_png(&bytes, selected.width, selected.height)?;
    check_head(store, &request.run, original.head)?;
    let next = request
        .offset
        .saturating_add(u64::from(request.max_bytes))
        .min(selected.bytes);
    let chunk = p::ToolArtifactChunk {
        schema_version: p::TOOL_ARTIFACT_SCHEMA_VERSION,
        session: request.session.clone(),
        run: request.run.clone(),
        content_ref: request.content_ref.clone(),
        content_digest: request.content_digest.clone(),
        offset: request.offset,
        next_offset: next,
        total_bytes: selected.bytes,
        finished: next == selected.bytes,
        bytes: bytes[request.offset as usize..next as usize].to_vec(),
    };
    chunk.validate_for(&request)?;
    Ok(chunk)
}

fn original_run(
    store: &SqliteEventStore,
    session: &p::SessionId,
    run: &p::RunId,
) -> p::Result<OriginalRun> {
    let head = *store.run_heads()?.0.get(run).ok_or_else(invalid)?;
    let mut accepted = false;
    let mut scope = None;
    let mut last = 0;
    let mut actions: BTreeMap<p::ActionId, Action> = BTreeMap::new();
    for (index, event) in store.read_run(run.clone()).enumerate() {
        if index >= 100_000 {
            return Err(invalid());
        }
        let event = event?;
        if event.run_id != *run || event.stream_seq <= last || event.stream_seq > head {
            return Err(conflict());
        }
        last = event.stream_seq;
        match event.payload {
            p::EventPayload::RunAccepted(value) => {
                if accepted || scope.is_some() || value.session_ref != *session {
                    return Err(invalid());
                }
                accepted = true;
            }
            p::EventPayload::SessionBound(value) => {
                if !accepted || scope.is_some() {
                    return Err(invalid());
                }
                scope = Some(p::Scope(value.workspace.0));
            }
            p::EventPayload::ActionPlanned(value) => {
                let action = actions.entry(value.intent_id.clone()).or_default();
                if scope.is_none() || action.plan.as_ref().is_some_and(|old| old != &value) {
                    return Err(invalid());
                }
                action.plan = Some(value);
            }
            p::EventPayload::ActionStarted(value) => {
                let action = actions.entry(value.intent_id).or_default();
                let Some(plan) = &action.plan else {
                    continue;
                };
                if plan.backend != value.backend || plan.scope != value.scope || action.started {
                    return Err(invalid());
                }
                action.started = true;
            }
            p::EventPayload::ActionCompleted(value) => {
                let action = actions.entry(value.intent_id.clone()).or_default();
                if !action.started {
                    continue;
                }
                if let Some(receipt) = value.receipt {
                    if receipt.action != value.intent_id || action.completed.is_some() {
                        return Err(invalid());
                    }
                    action.completed = Some(receipt);
                }
            }
            p::EventPayload::ActionOutcomeUnknown(value) => {
                if let Some(action) = actions.get_mut(&value.intent_id) {
                    action.completed = None;
                }
            }
            p::EventPayload::ActionFailed(value) => {
                if let Some(action) = actions.get_mut(&value.intent_id) {
                    action.completed = None;
                }
            }
            p::EventPayload::ActionCancelled(value) => {
                if let Some(action) = actions.get_mut(&value.intent_id) {
                    action.completed = None;
                }
            }
            _ => (),
        }
        if actions.len() > 20_000 {
            return Err(invalid());
        }
    }
    if !accepted {
        return Err(invalid());
    }
    check_head(store, run, head)?;
    Ok(OriginalRun {
        scope: scope.ok_or_else(invalid)?,
        head,
        actions,
    })
}
fn entry(
    store: &SqliteEventStore,
    owner: &p::VerifiedPrincipal,
    scope: &p::Scope,
    id: &p::ActionId,
    action: &Action,
) -> p::Result<Option<p::ToolArtifactEntry>> {
    let (Some(plan), Some(receipt)) = (&action.plan, &action.completed) else {
        return Ok(None);
    };
    if !action.started
        || plan.backend != p::BackendKind::Computer
        || receipt.effect != p::EffectStatus::Observed
    {
        return Ok(None);
    }
    let Some(content) = &receipt.content_ref else {
        return Ok(None);
    };
    let Some(material) = verified_worker_metadata(store, content, owner, scope)? else {
        return Ok(None);
    };
    let binding = &material.manifest.binding;
    if plan.scope != *scope
        || binding.workspace_scope != *scope
        || binding.action_ref != *id
        || binding.plan_digest != plan.plan_digest
        || receipt.schema_version != p::SchemaVersion(1)
        || receipt.trust != p::TrustTier::Untrusted
        || receipt.content_digest.as_ref() != Some(&material.descriptor.content_digest)
        || receipt.probe_hint != Some(p::ProbeHintRef(format!("worker:{}", binding.job_id)))
    {
        return Err(invalid());
    }
    let value = p::ToolArtifactEntry {
        schema_version: p::TOOL_ARTIFACT_SCHEMA_VERSION,
        content_ref: content.clone(),
        content_digest: material.descriptor.content_digest,
        action: id.clone(),
        scope: scope.clone(),
        bytes: material.descriptor.bytes,
        media_type: material.descriptor.media_type,
        width: material.descriptor.width,
        height: material.descriptor.height,
    };
    value.validate()?;
    Ok(Some(value))
}
fn check_head(store: &SqliteEventStore, run: &p::RunId, head: u64) -> p::Result<()> {
    if store.run_heads()?.0.get(run) != Some(&head) {
        return Err(conflict());
    }
    Ok(())
}
fn invalid() -> p::Error {
    p::Error("tool artifact is unavailable for the original owner session and run".into())
}
fn conflict() -> p::Error {
    p::Error("tool artifact catalog snapshot conflict".into())
}
