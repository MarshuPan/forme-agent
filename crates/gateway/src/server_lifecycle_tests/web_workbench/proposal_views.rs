//! Real owner authentication and the signed bridge for proposal outcomes.
use super::*;
pub(super) async fn verify(
    address: SocketAddr,
    runtime: &ProductionRuntime,
    cookie: &str,
    csrf: &str,
) {
    let run = runtime.services().aggregates.memory_cognition.clone();
    let command = json!({"schema_version":1,"command":"proposal:web-resolve","run":run,"proposal":"proposal:missing","outcome":"Adopt","defer_until":null});
    let route = "/api/v1/brain/workbench/proposal";
    let before = runtime.services().store.run_head(&run).unwrap();
    for (key, forgery, expected) in [
        ("proposal:wrong-key", false, 400),
        ("proposal:web-resolve", true, 400),
    ] {
        let mut body = command.clone();
        if forgery {
            body["actor"] = json!("Owner");
        }
        let reply = request(
            address,
            "POST",
            route,
            mutation_headers(address, cookie, csrf, key),
            body.to_string(),
        )
        .await;
        assert_eq!(reply.status, expected, "{}", reply.body);
    }
    let denied = request(
        address,
        "POST",
        route,
        mutation_headers(address, cookie, "invalid-csrf", "proposal:web-resolve"),
        command.to_string(),
    )
    .await;
    assert_eq!(denied.status, 403);
    // A valid Web owner still cannot resolve a proposal that the brain never
    // emitted. This response must come through the real signed bridge/core.
    let reply = request(
        address,
        "POST",
        route,
        mutation_headers(address, cookie, csrf, "proposal:web-resolve"),
        command.to_string(),
    )
    .await;
    assert_eq!(reply.status, 409, "{}", reply.body);
    let pending = request(
        address,
        "GET",
        "/api/v1/workbench/proposals/pending",
        vec![("Cookie", cookie.into())],
        String::new(),
    )
    .await;
    assert_eq!(pending.status, 200, "{}", pending.body);
    assert_eq!(pending.json()["schema_version"], 1);
    assert_eq!(pending.json()["commands"].as_array().unwrap().len(), 0);
    let unauthenticated = request(
        address,
        "GET",
        "/api/v1/workbench/proposals/pending",
        vec![],
        String::new(),
    )
    .await;
    assert_eq!(unauthenticated.status, 401, "{}", unauthenticated.body);
    assert_eq!(runtime.services().store.run_head(&run).unwrap(), before);
    assert!(!events(runtime, &run).iter().any(|e| matches!(
        e.kind,
        p::EventKind::ActionStarted | p::EventKind::ProactiveProposalResolved
    )));
}
