//! Owner views and control adapters on the one production brain.
use crate::{now_ms, ProductionRuntime};
use forme_protocol as p;
use forme_store::EventStore;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

mod material;
mod projections;
mod projects;
mod proposal;
mod review;

impl ProductionRuntime {
    pub fn workbench_snapshot(
        &self,
        owner: &p::VerifiedPrincipal,
        query: p::WorkbenchQuery,
    ) -> p::Result<p::WorkbenchSnapshot> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        let (items, digest) = self.workbench_items(&query)?;
        let total = items.len();
        let end = query.offset.saturating_add(query.limit).min(total);
        Ok(p::WorkbenchSnapshot {
            schema_version: p::SchemaVersion(1),
            section: query.section,
            digest,
            total,
            offset: query.offset,
            next_offset: (end < total).then_some(end),
            items: items
                .into_iter()
                .skip(query.offset)
                .take(query.limit)
                .collect(),
        })
    }

    // Callers own the data fence and authenticated owner boundary. Keep a
    // single projection/digest implementation for list and material queries.
    fn workbench_items(
        &self,
        query: &p::WorkbenchQuery,
    ) -> p::Result<(Vec<p::WorkbenchItem>, p::SchemaDigest)> {
        query.validate()?;
        let services = self.services();
        let heads = services.store.run_heads()?;
        // Disposable caches may lag another governed producer. Refresh them
        // without writing, and reject a concurrent change instead of combining
        // two revisions into a purported snapshot.
        services.memory.refresh_projection()?;
        services.cognition.refresh_projection()?;
        let events = self.owner_brain_events_at(None)?;
        let mut items = projections::project(self, &events, query.section)?;
        if services.store.run_heads()? != heads {
            return Err(p::Error(
                "workbench snapshot changed; restart pagination".into(),
            ));
        }
        items.retain(|item| {
            query.scope.as_ref().is_none_or(|scope| {
                item.scope.as_ref().is_some_and(|item_scope| {
                    item_scope == scope || item_scope.0.starts_with(&format!("{}/", scope.0))
                })
            })
        });
        items.sort_by(|a, b| (&a.run, &a.id).cmp(&(&b.run, &b.id)));
        let digest = p::canonical_digest(&(query.section, &query.scope, &items))?;
        if query
            .snapshot
            .as_ref()
            .is_some_and(|bound| bound != &digest)
        {
            return Err(p::Error(
                "workbench snapshot changed; restart pagination".into(),
            ));
        }
        Ok((items, digest))
    }
}

fn item(event: &p::Event, id: String, kind: p::WorkbenchItemKind) -> p::WorkbenchItem {
    p::WorkbenchItem {
        schema_version: p::SchemaVersion(1),
        id,
        kind,
        run: event.run_id.clone(),
        event: event.event_id.clone(),
        stream_seq: event.stream_seq,
        at: event.ts_unix_ms,
        scope: None,
        state: "Recorded".into(),
        summary: String::new(),
        content: None,
        tier: None,
        confidence: None,
        evidence: Vec::new(),
        evidence_count: 0,
        derived: Vec::new(),
        derived_count: 0,
        reviews: Vec::new(),
        learning: None,
    }
}

fn lineage(events: &[p::Event], source: &p::ObjectRef) -> Vec<p::ObjectRef> {
    let mut edges = BTreeMap::<p::ObjectRef, BTreeSet<p::ObjectRef>>::new();
    for event in events {
        if let p::EventPayload::MemoryMaintenanceApplied(payload) = &event.payload {
            if let Some(value) = payload.deltas_ref.0.strip_prefix("cognition-lineage|") {
                if let Some((from, to)) = value.split_once('|') {
                    let decode = |v: &str| p::ObjectRef(v.replace("%7C", "|").replace("%25", "%"));
                    edges.entry(decode(from)).or_default().insert(decode(to));
                }
            }
        }
    }
    let mut visited = BTreeSet::from([source.clone()]);
    let mut queue = VecDeque::from([source.clone()]);
    while let Some(current) = queue.pop_front() {
        for next in edges.get(&current).into_iter().flatten() {
            if visited.insert(next.clone()) {
                queue.push_back(next.clone());
            }
        }
    }
    // The root is included: even a leaf needs its own re-evaluation task.
    visited.into_iter().collect()
}

fn references(
    row: &mut p::WorkbenchItem,
    evidence: Vec<p::EvidenceRef>,
    derived: Vec<p::ObjectRef>,
) {
    row.evidence_count = evidence.len();
    row.evidence = evidence.into_iter().take(128).collect();
    row.derived_count = derived.len();
    row.derived = derived.into_iter().take(128).collect();
}
