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
    let mut forged = query.clone();
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

    let target = request(
        address,
        "POST",
        "/api/v1/brain/workbench/material-lifecycle",
        mutation_headers(address, cookie, csrf, "material:target"),
        query.to_string(),
    )
    .await;
    assert_eq!(target.status, 200, "{}", target.body);
    assert_eq!(target.json()["node"], node.0);
    assert_eq!(target.json()["content_ref"], artifact.content_ref.0);
    assert!(!target.body.contains(text));
    let selected: p::WorkbenchMaterialLifecycleView = serde_json::from_str(&target.body).unwrap();
    assert_eq!(
        selected.subject,
        runtime
            .services()
            .store
            .protected_content_for_current_owner(&artifact.content_ref)
            .unwrap()
            .unwrap()
            .subject_digest
    );
    for (name, proof, body, expected) in [
        ("csrf", "wrong", query.clone(), 403),
        ("stale", csrf, stale, 409),
        ("forged", csrf, forged, 400),
    ] {
        let denied = request(
            address,
            "POST",
            "/api/v1/brain/workbench/material-lifecycle",
            mutation_headers(address, cookie, proof, &format!("material:target-{name}")),
            body.to_string(),
        )
        .await;
        if name == "forged" {
            assert_ne!(denied.status, 200);
        } else {
            assert_eq!(denied.status, expected, "{}", denied.body);
        }
        assert!(!denied.body.contains(text));
    }
    assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    assert_eq!(calls.load(Ordering::SeqCst), model_calls);
    let action = json!({"PrepareLifecycle":{"scope":selected.scope,"subject":selected.subject,
        "operation":"Delete","expected_version":selected.lifecycle_version}});
    let denied = data_views::command(
        address,
        cookie,
        csrf,
        "material:bad-password",
        action.clone(),
        "wrong password",
    )
    .await;
    assert_eq!(denied.status, 401);
    assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    let prepared =
        data_views::command(address, cookie, csrf, "material:prepare", action, PASSWORD).await;
    assert_eq!(prepared.status, 200, "{}", prepared.body);
    assert_eq!(prepared.json()["state"], "Prepared");
    assert_eq!(prepared.json()["plan"]["objects"], 3);
    // Preview is not deletion; the body is still available until exact Apply.
    assert_eq!(
        runtime
            .workbench_material(&owner, serde_json::from_value(query.clone()).unwrap())
            .unwrap()
            .text
            .as_deref(),
        Some(text)
    );
    let digest = prepared.json()["plan"]["digest"].clone();
    let bad_apply = data_views::command(address, cookie, csrf, "material:bad-plan",
        json!({"Apply":{"prepared_by":"material:prepare","plan_digest":format!("sha256:{}", "0".repeat(64))}}), PASSWORD).await;
    assert_eq!(bad_apply.status, 409);
    let apply = json!({"Apply":{"prepared_by":"material:prepare","plan_digest":digest}});
    let applied = data_views::command(
        address,
        cookie,
        csrf,
        "material:apply",
        apply.clone(),
        PASSWORD,
    )
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["state"], "Confirmed");
    assert_eq!(applied.json()["lifecycle"]["operation"], "Delete");
    assert!(
        applied.json()["lifecycle"]["destroyed_keys"]
            .as_u64()
            .unwrap()
            >= 3
    );
    assert!(runtime
        .services()
        .store
        .protected_content_for_current_owner(&artifact.content_ref)
        .unwrap()
        .is_none());
    let after_delete = runtime.services().store.run_heads().unwrap();
    let repeated = data_views::command(
        address,
        cookie,
        csrf,
        "material:apply",
        apply.clone(),
        PASSWORD,
    )
    .await;
    assert_eq!(repeated.json(), applied.json());
    let second = data_views::command(
        address,
        cookie,
        csrf,
        "material:second-apply",
        apply,
        PASSWORD,
    )
    .await;
    assert_eq!(second.status, 409);
    let original = data_views::query(
        address,
        cookie,
        csrf,
        "material:receipt",
        json!({"Inspect":{"command":"material:apply"}}),
    )
    .await;
    assert_eq!(original.status, 200);
    assert_eq!(original.json()["receipt"], applied.json());
    let erased = request(
        address,
        "POST",
        "/api/v1/brain/workbench/material-lifecycle",
        mutation_headers(address, cookie, csrf, "material:erased-target"),
        query.to_string(),
    )
    .await;
    assert_ne!(erased.status, 200);
    assert!(!erased.body.contains(text));
    assert!(runtime
        .read_workbench_artifact(
            &owner,
            &artifact.content_ref,
            &p::SessionId("session:material-web".into())
        )
        .is_err());
    assert_eq!(runtime.services().store.run_heads().unwrap(), after_delete);
    assert_eq!(calls.load(Ordering::SeqCst), model_calls);
}
