use forme_cognition::*;
use forme_protocol as p;

fn events() -> Vec<p::Event> {
    vec![p::Event::new(
        p::EventId("goal-event".into()),
        p::RunId("goal-stream".into()),
        None,
        p::EventPayload::GoalFramed(p::GoalFramedPayload {
            goal_frame: p::GoalFrameRef("goal:research".into()),
            long_term: None,
            knowledge: Some(p::GoalKnowledge {
                schema_version: p::SchemaVersion(1),
                scope: p::Scope("project:a".into()),
                allow_questions: true,
                requirements: vec![p::KnowledgeRequirement {
                    schema_version: p::SchemaVersion(1),
                    target: p::LearningTarget::UserAttribute(p::UserAttributeRef("risk".into())),
                    reason: p::ReasonRef("risk changes the choice".into()),
                    impact: p::Impact::High,
                    minimum_confidence: p::Confidence(0.8),
                }],
            }),
        }),
        p::SchemaVersion(1),
        100,
        p::Provenance {
            source: p::Source::UserTurn,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
    )]
}

#[test]
fn known_scoped_knowledge_closes_only_its_declared_gap() {
    let learning = LearningSnapshot::rebuild(&events());
    assert_eq!(learning.gaps(&[]).unwrap().len(), 1);
    let mut stable = StableCognition {
        schema_version: p::SchemaVersion(1),
        object: p::ObjectRef("user-attribute:risk:project:a".into()),
        scope: MapScope {
            schema_version: p::SchemaVersion(1),
            scope: p::Scope("project:a".into()),
        },
        kind: StableCognitionKind::UserAttribute,
        statement: "risk is conservative".into(),
        tier: p::StabilityTier::Stable,
        confidence: p::Confidence(0.9),
        evidence: vec![],
        last_reproduced_at: 100,
        active: true,
    };
    assert!(learning.gaps(&[stable.clone()]).unwrap().is_empty());
    stable.scope.scope = p::Scope("project:b".into());
    assert_eq!(learning.gaps(&[stable]).unwrap().len(), 1);
}

#[test]
fn legacy_payloads_omit_new_fields_and_untrusted_or_forged_learning_is_rejected() {
    let mut event = events().remove(0);
    event.provenance.trust_tier = p::TrustTier::Untrusted;
    assert!(event.validate_payload_kind().is_err());
    let record = LearningSnapshot::rebuild(&events())
        .gaps(&[])
        .unwrap()
        .remove(0)
        .record;
    record.validate().unwrap();
    let mut forged = record;
    forged.scope = p::Scope("other".into());
    assert!(forged.validate().is_err());
}
