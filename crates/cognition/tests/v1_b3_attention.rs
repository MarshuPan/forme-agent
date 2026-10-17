use forme_cognition::*;
use forme_protocol as p;

fn goal(run: &str, id: &str, scope: &str, seq: u64) -> p::Event {
    let mut event = p::Event::new(
        p::EventId(format!("event:{id}")),
        p::RunId(run.into()),
        None,
        p::EventPayload::GoalFramed(p::GoalFramedPayload {
            goal_frame: p::GoalFrameRef(id.into()),
            long_term: None,
            knowledge: Some(p::GoalKnowledge {
                schema_version: p::SchemaVersion(1),
                scope: p::Scope(scope.into()),
                allow_questions: false,
                requirements: vec![],
            }),
        }),
        p::SchemaVersion(1),
        100,
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: None,
        },
    );
    event.stream_seq = seq;
    event
}
fn query(scope: Option<&str>, capacity: usize) -> AgentWorkspaceQuery {
    AgentWorkspaceQuery {
        schema_version: p::SchemaVersion(1),
        scope: scope.map(|s| p::Scope(s.into())),
        capacity,
        now: 1000,
        config_version: p::Version(1),
    }
}

#[test]
fn c9_owner_projection_filters_before_capacity_and_is_order_independent() {
    let events = vec![
        goal("run:a", "a", "workspace:a", 1),
        goal("run:b", "b", "workspace:b", 9),
        goal("run:c", "c", "workspace:a", 1),
    ];
    let all = AgentWorkspaceProjection::rebuild_scoped(&events, &query(None, 2)).unwrap();
    assert_eq!(
        all.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        vec!["goal:b", "goal:a"]
    );
    let scoped =
        AgentWorkspaceProjection::rebuild_scoped(&events, &query(Some("workspace:a"), 2)).unwrap();
    assert_eq!(scoped.items.len(), 2);
    assert!(scoped.items.iter().all(|i| i.scope.0 == "workspace:a"));
    let mut reversed = events.clone();
    reversed.reverse();
    assert_eq!(
        scoped,
        AgentWorkspaceProjection::rebuild_scoped(&reversed, &query(Some("workspace:a"), 2))
            .unwrap()
    );
    let other =
        AgentWorkspaceProjection::rebuild_scoped(&events, &query(Some("workspace:a"), 1)).unwrap();
    assert_ne!(other.snapshot_ref, scoped.snapshot_ref);
    assert!(AgentWorkspaceProjection::rebuild_scoped(&events, &query(None, 65)).is_err());
    assert!(
        AgentWorkspaceProjection::rebuild_scoped(&events, &query(Some("workspace:ab"), 4))
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn c9_attention_digest_binds_full_cursor_time_config_and_rejects_conflicting_positions() {
    let mut events = vec![goal("run:a", "a", "workspace:a", 1)];
    let base =
        AgentWorkspaceProjection::rebuild_scoped(&events, &query(Some("workspace:a"), 1)).unwrap();
    events.push(goal("run:out", "out", "workspace:b", 1));
    let advanced =
        AgentWorkspaceProjection::rebuild_scoped(&events, &query(Some("workspace:a"), 1)).unwrap();
    assert_eq!(base.items, advanced.items);
    assert_ne!(base.snapshot_ref, advanced.snapshot_ref);
    let mut config = query(Some("workspace:a"), 1);
    config.config_version = p::Version(2);
    assert_ne!(
        advanced.snapshot_ref,
        AgentWorkspaceProjection::rebuild_scoped(&events, &config)
            .unwrap()
            .snapshot_ref
    );
    config.config_version = p::Version(1);
    config.now += 1;
    assert_ne!(
        advanced.snapshot_ref,
        AgentWorkspaceProjection::rebuild_scoped(&events, &config)
            .unwrap()
            .snapshot_ref
    );
    events.push(goal("run:a", "conflict", "workspace:a", 1));
    assert!(AgentWorkspaceProjection::rebuild_scoped(&events, &config).is_err());
}
