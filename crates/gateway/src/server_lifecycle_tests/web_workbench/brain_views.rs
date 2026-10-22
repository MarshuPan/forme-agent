//! Actual authenticated Web -> signed bridge -> production brain boundaries.
use super::*;

pub(super) async fn verify(
    address: SocketAddr,
    runtime: &ProductionRuntime,
    cookie: &str,
    csrf: &str,
) {
    let run = runtime.services().aggregates.memory_cognition.clone();
    let mut ask = p::AskToLearnRecord {
        schema_version: p::SchemaVersion(1),
        goal: p::GoalFrameRef("goal:fixture".into()),
        scope: p::Scope(runtime.config().workspace.0.clone()),
        target: p::LearningTarget::UserAttribute(p::UserAttributeRef("choice".into())),
        reason: p::ReasonRef("check owner preference".into()),
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
                proposal: p::ProposalRef("proposal:fixture".into()),
                value: p::UserAttributeValueRef("content:owner-observation".into()),
            },
            p::EvidenceRef("evidence:owner-observation".into()),
            now_ms(),
        )
        .unwrap();
    let query = json!({"schema_version":1,"section":"Cognition","scope":null,"offset":0,"limit":1,"snapshot":null});
    let before = runtime.services().store.run_head(&run).unwrap();
    let denied = request(
        address,
        "POST",
        "/api/v1/brain/workbench/query",
        mutation_headers(address, cookie, "wrong-csrf", "query:denied"),
        query.to_string(),
    )
    .await;
    assert_eq!(denied.status, 403);
    let reply = request(
        address,
        "POST",
        "/api/v1/brain/workbench/query",
        mutation_headers(address, cookie, csrf, "query:one"),
        query.to_string(),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    let view = reply.json();
    assert_eq!(view["items"][0]["id"], candidate.0);
    assert_eq!(runtime.services().store.run_head(&run).unwrap(), before);
    let review = json!({"schema_version":1,"command":"review:web-one","run":run,"candidate":candidate,"expected_state":"Candidate","decision":"Reject"});
    let mismatched = request(
        address,
        "POST",
        "/api/v1/brain/workbench/review",
        mutation_headers(address, cookie, csrf, "review:wrong-key"),
        review.to_string(),
    )
    .await;
    assert_eq!(mismatched.status, 400);
    let mut forged = review.clone();
    forged["actor"] = json!("Owner");
    assert_eq!(
        request(
            address,
            "POST",
            "/api/v1/brain/workbench/review",
            mutation_headers(address, cookie, csrf, "review:web-one"),
            forged.to_string()
        )
        .await
        .status,
        400
    );
    assert_eq!(runtime.services().store.run_head(&run).unwrap(), before);
    let applied = request(
        address,
        "POST",
        "/api/v1/brain/workbench/review",
        mutation_headers(address, cookie, csrf, "review:web-one"),
        review.to_string(),
    )
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["status"], "Confirmed");
    let after = runtime.services().store.run_head(&run).unwrap();
    assert_eq!(after, before + 3); // start receipt, governed rejection, confirmation
    let repeated = request(
        address,
        "POST",
        "/api/v1/brain/workbench/review",
        mutation_headers(address, cookie, csrf, "review:web-one"),
        review.to_string(),
    )
    .await;
    assert_eq!(repeated.json(), applied.json());
    assert_eq!(runtime.services().store.run_head(&run).unwrap(), after);
    let mut changed = review;
    changed["decision"] = json!("Promote");
    assert_eq!(
        request(
            address,
            "POST",
            "/api/v1/brain/workbench/review",
            mutation_headers(address, cookie, csrf, "review:web-one"),
            changed.to_string()
        )
        .await
        .status,
        409
    );
    let mut stale = query;
    stale["snapshot"] = view["digest"].clone();
    assert_eq!(
        request(
            address,
            "POST",
            "/api/v1/brain/workbench/query",
            mutation_headers(address, cookie, csrf, "query:stale"),
            stale.to_string()
        )
        .await
        .status,
        409
    );
}
