//! Publish only a completed original; retries never call a worker or a model.
use super::*;
use forme_execution::installed::ManagedMcpSourceVerifier;
use forme_store::{EventStore, VersionedEventStore};

pub(crate) fn publish(
    store: &SqliteEventStore,
    owner: &p::VerifiedPrincipal,
    authority: &dyn ManagedMcpSourceVerifier,
    run: &p::RunId,
    intent: &p::ActionIntent,
) -> p::Result<()> {
    store.require_current_execution(run)?;
    let version = store.aggregate_version(run.clone())?;
    let events = store.read_run(run.clone()).collect::<p::Result<Vec<_>>>()?;
    let completed = events
        .iter()
        .filter_map(|e| match &e.payload {
            p::EventPayload::ActionCompleted(v) if v.intent_id == intent.intent_id => Some(v),
            _ => None,
        })
        .collect::<Vec<_>>();
    if completed.len() != 1 {
        return Err(invalid());
    }
    let id = completed[0]
        .receipt
        .as_ref()
        .and_then(|v| v.content_ref.as_ref())
        .ok_or_else(invalid)?;
    let (original, bytes) = read_material_original(store, id, owner)?.ok_or_else(invalid)?;
    if original.request.execution.as_ref().map(|v| &v.intent) != Some(intent) {
        return Err(invalid());
    }
    let catalog = validate_managed_mcp_material(
        &original.request,
        &original.observation,
        &original.manifest,
        &bytes,
    )?;
    let settings = authority.verify_source(&original.manifest)?;
    catalog.spec.validate_settings(&settings, &intent.scope)?;
    source::verify_run(
        run,
        version.value,
        &events,
        &original,
        &settings,
        &catalog,
        false,
    )?;
    if events.iter().any(|e| matches!(&e.payload, p::EventPayload::McpDiscovered(v) if v.server == catalog.spec.server)) {
        return Ok(());
    }
    let payload = p::EventPayload::McpDiscovered(p::McpDiscoveredPayload {
        server: catalog.spec.server,
        tools: catalog
            .tools
            .iter()
            .filter(|t| settings.allowlist.tools.contains(&t.name))
            .map(|t| p::ToolRef(t.name.clone()))
            .collect(),
        resources: catalog
            .resources
            .iter()
            .filter(|r| settings.allowlist.resources.contains(&r.uri))
            .map(|r| p::ResourceRef(r.uri.clone()))
            .collect(),
    });
    // Recheck after the Host round trip, including current lifecycle presence.
    if authority.verify_source(&original.manifest)? != settings
        || read_material_original(store, id, owner)?
            .map(|v| v.0)
            .as_ref()
            != Some(&original)
    {
        return Err(invalid());
    }
    let event = p::Event::new(
        p::EventId(format!(
            "event:mcp-publication:{}",
            p::canonical_digest(&(run, &original.manifest.manifest_digest))?.0
        )),
        run.clone(),
        None,
        payload,
        p::SchemaVersion(1),
        crate::now_ms(),
        crate::internal_provenance(p::Source::OwnerControl),
    );
    match store.append_expected(event, version)?.status {
        p::ExpectedAppendStatus::Applied | p::ExpectedAppendStatus::Duplicate => Ok(()),
        p::ExpectedAppendStatus::Conflict => Err(invalid()),
    }
}
