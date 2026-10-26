//! Storage recovery must survive unavailable runtime-only configuration.
use super::*;

fn configuration() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "install_ref": "install:offline",
        "revision": 2,
        "brain": {
            "schema_version": 1,
            "owner": "owner:offline",
            "workspace": "workspace:managed",
            "store_path": "fixture.db",
            "artifact_root": "artifacts",
            "agent_profile": null,
            "models": {"unavailable": true},
            "scheduler_enabled": null,
            "scheduler": null,
            "cognition": null,
            "authority_lease": null
        },
        "transport": null,
        "drain_timeout_ms": null
    })
}

#[test]
fn offline_projection_ignores_known_routing_but_rejects_unknown_storage_fields() {
    let mut value = configuration();
    for routing in [
        None,
        Some(serde_json::json!({"schema_version": 1, "cognition": "model:missing"})),
        Some(serde_json::json!({"unavailable": true})),
        Some(serde_json::Value::Null),
    ] {
        if let Some(routing) = routing {
            value["brain"]["model_routing"] = routing;
        }
        let parsed: StorageConfig = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(parsed.brain.owner.0, "owner:offline");
        assert_eq!(parsed.brain.workspace.0, "workspace:managed");
        assert_eq!(parsed.brain.store_path, PathBuf::from("fixture.db"));
    }
    for runtime in [
        serde_json::Value::Null,
        serde_json::json!({"store_path":"wrong.db","url":"unavailable"}),
    ] {
        value["model_proxy"] = runtime.clone();
        value["brain"]["worker_binding"] = runtime.clone();
        value["brain"]["worker_file_roots"] = runtime;
        let parsed: StorageConfig = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(parsed.brain.owner.0, "owner:offline");
        assert_eq!(parsed.brain.store_path, PathBuf::from("fixture.db"));
        assert_eq!(parsed.brain.artifact_root, PathBuf::from("artifacts"));
    }
    let mut unknown = value.clone();
    unknown["runtime_override"] = serde_json::json!({});
    assert!(serde_json::from_value::<StorageConfig>(unknown).is_err());
    value["brain"]["unknown_storage_override"] = serde_json::json!("other.db");
    assert!(serde_json::from_value::<StorageConfig>(value).is_err());
}
