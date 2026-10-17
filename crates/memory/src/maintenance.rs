use crate::*;

impl<S: EventStore> EventSourcedMemory<S> {
    /// Computes on the fixed graph. Non-graph appends may advance the CAS
    /// head, but late graph knowledge must wait for a new window.
    pub fn maintain_low_value_branches(
        &self,
        snapshot: &MemoryGraphSnapshot,
        at: p::Timestamp,
        age_ms: i64,
        command: p::IdempotencyKey,
    ) -> p::Result<Option<p::EventId>> {
        if age_ms <= 0 || at < 0 {
            return Err(p::Error("invalid memory maintenance time bounds".into()));
        }
        for event in self.store.read_run(self.aggregate_run.clone()) {
            let event = event?;
            if let p::EventPayload::MemoryMaintenanceApplied(payload) = &event.payload {
                if let Some(set) = p::MemoryMaintenanceDeltaSet::from_ref(&payload.deltas_ref)? {
                    if set.command == command {
                        if set.at != at {
                            return Err(p::Error(
                                "maintenance command changed its judgment time".into(),
                            ));
                        }
                        return Ok(Some(event.event_id));
                    }
                }
            }
        }
        let low = snapshot
            .nodes
            .values()
            .filter(|node| {
                matches!(
                    node.tier,
                    p::StabilityTier::Working
                        | p::StabilityTier::Session
                        | p::StabilityTier::Ephemeral
                ) && !matches!(node.kind, NodeKind::Goal | NodeKind::CognitiveObjectRef)
                    && node.recency.saturating_add(age_ms) <= at
                    && node.confidence.0 <= 0.25
                    && node.resting_activation.0 <= 0.1
            })
            .map(|node| node.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut deltas = Vec::new();
        for id in &low {
            if deltas.len() >= p::MAX_MEMORY_DELTAS / 2 {
                break;
            }
            let node = &snapshot.nodes[id];
            if node.confidence.0 <= 0.1 {
                deltas.push(p::MemoryMaintenanceDelta::PruneNode {
                    node: id.clone(),
                    expected: node.content_ref.clone(),
                });
            } else {
                deltas.push(p::MemoryMaintenanceDelta::ConfidenceDecay {
                    node: id.clone(),
                    expected: node.confidence,
                    next: p::Confidence(node.confidence.0 * 0.5),
                });
                if node.resting_activation.0 > 0.0 {
                    deltas.push(p::MemoryMaintenanceDelta::RestingActivation {
                        node: id.clone(),
                        expected: node.resting_activation,
                        next: p::RestingActivation(node.resting_activation.0 * 0.5),
                    });
                }
            }
        }
        let pruned = deltas
            .iter()
            .filter_map(|delta| {
                if let p::MemoryMaintenanceDelta::PruneNode { node, .. } = delta {
                    Some(node.clone())
                } else {
                    None
                }
            })
            .collect::<std::collections::BTreeSet<_>>();
        for edge in snapshot.edges.values() {
            if deltas.len() == p::MAX_MEMORY_DELTAS {
                break;
            }
            if pruned.contains(&edge.from) || pruned.contains(&edge.to) {
                continue;
            }
            if low.contains(&edge.from) || low.contains(&edge.to) {
                deltas.push(if edge.weight.0 <= 0.05 {
                    p::MemoryMaintenanceDelta::PruneEdge {
                        edge: edge.id.clone(),
                        expected: edge.weight,
                    }
                } else {
                    p::MemoryMaintenanceDelta::EdgeWeight {
                        edge: edge.id.clone(),
                        expected: edge.weight,
                        next: p::Weight(edge.weight.0 * 0.5),
                    }
                });
            }
        }
        if deltas.is_empty() {
            return Ok(None);
        }
        let current = self.graph_snapshot_at(None)?;
        if current.nodes != snapshot.nodes || current.edges != snapshot.edges {
            return Ok(None);
        }
        self.apply_maintenance(
            p::MemoryMaintenanceDeltaSet {
                schema_version: p::SchemaVersion(1),
                command,
                expected_version: current.version,
                scope: p::Scope("*".into()),
                at,
                deltas,
            },
            system_provenance(),
        )
        .map(Some)
    }
}
