use forme_memory::*;
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore, StoreOptions};
use std::sync::Arc;

fn trusted() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}
fn scope() -> MemoryScope {
    MemoryScope {
        schema_version: p::SchemaVersion(1),
        slice: TimeSlice::LongTerm,
        workspace: Some(p::WorkspaceRef("project:a".into())),
        scope: p::Scope("project:a".into()),
    }
}
fn node(id: &str) -> MemoryNode {
    MemoryNode {
        schema_version: p::SchemaVersion(1),
        id: p::NodeId(id.into()),
        kind: NodeKind::Fact,
        content_ref: p::ContentRef(format!("content:{id}")),
        tier: p::StabilityTier::Working,
        confidence: p::Confidence(0.7),
        scope: scope(),
        resting_activation: p::RestingActivation(0.0),
        recency: 1000,
        provenance: trusted(),
    }
}
fn fixture() -> (Arc<SqliteEventStore>, EventSourcedMemory<SqliteEventStore>) {
    let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
    let memory =
        EventSourcedMemory::with_clock(store.clone(), p::RunId("graph".into()), || 1000).unwrap();
    for id in ["a", "b", "c", "d"] {
        memory.add_node(node(id)).unwrap();
    }
    for (id, from, to, kind) in [
        ("ac", "a", "c", EdgeKind::Association),
        ("bc", "b", "c", EdgeKind::Temporal),
        ("cd", "c", "d", EdgeKind::Contradicts),
        ("da", "d", "a", EdgeKind::Association),
    ] {
        memory
            .add_edge(MemoryEdge {
                schema_version: p::SchemaVersion(1),
                id: p::EdgeId(id.into()),
                from: p::NodeId(from.into()),
                to: p::NodeId(to.into()),
                kind,
                weight: p::Weight(0.8),
                provenance: trusted(),
            })
            .unwrap();
    }
    (store, memory)
}
fn set(
    memory: &EventSourcedMemory<SqliteEventStore>,
    command: &str,
    deltas: Vec<p::MemoryMaintenanceDelta>,
) -> p::MemoryMaintenanceDeltaSet {
    p::MemoryMaintenanceDeltaSet {
        schema_version: p::SchemaVersion(1),
        command: p::IdempotencyKey(command.into()),
        expected_version: memory.graph_snapshot_at(None).unwrap().version,
        scope: scope().scope,
        at: 1000,
        deltas,
    }
}

#[test]
fn c10_five_shapes_converge_decay_and_obey_all_activation_bounds() {
    let (_, memory) = fixture();
    let graph = memory.graph_snapshot_at(None).unwrap();
    let config = ActivationConfig {
        surface_threshold: 0.0,
        ..Default::default()
    };
    for shape in [
        ActivationShape::Association,
        ActivationShape::Change,
        ActivationShape::Pressure,
        ActivationShape::Gap,
        ActivationShape::Tension,
    ] {
        let output = graph
            .activate(
                &[p::NodeId("a".into()), p::NodeId("b".into())],
                shape,
                &scope(),
                &config,
                1000,
            )
            .unwrap();
        assert_eq!(
            output,
            graph
                .activate(
                    &[p::NodeId("b".into()), p::NodeId("a".into())],
                    shape,
                    &scope(),
                    &config,
                    1000
                )
                .unwrap()
        );
        assert!(output.considered <= config.per_tick_nodes);
        assert!(output.hops <= config.max_hops);
        let tight = ActivationConfig {
            max_hops: 0,
            top_k_frontier: 1,
            per_tick_nodes: 1,
            ..config.clone()
        };
        let bounded = graph
            .activate(
                &[p::NodeId("a".into()), p::NodeId("b".into())],
                shape,
                &scope(),
                &tight,
                1000,
            )
            .unwrap();
        assert_eq!(bounded.considered, 1);
        assert_eq!(bounded.surfaced.len(), 1);
        assert!(bounded.truncated);
        let high = ActivationConfig {
            surface_threshold: 1.0,
            ..config.clone()
        };
        assert!(graph
            .activate(&[p::NodeId("a".into())], shape, &scope(), &high, 1000)
            .unwrap()
            .surfaced
            .is_empty());
    }
    let single = graph
        .activate(
            &[p::NodeId("a".into())],
            ActivationShape::Association,
            &scope(),
            &config,
            1000,
        )
        .unwrap();
    let converged = graph
        .activate(
            &[p::NodeId("a".into()), p::NodeId("b".into())],
            ActivationShape::Association,
            &scope(),
            &config,
            1000,
        )
        .unwrap();
    let level = |run: &ActivationRun, id: &str| {
        run.surfaced
            .iter()
            .find(|a| a.node.0 == id)
            .unwrap()
            .level
            .0
    };
    assert!(level(&converged, "c") > level(&single, "c"));
    assert!(level(&single, "d") < level(&single, "c"));
    let old = graph
        .activate(
            &[p::NodeId("a".into())],
            ActivationShape::Association,
            &scope(),
            &config,
            31 * 86_400_000,
        )
        .unwrap();
    assert!(level(&old, "a") < level(&single, "a"));
    let mut modified = graph.clone();
    modified
        .nodes
        .get_mut(&p::NodeId("a".into()))
        .unwrap()
        .resting_activation = p::RestingActivation(1.0);
    assert!(
        level(
            &modified
                .activate(
                    &[p::NodeId("a".into())],
                    ActivationShape::Association,
                    &scope(),
                    &config,
                    1000
                )
                .unwrap(),
            "a"
        ) > level(&single, "a")
    );
    modified.nodes.get_mut(&p::NodeId("a".into())).unwrap().tier = p::StabilityTier::Ephemeral;
    assert_ne!(
        modified
            .activate(
                &[p::NodeId("a".into())],
                ActivationShape::Association,
                &scope(),
                &config,
                1000
            )
            .unwrap(),
        single
    );
}

#[test]
fn c10_no_seed_has_no_snapshot_and_untrusted_or_out_of_scope_cannot_spread() {
    let (_, memory) = fixture();
    let config = ActivationConfig::default();
    let empty = memory
        .activate_at(&[], ActivationShape::Gap, &scope(), &config, 1000, None)
        .unwrap();
    assert_eq!(empty.snapshot, None);
    assert_eq!(empty.considered, 0);
    assert!(empty.surfaced.is_empty());
    let mut graph = memory.graph_snapshot_at(None).unwrap();
    graph
        .nodes
        .get_mut(&p::NodeId("a".into()))
        .unwrap()
        .provenance
        .trust_tier = p::TrustTier::Untrusted;
    assert!(graph
        .activate(
            &[p::NodeId("a".into())],
            ActivationShape::Association,
            &scope(),
            &config,
            1000
        )
        .unwrap()
        .surfaced
        .is_empty());
    let mut other = scope();
    other.workspace = Some(p::WorkspaceRef("other".into()));
    assert!(graph
        .activate(
            &[p::NodeId("b".into())],
            ActivationShape::Association,
            &other,
            &config,
            1000
        )
        .unwrap()
        .surfaced
        .is_empty());
    assert!(ActivationConfig {
        max_hops: 9,
        ..config
    }
    .validate()
    .is_err());
}

#[test]
fn c10_typed_maintenance_cas_replays_every_delta_and_rejects_poison() {
    let (store, memory) = fixture();
    let head = store.run_head(memory.aggregate_run()).unwrap();
    let decay = set(
        &memory,
        "decay",
        vec![p::MemoryMaintenanceDelta::EdgeWeight {
            edge: p::EdgeId("ac".into()),
            expected: p::Weight(0.8),
            next: p::Weight(0.4),
        }],
    );
    let id = memory.apply_maintenance(decay.clone(), trusted()).unwrap();
    assert_eq!(
        id,
        memory.apply_maintenance(decay.clone(), trusted()).unwrap()
    );
    assert_eq!(store.run_head(memory.aggregate_run()).unwrap(), head + 1);
    let mut stale = decay.clone();
    stale.command = p::IdempotencyKey("stale".into());
    assert!(memory.apply_maintenance(stale, trusted()).is_err());
    let increase = set(
        &memory,
        "increase",
        vec![p::MemoryMaintenanceDelta::RestingActivation {
            node: p::NodeId("a".into()),
            expected: p::RestingActivation(0.0),
            next: p::RestingActivation(0.4),
        }],
    );
    assert!(memory
        .apply_maintenance(increase.clone(), trusted())
        .is_err());
    let owner = p::Provenance {
        source: p::Source::OwnerControl,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    };
    memory.apply_maintenance(increase, owner.clone()).unwrap();
    memory
        .apply_maintenance(
            set(
                &memory,
                "consolidate",
                vec![p::MemoryMaintenanceDelta::Consolidate {
                    node: p::NodeId("a".into()),
                    expected: p::ContentRef("content:a".into()),
                    next: p::ContentRef("content:a-summary".into()),
                    evidence: vec![p::EvidenceRef("owner:evidence".into())],
                }],
            ),
            owner,
        )
        .unwrap();
    let changes = set(
        &memory,
        "prune",
        vec![
            p::MemoryMaintenanceDelta::ConfidenceDecay {
                node: p::NodeId("b".into()),
                expected: p::Confidence(0.7),
                next: p::Confidence(0.5),
            },
            p::MemoryMaintenanceDelta::PruneEdge {
                edge: p::EdgeId("bc".into()),
                expected: p::Weight(0.8),
            },
            p::MemoryMaintenanceDelta::PruneNode {
                node: p::NodeId("d".into()),
                expected: p::ContentRef("content:d".into()),
            },
        ],
    );
    memory.apply_maintenance(changes, trusted()).unwrap();
    let before = memory.graph_snapshot_at(None).unwrap();
    let replay = EventSourcedMemory::open(store.clone(), memory.aggregate_run().clone()).unwrap();
    assert_eq!(before, replay.graph_snapshot_at(None).unwrap());
    assert_eq!(
        memory.query(GraphQuery::default()),
        replay.query(GraphQuery::default())
    );
    assert_eq!(before.edges[&p::EdgeId("ac".into())].weight, p::Weight(0.4));
    assert!(!before.nodes.contains_key(&p::NodeId("d".into())));
    let poison = set(
        &memory,
        "poison",
        vec![p::MemoryMaintenanceDelta::EdgeWeight {
            edge: p::EdgeId("ac".into()),
            expected: p::Weight(0.4),
            next: p::Weight(0.9),
        }],
    );
    let mut untrusted = trusted();
    untrusted.trust_tier = p::TrustTier::Untrusted;
    let bad = p::Event::new(
        p::EventId("poison".into()),
        memory.aggregate_run().clone(),
        None,
        p::EventPayload::MemoryMaintenanceApplied(p::MemoryMaintenanceAppliedPayload {
            deltas_ref: poison.into_ref().unwrap(),
        }),
        p::SchemaVersion(1),
        1000,
        untrusted,
    );
    let count = store.run_head(memory.aggregate_run()).unwrap();
    assert!(store.append(bad).is_err());
    assert_eq!(store.run_head(memory.aggregate_run()).unwrap(), count);
    let mut tamper = poison.into_ref().unwrap();
    tamper.0 = tamper.0.replace("\"next\":0.9", "\"next\":0.8");
    assert!(p::MemoryMaintenanceDeltaSet::from_ref(&tamper).is_err());
}
