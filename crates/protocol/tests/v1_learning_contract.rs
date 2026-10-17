use forme_protocol as p;

#[test]
fn learning_additions_keep_the_legacy_payload_bytes_and_taxonomy() {
    let legacy: p::GoalFramedPayload = serde_json::from_str(r#"{"goal_frame":"goal"}"#).unwrap();
    assert!(legacy.knowledge.is_none());
    assert_eq!(
        serde_json::to_string(&legacy).unwrap(),
        r#"{"goal_frame":"goal"}"#
    );
    let observation: p::ObservationRecordedPayload =
        serde_json::from_str(r#"{"source":"Internal","scope":"owner","grant_ref":null}"#).unwrap();
    assert!(observation.learning_reply.is_none());
    assert!(!serde_json::to_string(&observation)
        .unwrap()
        .contains("learning_reply"));
    assert_eq!(p::EventKind::ALL.len(), 99);
}
