//! Owner artifacts retain their catalog across process lifetimes; companion
//! metadata is protected storage, not an extra piece of model context.
#[path = "support/core_brain.rs"]
mod support;
use forme_context::{ContextBudget, ContextHydrationRequest, ContextHydrator};
use forme_harness::ProductionRuntime;
use forme_protocol as p;
use support::*;

fn upload(key: &str, session: &str, bytes: &[u8]) -> p::ArtifactUploadCommand {
    p::ArtifactUploadCommand {
        schema_version: p::SchemaVersion(1),
        command: p::IdempotencyKey(key.into()),
        session: p::SessionId(session.into()),
        workspace: None,
        file_name: "owner-notes.txt".into(),
        media_type: "text/plain".into(),
        bytes_hex: bytes.iter().map(|b| format!("{b:02x}")).collect(),
    }
}
fn query(session: &str) -> p::ArtifactCatalogQuery {
    p::ArtifactCatalogQuery {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId(session.into()),
        page: p::DataPageQuery {
            schema_version: p::SchemaVersion(1),
            offset: 0,
            limit: 1,
            snapshot: None,
        },
    }
}
fn view(runtime: &ProductionRuntime, f: &Fixture) -> p::ArtifactCatalogView {
    runtime
        .query_workbench_artifacts(&f.owner(), query("session:one"))
        .unwrap()
}
fn companion(id: &p::ContentRef) -> p::ContentRef {
    p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(format!("workbench-artifact-metadata-v1:{}", id.0).as_bytes()).0
    ))
}
fn write_raw(
    runtime: &ProductionRuntime,
    f: &Fixture,
    command: &p::ArtifactUploadCommand,
    bytes: &[u8],
) -> p::ContentRef {
    let protection = runtime
        .services()
        .store
        .content_protection()
        .unwrap()
        .unwrap();
    let subject = p::canonical_digest(&(
        &command.session,
        &command.workspace,
        &command.file_name,
        &command.media_type,
        &p::sha256_content_digest(bytes),
    ))
    .unwrap();
    let (_, record) = protection
        .seal_record(
            bytes,
            &p::Scope(format!("workbench-artifact:{}", command.session.0)),
            &format!("artifact:{}", command.command.0),
            &subject,
        )
        .unwrap();
    let id = record.content_ref.clone();
    protection
        .lifecycle_service(runtime.services().store.as_ref().clone())
        .put_protected_content(&f.owner(), record)
        .unwrap();
    id
}

#[test]
fn encrypted_catalog_survives_restart_and_reads_preserve_brain_events() {
    let f = Fixture::new();
    let runtime = f.build();
    let command = upload("catalog:one", "session:one", b"owner artifact body");
    let receipt = runtime
        .upload_workbench_artifact(&f.owner(), command.clone())
        .unwrap();
    assert_eq!(
        runtime
            .upload_workbench_artifact(&f.owner(), command)
            .unwrap(),
        receipt
    );
    let before = events(&runtime);
    let first = view(&runtime, &f);
    assert_eq!(first.total, 1);
    assert_eq!(
        first.artifacts[0].file_name.as_deref(),
        Some("owner-notes.txt")
    );
    assert_eq!(first.artifacts[0].media_type.as_deref(), Some("text/plain"));
    assert_eq!(first.artifacts[0].content_digest, receipt.content_digest);
    assert_eq!(events(&runtime), before);
    assert_eq!(f.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    let refs = runtime
        .services()
        .store
        .protected_content_refs(&f.owner(), &receipt.scope)
        .unwrap();
    assert_eq!(
        refs.len(),
        2,
        "one raw object and one companion, even after repeat upload"
    );
    drop(runtime);
    let runtime = f.build();
    assert_eq!(view(&runtime, &f), first);
    assert_eq!(
        runtime
            .read_workbench_artifact(
                &f.owner(),
                &receipt.content_ref,
                &p::SessionId("session:one".into())
            )
            .unwrap(),
        b"owner artifact body"
    );
    let stored = std::fs::read(&f.config.store_path).unwrap();
    for plaintext in [
        b"owner artifact body".as_slice(),
        b"owner-notes.txt",
        b"text/plain",
    ] {
        assert!(!stored
            .windows(plaintext.len())
            .any(|bytes| bytes == plaintext));
    }
}

#[test]
fn catalog_is_owner_and_session_bound_and_pages_reject_stale_snapshots() {
    let f = Fixture::new();
    let runtime = f.build();
    let first = runtime
        .upload_workbench_artifact(&f.owner(), upload("one", "session:one", b"first"))
        .unwrap();
    assert!(runtime
        .query_workbench_artifacts(
            &p::VerifiedPrincipal("owner:other".into()),
            query("session:one")
        )
        .is_err());
    assert!(runtime
        .read_workbench_artifact(
            &f.owner(),
            &first.content_ref,
            &p::SessionId("session:wrong".into())
        )
        .is_err());
    assert_eq!(
        runtime
            .query_workbench_artifacts(&f.owner(), query("session:wrong"))
            .unwrap()
            .total,
        0
    );
    let snapshot = view(&runtime, &f).digest;
    runtime
        .upload_workbench_artifact(&f.owner(), upload("two", "session:one", b"second"))
        .unwrap();
    let mut page = query("session:one");
    page.page.offset = 1;
    page.page.snapshot = Some(snapshot);
    assert!(runtime
        .query_workbench_artifacts(&f.owner(), page.clone())
        .unwrap_err()
        .0
        .contains("snapshot conflict"));
    page.page.snapshot = Some(view(&runtime, &f).digest);
    assert_eq!(
        runtime
            .query_workbench_artifacts(&f.owner(), page.clone())
            .unwrap()
            .artifacts
            .len(),
        1
    );
    page.page.limit = 0;
    assert!(runtime.query_workbench_artifacts(&f.owner(), page).is_err());
    for altered in [
        upload("one", "session:one", b"different"),
        upload("one", "session:other", b"first"),
    ] {
        assert!(runtime
            .upload_workbench_artifact(&f.owner(), altered)
            .unwrap_err()
            .0
            .contains("identity conflict"));
    }
}

#[test]
fn partial_uploads_are_readonly_legacy_until_the_original_command_repairs_metadata() {
    let f = Fixture::new();
    let runtime = f.build();
    let command = upload("partial", "session:one", b"partially saved original");
    let id = write_raw(&runtime, &f, &command, b"partially saved original");
    let before = events(&runtime);
    let legacy = view(&runtime, &f);
    assert_eq!(legacy.artifacts[0].content_ref, id);
    assert_eq!(legacy.artifacts[0].file_name, None);
    assert_eq!(legacy.artifacts[0].media_type, None);
    assert_eq!(events(&runtime), before);
    assert_eq!(
        runtime
            .services()
            .store
            .protected_content_refs(&f.owner(), &legacy.artifacts[0].scope)
            .unwrap()
            .len(),
        1
    );
    let fixed = runtime
        .upload_workbench_artifact(&f.owner(), command)
        .unwrap();
    assert_eq!(fixed.content_ref, id);
    assert_eq!(
        view(&runtime, &f).artifacts[0].file_name.as_deref(),
        Some("owner-notes.txt")
    );
}

#[test]
fn companion_is_not_downloaded_or_hydrated_and_json_upload_cannot_spoof_it() {
    let f = Fixture::new();
    let runtime = f.build();
    let receipt = runtime
        .upload_workbench_artifact(
            &f.owner(),
            upload("context", "session:one", b"unique owner artifact"),
        )
        .unwrap();
    let id = companion(&receipt.content_ref);
    assert!(runtime
        .read_workbench_artifact(&f.owner(), &id, &p::SessionId("session:one".into()))
        .unwrap_err()
        .0
        .contains("not a downloadable"));
    let context = runtime
        .services()
        .context_hydrator
        .hydrate(&ContextHydrationRequest {
            schema_version: p::SchemaVersion(1),
            run: p::RunId("run:context".into()),
            session: p::SessionId("session:one".into()),
            workspace: f.config.workspace.clone(),
            scope: p::Scope(f.config.workspace.0.clone()),
            brain_call: true,
            selected_skills: vec![],
            model: p::ModelProfileRef("model:core-b".into()),
            budget: ContextBudget {
                schema_version: p::SchemaVersion(1),
                max_tokens: 32768,
                reserve: 1024,
            },
        })
        .unwrap();
    let text = format!("{:?}", context.sources.slices);
    assert!(text.contains("unique owner artifact"));
    assert!(!text.contains("owner-notes.txt"));
    let bytes = runtime.read_protected_artifact(&f.owner(), &id).unwrap();
    let fake = runtime
        .upload_workbench_artifact(
            &f.owner(),
            upload("json-as-document", "session:one", &bytes),
        )
        .unwrap();
    assert_eq!(view(&runtime, &f).total, 2);
    assert_eq!(
        runtime
            .read_workbench_artifact(
                &f.owner(),
                &fake.content_ref,
                &p::SessionId("session:one".into())
            )
            .unwrap(),
        bytes
    );
}

#[test]
fn deletion_removes_original_and_companion_and_blocks_resurrection_after_restart() {
    let f = Fixture::new();
    let runtime = f.build();
    let command = upload("erased", "session:one", b"forget this attachment");
    let receipt = runtime
        .upload_workbench_artifact(&f.owner(), command.clone())
        .unwrap();
    let protection = runtime
        .services()
        .store
        .content_protection()
        .unwrap()
        .unwrap();
    let lifecycle = protection.lifecycle_service(runtime.services().store.as_ref().clone());
    let record = lifecycle
        .protected_content(&f.owner(), &receipt.content_ref)
        .unwrap()
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
            scope: receipt.scope.clone(),
            subject: record.subject_digest,
            operation: p::DataLifecycleOperation::Delete,
            expected_version: 0,
        },
    );
    let plan = prepared.plan.unwrap();
    assert_eq!(plan.objects, 2);
    let applied = execute(
        "erase:apply",
        p::DataWorkbenchAction::Apply {
            prepared_by: p::IdempotencyKey("erase:prepare".into()),
            plan_digest: plan.digest,
        },
    );
    assert_eq!(applied.state, p::DataCommandState::Confirmed);
    assert_eq!(view(&runtime, &f).total, 0);
    for id in [&receipt.content_ref, &companion(&receipt.content_ref)] {
        assert!(lifecycle
            .protected_content(&f.owner(), id)
            .unwrap()
            .is_none());
    }
    assert!(runtime
        .upload_workbench_artifact(&f.owner(), command.clone())
        .is_err());
    drop(lifecycle);
    drop(runtime);
    let runtime = f.build();
    assert!(runtime
        .upload_workbench_artifact(&f.owner(), command)
        .is_err());
    assert_eq!(view(&runtime, &f).total, 0);
}
