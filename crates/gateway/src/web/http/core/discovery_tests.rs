use super::*;

fn owner() -> p::WebSessionView {
    p::WebSessionView {
        schema_version: p::SchemaVersion(1),
        account_ref: "account:discovery".into(),
        owner: p::OwnerPrincipalRef("owner:actual".into()),
        login_name: "fixture".into(),
        role: Default::default(),
        auth_epoch: 1,
        idle_expires_at: 100,
        absolute_expires_at: 200,
    }
}
fn request() -> serde_json::Value {
    serde_json::json!({
        "schema_version":1,"session":"session:discovery","command":"command:discovery",
        "spec":{"schema_version":1,"server":"fixture","program_ref":"program:fixture",
            "registration_digest":p::sha256_content_digest(b"registration"),
            "settings_digest":p::sha256_content_digest(b"settings"),"timeout":1000,"output_budget_bytes":8192}
    })
}

#[test]
fn discovery_bridge_binds_the_exact_key_and_rejects_caller_authority_or_process_fields() {
    let request = request();
    let body = serde_json::to_vec(&request).unwrap();
    let bound = commands::bind_body(
        "POST",
        "mcp/discovery",
        &body,
        &owner(),
        Some("command:discovery"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bound).unwrap(),
        request
    );
    for field in [
        "owner",
        "source",
        "plan",
        "envelope",
        "credential",
        "args",
        "env",
    ] {
        let mut bad = request.clone();
        bad[field] = "forged".into();
        assert!(commands::bind_body(
            "POST",
            "mcp/discovery",
            &serde_json::to_vec(&bad).unwrap(),
            &owner(),
            Some("command:discovery")
        )
        .is_err());
    }
    for key in [None, Some("command:other")] {
        assert!(commands::bind_body("POST", "mcp/discovery", &body, &owner(), key).is_err());
    }
    let mut bad = request;
    bad["spec"]["tool"] = "arbitrary.execute".into();
    assert!(commands::bind_body(
        "POST",
        "mcp/discovery",
        &serde_json::to_vec(&bad).unwrap(),
        &owner(),
        Some("command:discovery")
    )
    .is_err());
}

#[test]
fn discovery_routes_and_publication_are_finite_and_never_forward_worker_management() {
    assert_eq!(
        allowlisted_core_path("POST", "mcp/discovery", None).unwrap(),
        "/v1/managed/mcp/discovery"
    );
    assert_eq!(
        allowlisted_core_path("POST", "mcp/discovery/run:original/publish", None).unwrap(),
        "/v1/managed/mcp/discovery/run:original/publish"
    );
    for (method, path, query) in [
        ("GET", "mcp/discovery", None),
        ("POST", "mcp/discovery", Some("auto=true")),
        ("POST", "mcp/discovery/run:original/execute", None),
        ("POST", "mcp/programs", None),
        ("POST", "agent/workers/dispatch", None),
        ("POST", "mcp/discovery/../publish", None),
    ] {
        assert!(allowlisted_core_path(method, path, query).is_err());
    }
    let body =
        serde_json::to_vec(&serde_json::json!({"schema_version":1,"command":"command:publish"}))
            .unwrap();
    assert!(commands::bind_body(
        "POST",
        "mcp/discovery/run:original/publish",
        &body,
        &owner(),
        Some("command:publish")
    )
    .is_ok());
    assert!(commands::bind_body(
        "POST",
        "mcp/discovery/run:original/publish",
        &body,
        &owner(),
        Some("command:other")
    )
    .is_err());
}
