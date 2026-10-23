//! Material disclosure is bound to the exact current, owner-visible memory row.
use super::*;

impl ProductionRuntime {
    pub fn workbench_material(
        &self,
        owner: &p::VerifiedPrincipal,
        query: p::WorkbenchMaterialQuery,
    ) -> p::Result<p::WorkbenchMaterialView> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        query.validate()?;
        let store = self.services().store.clone();
        let heads = store.run_heads()?;
        let (items, _) = self.workbench_items(&query.memory_query())?;
        let row = items
            .into_iter()
            .find(|row| row.id == query.node.0 && row.run == query.run && row.event == query.event)
            .ok_or_else(|| p::Error("memory material is not in the requested snapshot".into()))?;
        let scope = row
            .scope
            .ok_or_else(|| p::Error("memory material scope is unavailable".into()))?;
        let content_ref = row
            .content
            .ok_or_else(|| p::Error("memory material reference is unavailable".into()))?;
        let source = store
            .read_run(query.run.clone())
            .after(row.stream_seq.saturating_sub(1))
            .next()
            .transpose()?;
        let bound = source.is_some_and(|event| {
            event.event_id == query.event
                && matches!(event.payload, p::EventPayload::MemoryNodeAppended(node)
                    if node.node_id == query.node && node.content_ref == content_ref && node.scope == scope)
        });
        if !bound {
            return Err(p::Error(
                "memory material source no longer matches its row".into(),
            ));
        }
        let material =
            crate::production_artifact_store::read_material(&store, &content_ref, &scope)?;
        if store.run_heads()? != heads {
            return Err(p::Error(
                "workbench snapshot changed; restart pagination".into(),
            ));
        }
        Ok(p::WorkbenchMaterialView {
            schema_version: p::SchemaVersion(1),
            node: query.node,
            run: query.run,
            event: query.event,
            scope,
            content_ref,
            state: material.state,
            bytes: material.bytes,
            content_digest: material.digest,
            text: material.text,
        })
    }
}
