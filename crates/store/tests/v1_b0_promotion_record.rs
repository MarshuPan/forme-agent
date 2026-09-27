//! B0: the authority store refuses a promotion whose stable record is
//! invalid, so an unreplayable or secret-bearing record never enters the log.

use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore, StoreOptions};

fn promotion(id: &str, statement: &str) -> p::Event {
    p::Event::new(
        p::EventId(id.into()),
        p::RunId("b0:store".into()),
        None,
        p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
            candidate_id: p::CandidateId("candidate-1".into()),
            by: p::DecisionActor::User,
            reason: p::ReasonRef("confirmed".into()),
            stable: Some(p::StableCognitionRecord {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef("frame:verify-first".into()),
                scope: p::Scope("workspace:b0".into()),
                kind: p::StableCognitionKindRef("frame".into()),
                statement: statement.into(),
                tier: p::StabilityTier::Stable,
                confidence: p::Confidence(0.9),
                evidence: Vec::new(),
                last_reproduced_at: 30,
            }),
        }),
        p::SchemaVersion(1),
        1,
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
    )
}

#[test]
fn store_refuses_an_invalid_promotion_record_before_writing() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    assert!(store
        .append(promotion("b0:invalid", "remember the password is hunter2"))
        .is_err());
    assert!(store.read_run(p::RunId("b0:store".into())).next().is_none());
    store
        .append(promotion("b0:valid", "verify externally before completion"))
        .unwrap();
    let stored = store
        .read_run(p::RunId("b0:store".into()))
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(stored.len(), 1);
    let p::EventPayload::CandidatePromoted(payload) = &stored[0].payload else {
        panic!("the valid promotion is stored as written");
    };
    assert_eq!(
        payload.stable.as_ref().map(|record| record.object.clone()),
        Some(p::ObjectRef("frame:verify-first".into()))
    );
}
