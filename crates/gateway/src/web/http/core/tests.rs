use super::allowlisted_core_path;

#[test]
fn core_routes_are_explicit_and_preserve_event_cursor() {
    assert_eq!(
        allowlisted_core_path("GET", "runs/run:one/events", Some("after=7")).unwrap(),
        "/v1/runs/run:one/events?after=7"
    );
    assert_eq!(
        allowlisted_core_path("GET", "runs/run:one/pending", None).unwrap(),
        "/v1/runs/run:one/pending"
    );
    assert_eq!(
        allowlisted_core_path("GET", "runtime/doctor", None).unwrap(),
        "/v1/runtime/doctor"
    );
    assert!(allowlisted_core_path("GET", "runs/run:one/events", Some("before=7")).is_err());
    assert!(allowlisted_core_path("GET", "runs/../events", None).is_err());
}

#[test]
fn product_routes_are_mapped_without_exposing_a_wildcard_proxy() {
    assert_eq!(
        allowlisted_core_path("GET", "workspace", None).unwrap(),
        "/v1/brain/workspace"
    );
    assert_eq!(
        allowlisted_core_path("GET", "workspaces/workspace:demo/charter", None).unwrap(),
        "/v1/workspaces/workspace:demo/charter"
    );
    assert_eq!(
        allowlisted_core_path("GET", "runs/run:one", None).unwrap(),
        "/v1/runs/run:one"
    );
    assert_eq!(
        allowlisted_core_path("PUT", "workspaces/workspace:demo/charter", None).unwrap(),
        "/v1/workspaces/workspace:demo/charter"
    );
    assert_eq!(
        allowlisted_core_path("POST", "workspaces/workspace:demo/charter", None).unwrap(),
        "/v1/workspaces/workspace:demo/charter"
    );
    assert_eq!(
        allowlisted_core_path(
            "GET",
            "artifacts/content:fixture",
            Some("session=session%3Aone")
        )
        .unwrap(),
        "/v1/artifacts/content:fixture?session=session%3Aone"
    );
    assert!(
        allowlisted_core_path("GET", "artifacts/content:fixture", Some("scope=other")).is_err()
    );
    assert!(allowlisted_core_path("POST", "runtime/doctor", None).is_err());
    assert!(allowlisted_core_path("GET", "runtime/doctor", Some("tick=1")).is_err());
    assert!(allowlisted_core_path("POST", "runtime/tick", None).is_err());
    assert!(allowlisted_core_path("POST", "runtime/reload", None).is_err());
    assert!(allowlisted_core_path("GET", "unknown", None).is_err());
    assert!(allowlisted_core_path("POST", "candidates/candidate:one/review", None).is_err());
    assert!(allowlisted_core_path("POST", "profile", None).is_err());
    assert!(allowlisted_core_path("DELETE", "runs/run:one/control", None).is_err());
}
