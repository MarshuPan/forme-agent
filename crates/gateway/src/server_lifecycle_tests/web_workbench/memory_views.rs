//! Authenticated Web -> signed bridge -> current memory -> registered material.
use super::*;
use forme_harness::HarnessIngress;
use std::sync::atomic::AtomicUsize;

pub(super) async fn verify(
    address: SocketAddr,
    runtime: &ProductionRuntime,
    calls: &Arc<AtomicUsize>,
    cookie: &str,
    csrf: &str,
) {
    let text = "原始材料 <script>globalThis.materialExecuted = true</script>";
    let owner = p::VerifiedPrincipal(runtime.config().owner.0.clone());
    let artifact = runtime
        .upload_workbench_artifact(
            &owner,
            p::ArtifactUploadCommand {
                schema_version: p::SchemaVersion(1),
                command: p::IdempotencyKey("material:web-upload".into()),
                session: p::SessionId("session:material-web".into()),
                workspace: None,
                file_name: "source.txt".into(),
                media_type: "text/plain".into(),
                bytes_hex: text.as_bytes().iter().map(|b| format!("{b:02x}")).collect(),
            },
        )
        .unwrap();
    let run = runtime.services().aggregates.memory_cognition.clone();
    let node = p::NodeId("memory:web-material".into());
    let harness = runtime.harness();
    let event = harness
        .append_ingress_events(
            run.clone(),
            vec![harness.ingress_authority().stamp(
                p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
                    node_id: node.clone(),
                    kind: p::MemoryNodeType("episodic".into()),
                    content_ref: artifact.content_ref.clone(),
                    tier: p::StabilityTier::Working,
                    confidence: p::Confidence(0.4),
                    scope: artifact.scope.clone(),
                    resting_activation: p::RestingActivation(0.0),
                    recency: p::Recency(now_ms()),
                }),
                p::Provenance {
                    source: p::Source::OwnerControl,
                    actor: p::Actor::Owner,
                    trust_tier: p::TrustTier::OwnerInput,
                    caused_by: None,
                },
            )],
        )
        .unwrap()
        .remove(0);
    let heads = runtime.services().store.run_heads().unwrap();
    let model_calls = calls.load(Ordering::SeqCst);
    let list = request(address, "POST", "/api/v1/brain/workbench/query",
        mutation_headers(address, cookie, csrf, "material:list"),
        json!({"schema_version":1,"section":"Memory","scope":null,"offset":0,"limit":100,"snapshot":null}).to_string(),
    ).await;
    assert_eq!(list.status, 200, "{}", list.body);
    let query = json!({"schema_version":1,"node":node,"run":run,"event":event,"scope":null,"snapshot":list.json()["digest"]});
    let read = request(
        address,
        "POST",
        "/api/v1/brain/workbench/material",
        mutation_headers(address, cookie, csrf, "material:read"),
        query.to_string(),
    )
    .await;
    assert_eq!(read.status, 200, "{}", read.body);
    assert_eq!(read.json()["state"], "Available");
    assert_eq!(read.json()["text"], text);
    assert_eq!(read.json()["scope"], artifact.scope.0);
    assert!(read
        .head
        .to_ascii_lowercase()
        .contains("cache-control: no-store"));
    let denied = request(
        address,
        "POST",
        "/api/v1/brain/workbench/material",
        mutation_headers(address, cookie, "wrong-csrf", "material:csrf"),
        query.to_string(),
    )
    .await;
    assert_eq!(denied.status, 403);
    let mut forged = query.clone();
    forged["content_ref"] = json!(artifact.content_ref);
    let denied = request(
        address,
        "POST",
        "/api/v1/brain/workbench/material",
        mutation_headers(address, cookie, csrf, "material:arbitrary-ref"),
        forged.to_string(),
    )
    .await;
    assert_eq!(denied.status, 400);
    assert!(!denied.body.contains(text));
    let mut stale = query.clone();
    stale["snapshot"] = json!(format!("sha256:{}", "0".repeat(64)));
    let denied = request(
        address,
        "POST",
        "/api/v1/brain/workbench/material",
        mutation_headers(address, cookie, csrf, "material:stale"),
        stale.to_string(),
    )
    .await;
    assert_eq!(denied.status, 409);
    let mut forged = query;
    forged["node"] = json!("memory:absent");
    let denied = request(
        address,
        "POST",
        "/api/v1/brain/workbench/material",
        mutation_headers(address, cookie, csrf, "material:wrong-node"),
        forged.to_string(),
    )
    .await;
    assert_ne!(denied.status, 200);
    assert!(!denied.body.contains(text));
    assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    assert_eq!(calls.load(Ordering::SeqCst), model_calls);
}
