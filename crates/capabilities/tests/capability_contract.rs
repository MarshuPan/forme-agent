use forme_capabilities::{
    Capability, CapabilityDescriptor, CapabilityEvidence, CapabilityRegistry, CapabilitySource,
    InMemoryCapabilityRegistry, InMemorySkillRegistry, LoadTrigger, SkillBody, SkillDefinition,
    SkillMetadata, SkillRegistry, SkillSearchQuery, ToolsetResolver,
};
use forme_protocol as p;

#[test]
fn unified_registry_excludes_unknown_risk_untrusted_disabled_and_policy_denied_items() {
    let registry = InMemoryCapabilityRegistry::default();
    registry
        .index(CapabilitySource {
            schema_version: p::SchemaVersion(1),
            source_ref: p::CapabilitySourceRef("builtin".into()),
            trust: p::TrustTier::ApprovedSource,
            entries: vec![
                descriptor("tool:visible", Some(p::Risk::Low), true),
                descriptor("tool:unknown-risk", None, true),
                descriptor("tool:disabled", Some(p::Risk::Low), false),
            ],
        })
        .unwrap();
    registry
        .index(CapabilitySource {
            schema_version: p::SchemaVersion(1),
            source_ref: p::CapabilitySourceRef("untrusted-plugin".into()),
            trust: p::TrustTier::Untrusted,
            entries: vec![descriptor("tool:untrusted", Some(p::Risk::Low), true)],
        })
        .unwrap();

    let context = resolve_context(vec![
        "tool:visible",
        "tool:unknown-risk",
        "tool:disabled",
        "tool:untrusted",
    ]);
    let (toolset, evidence) = ToolsetResolver::resolve(&registry, &context).unwrap();
    assert_eq!(toolset.items.len(), 1);
    assert_eq!(
        toolset.items[0].id(),
        p::CapabilityRef("tool:visible".into())
    );
    assert_eq!(evidence.len(), 1);
    assert_eq!(
        evidence[0].capability,
        p::CapabilityRef("tool:visible".into())
    );

    let denied = resolve_context(Vec::new());
    assert!(CapabilityRegistry::resolve_toolset(&registry, &denied)
        .unwrap()
        .items
        .is_empty());

    registry
        .record_evidence(CapabilityEvidence {
            schema_version: p::SchemaVersion(1),
            capability: p::CapabilityRef("tool:visible".into()),
            outcome: p::CapabilityOutcome("success".into()),
            reliability: p::Reliability("observed".into()),
        })
        .unwrap();
    let events = registry.take_events();
    assert!(events
        .iter()
        .any(|event| matches!(event, p::EventPayload::CapabilityIndexed(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, p::EventPayload::ToolsetResolved(_))));
    assert!(matches!(
        events.last(),
        Some(p::EventPayload::CapabilityEvidenceRecorded(_))
    ));
}

#[test]
fn s4_skill_metadata_is_default_and_only_selected_body_is_loaded() {
    let registry = InMemorySkillRegistry::new(vec![
        skill("skill:chosen", "chosen body", p::TrustTier::ApprovedSource),
        skill("skill:other", "other body", p::TrustTier::ApprovedSource),
        skill("skill:untrusted", "hidden body", p::TrustTier::Untrusted),
    ])
    .unwrap();

    let metadata = registry.metadata(p::Scope("workspace:alpha".into()));
    assert_eq!(metadata.len(), 2);
    assert!(metadata.iter().all(|item| !item.summary.contains("body")));
    let mut events = registry.take_events();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, p::EventPayload::SkillMetadataExposed(_)))
            .count(),
        2
    );
    assert!(events
        .iter()
        .all(|event| !matches!(event, p::EventPayload::SkillBodyLoaded(_))));

    assert_eq!(
        registry
            .load_body(p::SkillRef("skill:chosen".into()), LoadTrigger::Selected,)
            .unwrap(),
        SkillBody("chosen body".into())
    );
    let body_events = registry.take_events();
    assert_eq!(body_events.len(), 1);
    let p::EventPayload::SkillBodyLoaded(loaded) = &body_events[0] else {
        panic!("selected skill must produce SkillBodyLoaded");
    };
    assert_eq!(loaded.skill, p::SkillRef("skill:chosen".into()));
    assert_ne!(loaded.skill, p::SkillRef("skill:other".into()));
    events.extend(body_events);
    assert_eq!(
        events.iter().map(p::EventPayload::kind).collect::<Vec<_>>(),
        vec![
            p::EventKind::SkillMetadataExposed,
            p::EventKind::SkillMetadataExposed,
            p::EventKind::SkillBodyLoaded,
        ]
    );
    assert!(registry
        .load_body(
            p::SkillRef("skill:untrusted".into()),
            LoadTrigger::Referenced,
        )
        .is_err());
}

#[test]
fn s35_skill_search_is_bounded_explained_and_loads_only_the_selection() {
    let registry = InMemorySkillRegistry::with_search_limit(
        vec![
            skill(
                "skill:rust-review",
                "selected rust review body",
                p::TrustTier::ApprovedSource,
            ),
            skill(
                "skill:rust-format",
                "unselected rust format body",
                p::TrustTier::ApprovedSource,
            ),
            skill(
                "skill:rust-disabled",
                "disabled body",
                p::TrustTier::ApprovedSource,
            ),
            skill(
                "skill:rust-untrusted",
                "untrusted body",
                p::TrustTier::Untrusted,
            ),
        ],
        1,
    )
    .unwrap();
    registry
        .set_enabled(p::SkillRef("skill:rust-disabled".into()), false)
        .unwrap();

    let hits = registry
        .search(SkillSearchQuery {
            schema_version: p::SchemaVersion(1),
            text: "rust review".into(),
            scope: p::Scope("workspace:alpha".into()),
            limit: 20,
        })
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].metadata.id, p::SkillRef("skill:rust-review".into()));
    assert_eq!(hits[0].matched_terms, vec!["review", "rust"]);
    assert!(hits[0].score > 0 && !hits[0].explanation.is_empty());
    assert_eq!(
        registry
            .take_events()
            .iter()
            .map(p::EventPayload::kind)
            .collect::<Vec<_>>(),
        vec![p::EventKind::SkillMetadataExposed]
    );

    let body = registry
        .load_body(hits[0].metadata.id.clone(), LoadTrigger::Selected)
        .unwrap();
    assert_eq!(body, SkillBody("selected rust review body".into()));
    for blocked in ["skill:rust-disabled", "skill:rust-untrusted"] {
        assert!(registry
            .load_body(p::SkillRef(blocked.into()), LoadTrigger::Selected)
            .is_err());
    }
    let events = registry.take_events();
    assert_eq!(events.len(), 1);
    let p::EventPayload::SkillBodyLoaded(payload) = &events[0] else {
        panic!("only the selected skill may load a body");
    };
    assert_eq!(payload.skill, p::SkillRef("skill:rust-review".into()));
    assert!(!events.iter().any(|event| match event {
        p::EventPayload::SkillBodyLoaded(payload) => {
            payload.skill == p::SkillRef("skill:rust-format".into())
                || payload.skill == p::SkillRef("skill:rust-disabled".into())
                || payload.skill == p::SkillRef("skill:rust-untrusted".into())
        }
        _ => false,
    }));
}

fn descriptor(id: &str, risk: Option<p::Risk>, enabled: bool) -> CapabilityDescriptor {
    CapabilityDescriptor {
        schema_version: p::SchemaVersion(1),
        id: p::CapabilityRef(id.into()),
        capability: Capability::Tool(p::ToolRef(id.into())),
        scope: p::Scope("workspace:alpha".into()),
        permissions: vec![p::PermissionRef("execute".into())],
        risk,
        enabled,
    }
}

fn resolve_context(allowed: Vec<&str>) -> p::ResolveContext {
    p::ResolveContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session-1".into()),
        toolset: p::ToolsetRef("toolset-1".into()),
        envelope: envelope(),
        policy_allowed_providers: Vec::new(),
        policy_allowed_capabilities: allowed
            .into_iter()
            .map(|id| p::CapabilityRef(id.into()))
            .collect(),
    }
}

fn envelope() -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:alpha".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![
                p::CapabilityRef("tool:visible".into()),
                p::CapabilityRef("tool:unknown-risk".into()),
                p::CapabilityRef("tool:disabled".into()),
                p::CapabilityRef("tool:untrusted".into()),
            ],
            permissions: vec![p::PermissionRef("execute".into())],
        },
        action_type: vec![p::ActionType::Execute],
        risk_limit: p::Risk::Low,
        approval_rule: p::ApprovalRule::Allow,
        budget: p::Budget("capability-budget".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: 0,
            expires_at: 100,
            max_turns: 2,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}

fn skill(id: &str, body: &str, trust: p::TrustTier) -> SkillDefinition {
    SkillDefinition {
        schema_version: p::SchemaVersion(1),
        metadata: SkillMetadata {
            schema_version: p::SchemaVersion(1),
            id: p::SkillRef(id.into()),
            summary: format!("summary for {id}"),
            scope: p::Scope("workspace:alpha".into()),
            version: p::Version(1),
            trust,
        },
        body: SkillBody(body.into()),
    }
}
