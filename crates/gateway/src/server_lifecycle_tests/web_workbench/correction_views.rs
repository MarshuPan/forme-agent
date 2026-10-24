//! Authenticated product API -> signed bridge -> the same atomic brain writer.
use super::*;

pub(super) async fn verify(
    address: SocketAddr,
    runtime: &ProductionRuntime,
    cookie: &str,
    csrf: &str,
) {
    let mut ask = p::AskToLearnRecord {
        schema_version: p::SchemaVersion(1),
        goal: p::GoalFrameRef("goal:correction".into()),
        scope: p::Scope(runtime.config().workspace.0.clone()),
        target: p::LearningTarget::UserAttribute(p::UserAttributeRef("temporary-interest".into())),
        reason: p::ReasonRef("verify owner choice".into()),
        impact: p::Impact::Low,
        origin: p::SchemaDigest(String::new()),
    };
    ask.origin = ask.expected_origin().unwrap();
    let candidate = runtime
        .services()
        .cognition
        .propose_learning_reply(
            &ask,
            &p::LearningReplyRecord {
                schema_version: p::SchemaVersion(1),
                proposal: p::ProposalRef("proposal:correction".into()),
                value: p::UserAttributeValueRef("content:temporary-interest".into()),
            },
            p::EvidenceRef("evidence:temporary-interest".into()),
            now_ms(),
        )
        .unwrap();
    let view = runtime
        .workbench_snapshot(
            &p::VerifiedPrincipal(runtime.config().owner.0.clone()),
            p::WorkbenchQuery {
                schema_version: p::SchemaVersion(1),
                section: p::WorkbenchSection::Cognition,
                scope: None,
                offset: 0,
                limit: 100,
                snapshot: None,
            },
        )
        .unwrap();
    let item = view.items.iter().find(|v| v.id == candidate.0).unwrap();
    let body = json!({"schema_version":1,"command":"correction:web","run":item.run,"kind":"Candidate",
        "id":item.id,"event":item.event,"snapshot":view.digest,"statement":"web correction feedback <script>data only</script>"});
    let before = runtime.services().store.run_heads().unwrap();
    let call = |body: Value, key: &str, csrf: &str| {
        request(
            address,
            "POST",
            "/api/v1/brain/workbench/correction",
            mutation_headers(address, cookie, csrf, key),
            body.to_string(),
        )
    };
    assert_eq!(
        call(body.clone(), "correction:web", "bad-csrf")
            .await
            .status,
        403
    );
    assert_eq!(call(body.clone(), "wrong-key", csrf).await.status, 400);
    for (field, value) in [
        ("actor", json!("Owner")),
        ("scope", json!("other")),
        ("confidence", json!(1.0)),
        ("target", json!("permissions")),
    ] {
        let mut forged = body.clone();
        forged[field] = value;
        assert_eq!(call(forged, "correction:web", csrf).await.status, 400);
    }
    let mut stale = body.clone();
    stale["snapshot"] = json!(p::sha256_content_digest(b"stale"));
    assert_eq!(call(stale, "correction:web", csrf).await.status, 409);
    assert_eq!(runtime.services().store.run_heads().unwrap(), before);
    let applied = call(body.clone(), "correction:web", csrf).await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["status"], "Confirmed");
    let after = runtime.services().store.run_heads().unwrap();
    assert_eq!(
        call(body.clone(), "correction:web", csrf).await.json(),
        applied.json()
    );
    let mut conflict = body;
    conflict["statement"] = json!("changed body");
    assert_eq!(call(conflict, "correction:web", csrf).await.status, 409);
    let query = request(
        address,
        "POST",
        "/api/v1/brain/workbench/corrections/query",
        mutation_headers(address, cookie, csrf, "correction-query:web"),
        json!({"schema_version":1,"command":"correction:web"}).to_string(),
    )
    .await;
    assert_eq!(query.status, 200, "{}", query.body);
    assert_eq!(query.json()["receipts"][0], applied.json());
    assert!(!query.body.contains("web correction feedback"));
    assert_eq!(runtime.services().store.run_heads().unwrap(), after);
    let snapshot = runtime
        .workbench_snapshot(
            &p::VerifiedPrincipal(runtime.config().owner.0.clone()),
            p::WorkbenchQuery {
                schema_version: p::SchemaVersion(1),
                section: p::WorkbenchSection::Cognition,
                scope: None,
                offset: 0,
                limit: 100,
                snapshot: None,
            },
        )
        .unwrap();
    assert_eq!(
        snapshot
            .items
            .iter()
            .find(|v| v.id == candidate.0)
            .unwrap()
            .state,
        "Downgraded"
    );
    assert!(snapshot
        .items
        .iter()
        .any(|v| v.id == applied.json()["candidate"].as_str().unwrap()
            && v.state == "Candidate"
            && v.confidence == Some(p::Confidence(0.4))));
}
