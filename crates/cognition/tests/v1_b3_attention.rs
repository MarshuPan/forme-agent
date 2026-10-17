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

#[test]
fn c9_all_six_item_kinds_remove_terminal_work_and_exclude_future_intentions() {
    let template = goal("run:a", "current", "workspace:a", 3);
    let fact = |seq, payload: p::EventPayload| {
        let mut event = template.clone();
        event.stream_seq = seq;
        event.event_id = p::EventId(format!("a:{seq}"));
        event.kind = payload.kind();
        event.payload = payload;
        event
    };
    let mut charter = p::WorkspaceCharterRecord {
        schema_version: p::SchemaVersion(1),
        workspace: p::WorkspaceRef("workspace:a".into()),
        version: 1,
        goals: vec![],
        constraints: vec![],
        prohibitions: vec![],
        done_contract: None,
        review_cadence: Some(p::DurationMs(100)),
        actor: p::Actor::Owner,
        digest: p::SchemaDigest(String::new()),
    };
    charter.refresh_digest().unwrap();
    let mut events = vec![
        fact(
            1,
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: p::Source::UserTurn,
                session_ref: p::SessionId("s".into()),
                input_ref: p::InputRef("input".into()),
                idempotency_key: None,
            }),
        ),
        fact(
            2,
            p::EventPayload::SessionBound(p::SessionBoundPayload {
                policy_profile: p::PolicyProfileRef("policy".into()),
                model_profile: p::ModelProfileRef("model".into()),
                toolset_ref: p::ToolsetRef("tools".into()),
                workspace: p::WorkspaceRef("workspace:a".into()),
                effect_mode: None,
                evolution_snapshot: None,
                federation_snapshot: None,
                composition_manifest: None,
            }),
        ),
        template.clone(),
        fact(
            4,
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: p::CandidateId("pending".into()),
                target: p::CandidateTargetRef("target".into()),
                evidence_refs: vec![],
                confidence: p::Confidence(0.3),
                provenance: goal("x", "x", "x", 1).provenance,
                target_tier: p::StabilityTier::Working,
                capability_update: None,
                strategy_candidate: None,
            }),
        ),
        fact(
            5,
            p::EventPayload::ObservationRecorded(p::ObservationRecordedPayload {
                source: p::Source::Internal,
                scope: p::Scope("workspace:a".into()),
                grant_ref: None,
                learning_reply: None,
            }),
        ),
        fact(
            6,
            p::EventPayload::ImpulseRaised(p::ImpulseRaisedPayload {
                source: p::ImpulseSource::Association,
                reach: p::Reach(1),
                seed: vec![],
            }),
        ),
        fact(
            7,
            p::EventPayload::ProspectiveIntentionCreated(p::ProspectiveIntentionCreatedPayload {
                intention_id: p::IntentionId("due".into()),
                source: p::IntentionSource::Commitment,
                trigger: p::IntentionTriggerRef("at:900|seed=due".into()),
                schedule: None,
                goal_frame: None,
            }),
        ),
        fact(
            8,
            p::EventPayload::ProspectiveIntentionCreated(p::ProspectiveIntentionCreatedPayload {
                intention_id: p::IntentionId("future".into()),
                source: p::IntentionSource::Commitment,
                trigger: p::IntentionTriggerRef("at:1100|seed=future".into()),
                schedule: None,
                goal_frame: None,
            }),
        ),
        fact(
            9,
            p::EventPayload::WorkspaceCharterChanged(p::WorkspaceCharterChangedPayload {
                charter,
                expected_version: 0,
                committed_version: 1,
            }),
        ),
    ];
    let panel =
        AgentWorkspaceProjection::rebuild_scoped(&events, &query(Some("workspace:a"), 64)).unwrap();
    let kinds = panel
        .items
        .iter()
        .map(|item| format!("{:?}", item.kind))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(kinds.len(), 6);
    assert!(!panel.items.iter().any(|item| item.id.contains("future")));
    events.push(fact(
        10,
        p::EventPayload::RunComplete(p::RunCompletePayload {
            result_ref: None,
            stop_reason: p::StopReason("completed".into()),
        }),
    ));
    events.push(fact(
        11,
        p::EventPayload::ProspectiveIntentionResolved(p::ProspectiveIntentionResolvedPayload {
            intention_id: p::IntentionId("due".into()),
            outcome: p::IntentionOutcome::Done,
        }),
    ));
    let closed =
        AgentWorkspaceProjection::rebuild_scoped(&events, &query(Some("workspace:a"), 64)).unwrap();
    assert!(closed.items.iter().all(|item| !matches!(
        item.kind,
        AgentWorkspaceItemKind::Goal
            | AgentWorkspaceItemKind::Run
            | AgentWorkspaceItemKind::Intention
    )));
}
