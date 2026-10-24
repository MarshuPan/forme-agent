#![forbid(unsafe_code)]
#[path = "support/core_brain.rs"]
mod support;

use forme_harness::ProductionRuntime;
use forme_protocol as p;
use forme_store::EventStore;
use std::sync::atomic::Ordering;
use support::{now, Fixture};

fn upload(
    runtime: &ProductionRuntime,
    f: &Fixture,
    key: &str,
    bytes: &[u8],
) -> p::ArtifactUploadReceipt {
    runtime
        .upload_workbench_artifact(
            &f.owner(),
            p::ArtifactUploadCommand {
                schema_version: p::SchemaVersion(1),
                command: p::IdempotencyKey(key.into()),
                session: p::SessionId("session:material".into()),
                workspace: None,
                file_name: "source.txt".into(),
                media_type: "text/plain".into(),
                bytes_hex: bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
            },
        )
        .unwrap()
}

fn node(runtime: &ProductionRuntime, name: &str, content: p::ContentRef, scope: p::Scope) {
    runtime
        .services()
        .store
        .append(p::Event::new(
            p::EventId(format!("event:{name}")),
            runtime.services().aggregates.memory_cognition.clone(),
            None,
            p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
                node_id: p::NodeId(name.into()),
                kind: p::MemoryNodeType("episodic".into()),
                content_ref: content,
                tier: p::StabilityTier::Working,
                confidence: p::Confidence(0.4),
                scope,
                resting_activation: p::RestingActivation(0.0),
                recency: p::Recency(now()),
            }),
            p::SchemaVersion(1),
            now(),
            p::Provenance {
                source: p::Source::OwnerControl,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
        ))
        .unwrap();
}

fn query(runtime: &ProductionRuntime, f: &Fixture, name: &str) -> p::WorkbenchMaterialQuery {
    let list = runtime
        .workbench_snapshot(
            &f.owner(),
            p::WorkbenchQuery {
                schema_version: p::SchemaVersion(1),
                section: p::WorkbenchSection::Memory,
                scope: None,
                offset: 0,
                limit: 100,
                snapshot: None,
            },
        )
        .unwrap();
    let item = list.items.into_iter().find(|item| item.id == name).unwrap();
    p::WorkbenchMaterialQuery {
        schema_version: p::SchemaVersion(1),
        node: p::NodeId(item.id),
        run: item.run,
        event: item.event,
        scope: None,
        snapshot: list.digest,
    }
}

fn key_entries(f: &Fixture) -> std::collections::BTreeSet<std::ffi::OsString> {
    std::fs::read_dir(f.config.store_path.with_extension("content-keys"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect()
}

#[test]
fn registered_text_survives_restart_and_reads_never_write_events_or_call_models() {
    let f = Fixture::new();
    let runtime = f.build();
    let text = "材料原文：<script>globalThis.bad = true</script>\n不是指令。";
    let artifact = upload(&runtime, &f, "upload:one", text.as_bytes());
    node(
        &runtime,
        "memory:one",
        artifact.content_ref.clone(),
        artifact.scope.clone(),
    );
    let request = query(&runtime, &f, "memory:one");
    let heads = runtime.services().store.run_heads().unwrap();
    let keys = key_entries(&f);
    let first = runtime
        .workbench_material(&f.owner(), request.clone())
        .unwrap();
    let target = runtime
        .workbench_material_lifecycle_target(&f.owner(), request.clone())
        .unwrap();
    assert_eq!(target.node, request.node);
    assert_eq!(target.run, request.run);
    assert_eq!(target.event, request.event);
    assert_eq!(target.content_ref, artifact.content_ref);
    assert_eq!(target.scope, artifact.scope);
    assert_eq!(target.lifecycle_version, 0);
    assert_eq!(
        target.subject,
        runtime
            .services()
            .store
            .protected_content_for_current_owner(&artifact.content_ref)
            .unwrap()
            .unwrap()
            .subject_digest
    );
    assert_eq!(first.state, p::WorkbenchMaterialState::Available);
    assert_eq!(first.text.as_deref(), Some(text));
    assert_eq!(first.bytes, Some(text.len() as u64));
    assert_eq!(
        first.content_digest,
        Some(p::sha256_content_digest(text.as_bytes()))
    );
    for _ in 0..2 {
        assert_eq!(
            runtime
                .workbench_material_lifecycle_target(&f.owner(), request.clone())
                .unwrap(),
            target
        );
        assert_eq!(
            runtime
                .workbench_material(&f.owner(), request.clone())
                .unwrap(),
            first
        );
    }
    assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert_eq!(key_entries(&f), keys);
    drop(runtime);
    let runtime = f.build();
    assert_eq!(
        runtime
            .workbench_material(&f.owner(), request.clone())
            .unwrap(),
        first
    );
    assert_eq!(
        runtime
            .workbench_material_lifecycle_target(&f.owner(), request)
            .unwrap(),
        target
    );
    assert_eq!(key_entries(&f), keys);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn stale_or_forged_memory_identity_scope_schema_and_owner_do_not_disclose_text() {
    let f = Fixture::new();
    let runtime = f.build();
    let artifact = upload(&runtime, &f, "upload:binding", b"owner original");
    node(
        &runtime,
        "memory:binding",
        artifact.content_ref.clone(),
        artifact.scope.clone(),
    );
    let request = query(&runtime, &f, "memory:binding");
    let heads = runtime.services().store.run_heads().unwrap();
    assert!(runtime
        .workbench_material(&p::VerifiedPrincipal("other-owner".into()), request.clone())
        .is_err());
    assert!(runtime
        .workbench_material_lifecycle_target(
            &p::VerifiedPrincipal("other-owner".into()),
            request.clone()
        )
        .is_err());
    for kind in 0..5 {
        let mut forged = request.clone();
        match kind {
            0 => forged.node = p::NodeId("absent".into()),
            1 => forged.event = p::EventId("foreign-event".into()),
            2 => forged.run = p::RunId("foreign-run".into()),
            3 => forged.scope = Some(p::Scope("foreign-scope".into())),
            _ => forged.schema_version = p::SchemaVersion(2),
        }
        assert!(runtime
            .workbench_material(&f.owner(), forged.clone())
            .is_err());
        assert!(runtime
            .workbench_material_lifecycle_target(&f.owner(), forged)
            .is_err());
    }
    let mut body = serde_json::to_value(&request).unwrap();
    body["content_ref"] = serde_json::json!(artifact.content_ref);
    assert!(serde_json::from_value::<p::WorkbenchMaterialQuery>(body).is_err());
    assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    node(
        &runtime,
        "memory:new",
        p::ContentRef("not-a-readable-locator".into()),
        artifact.scope.clone(),
    );
    assert!(runtime
        .workbench_material(&f.owner(), request.clone())
        .unwrap_err()
        .0
        .contains("snapshot changed"));
    assert!(runtime
        .workbench_material_lifecycle_target(&f.owner(), request)
        .unwrap_err()
        .0
        .contains("snapshot changed"));
    node(
        &runtime,
        "memory:wrong-scope",
        artifact.content_ref,
        p::Scope("scope:other".into()),
    );
    assert!(runtime
        .workbench_material(&f.owner(), query(&runtime, &f, "memory:wrong-scope"))
        .is_err());
    assert!(runtime
        .workbench_material_lifecycle_target(&f.owner(), query(&runtime, &f, "memory:wrong-scope"))
        .is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn metadata_is_not_original_content_or_context_and_uploaded_receipt_shaped_json_is_data() {
    let f = Fixture::new();
    let runtime = f.build();
    let original = upload(&runtime, &f, "upload:namespace", b"source text");
    let metadata = p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(
            format!("runtime-artifact-material-v1:{}", original.content_ref.0).as_bytes()
        )
        .0
    ));
    node(
        &runtime,
        "memory:metadata",
        metadata.clone(),
        original.scope.clone(),
    );
    let view = runtime
        .workbench_material(&f.owner(), query(&runtime, &f, "memory:metadata"))
        .unwrap();
    assert_eq!(view.state, p::WorkbenchMaterialState::ReferenceOnly);
    assert_eq!(view.text, None);
    assert!(runtime
        .workbench_material_lifecycle_target(&f.owner(), query(&runtime, &f, "memory:metadata"))
        .is_err());
    assert!(runtime
        .read_workbench_artifact(
            &f.owner(),
            &metadata,
            &p::SessionId("session:material".into())
        )
        .is_err());
    let catalog = runtime
        .query_workbench_artifacts(
            &f.owner(),
            p::ArtifactCatalogQuery {
                schema_version: p::SchemaVersion(1),
                session: p::SessionId("session:material".into()),
                page: p::DataPageQuery {
                    schema_version: p::SchemaVersion(1),
                    offset: 0,
                    limit: 100,
                    snapshot: None,
                },
            },
        )
        .unwrap();
    assert_eq!(catalog.total, 1);
    let json = runtime
        .read_protected_artifact(&f.owner(), &metadata)
        .unwrap();
    let own_document = upload(&runtime, &f, "upload:json", &json);
    node(
        &runtime,
        "memory:json",
        own_document.content_ref,
        own_document.scope,
    );
    let view = runtime
        .workbench_material(&f.owner(), query(&runtime, &f, "memory:json"))
        .unwrap();
    assert_eq!(view.text.unwrap().as_bytes(), json);
    let protection = runtime
        .services()
        .store
        .content_protection()
        .unwrap()
        .unwrap();
    let (_, internal) = protection
        .seal_record(
            b"internal control companion must remain private",
            &original.scope,
            "control-resolution:private-fixture",
            &p::sha256_content_digest(b"private control subject"),
        )
        .unwrap();
    let private_ref = internal.content_ref.clone();
    protection
        .lifecycle_service(runtime.services().store.as_ref().clone())
        .put_protected_content(&f.owner(), internal)
        .unwrap();
    node(&runtime, "memory:opaque", private_ref, original.scope);
    assert!(runtime
        .workbench_material_lifecycle_target(&f.owner(), query(&runtime, &f, "memory:opaque"))
        .is_err());
    assert_eq!(
        runtime
            .workbench_material(&f.owner(), query(&runtime, &f, "memory:opaque"))
            .unwrap()
            .state,
        p::WorkbenchMaterialState::ReferenceOnly
    );
}

#[test]
fn binary_and_oversized_materials_have_no_partial_text() {
    let f = Fixture::new();
    let runtime = f.build();
    for (name, bytes, expected) in [
        (
            "binary",
            vec![0xff, 0xfe],
            p::WorkbenchMaterialState::NonText,
        ),
        (
            "large",
            vec![b'x'; p::WORKBENCH_MATERIAL_MAX_BYTES + 1],
            p::WorkbenchMaterialState::TooLarge,
        ),
    ] {
        let artifact = upload(&runtime, &f, &format!("upload:{name}"), &bytes);
        node(&runtime, name, artifact.content_ref, artifact.scope);
        let before = runtime.services().store.run_heads().unwrap();
        let view = runtime
            .workbench_material(&f.owner(), query(&runtime, &f, name))
            .unwrap();
        let target = runtime
            .workbench_material_lifecycle_target(&f.owner(), query(&runtime, &f, name))
            .unwrap();
        assert_eq!(target.lifecycle_version, 0);
        assert_eq!(view.state, expected);
        assert_eq!(view.bytes, Some(bytes.len() as u64));
        assert_eq!(view.text, None);
        assert_eq!(runtime.services().store.run_heads().unwrap(), before);
    }
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn real_source_deletion_does_not_recreate_original_or_companion_on_read_or_restart() {
    let f = Fixture::new();
    let runtime = f.build();
    let artifact = upload(&runtime, &f, "upload:erase", b"source to erase");
    node(
        &runtime,
        "memory:erase",
        artifact.content_ref.clone(),
        artifact.scope.clone(),
    );
    let request = query(&runtime, &f, "memory:erase");
    let selected = runtime
        .workbench_material_lifecycle_target(&f.owner(), request.clone())
        .unwrap();
    let execute = |key: &str, action| {
        runtime
            .execute_data_workbench(
                &f.owner(),
                p::DataWorkbenchCommand {
                    schema_version: p::SchemaVersion(1),
                    command: p::IdempotencyKey(key.into()),
                    action,
                },
            )
            .unwrap()
    };
    let prepared = execute(
        "erase:prepare",
        p::DataWorkbenchAction::PrepareLifecycle {
            scope: selected.scope.clone(),
            subject: selected.subject.clone(),
            operation: p::DataLifecycleOperation::Delete,
            expected_version: selected.lifecycle_version,
        },
    );
    let plan = prepared.plan.unwrap();
    assert_eq!(
        plan.objects, 3,
        "original and both derived receipts share the lifecycle subject"
    );
    let receipt = execute(
        "erase:apply",
        p::DataWorkbenchAction::Apply {
            prepared_by: p::IdempotencyKey("erase:prepare".into()),
            plan_digest: plan.digest,
        },
    );
    assert_eq!(receipt.state, p::DataCommandState::Confirmed);
    let heads = runtime.services().store.run_heads().unwrap();
    let keys = key_entries(&f);
    assert!(runtime
        .workbench_material_lifecycle_target(&f.owner(), request.clone())
        .is_err());
    let redacted = runtime
        .workbench_material(&f.owner(), request.clone())
        .unwrap();
    assert_eq!(redacted.state, p::WorkbenchMaterialState::ReferenceOnly);
    assert_eq!(redacted.text, None);
    assert_eq!(redacted.bytes, None);
    assert!(runtime
        .services()
        .store
        .protected_content_for_current_owner(&artifact.content_ref)
        .unwrap()
        .is_none());
    assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    assert_eq!(key_entries(&f), keys);
    drop(runtime);
    let runtime = f.build();
    assert_eq!(
        runtime
            .workbench_material(&f.owner(), request.clone())
            .unwrap(),
        redacted
    );
    assert!(runtime
        .workbench_material_lifecycle_target(&f.owner(), request)
        .is_err());
    assert_eq!(key_entries(&f), keys);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn lifecycle_selection_version_must_be_resolved_again_after_a_real_lifecycle_change() {
    let f = Fixture::new();
    let runtime = f.build();
    let artifact = upload(&runtime, &f, "upload:version", b"source remains retained");
    node(
        &runtime,
        "memory:version",
        artifact.content_ref,
        artifact.scope,
    );
    let request = query(&runtime, &f, "memory:version");
    let first = runtime
        .workbench_material_lifecycle_target(&f.owner(), request.clone())
        .unwrap();
    let retained = runtime
        .prepare_data_lifecycle(
            &f.owner(),
            first.scope.clone(),
            first.subject.clone(),
            p::DataLifecycleOperation::Retain,
            first.lifecycle_version,
        )
        .unwrap();
    runtime
        .apply_data_operation(
            &f.owner(),
            retained.clone(),
            forme_harness::DataOperationConfirmation {
                schema_version: p::SchemaVersion(1),
                plan_digest: retained.digest,
                nonce: retained.nonce,
                use_by: retained.expires_at,
                approved: true,
            },
        )
        .unwrap();
    let heads = runtime.services().store.run_heads().unwrap();
    let keys = key_entries(&f);
    assert!(runtime
        .prepare_data_lifecycle(
            &f.owner(),
            first.scope.clone(),
            first.subject.clone(),
            p::DataLifecycleOperation::Delete,
            first.lifecycle_version
        )
        .is_err());
    let current = runtime
        .workbench_material_lifecycle_target(&f.owner(), request)
        .unwrap();
    assert_eq!(current.subject, first.subject);
    assert_eq!(current.lifecycle_version, first.lifecycle_version + 1);
    assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    assert_eq!(key_entries(&f), keys);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}
