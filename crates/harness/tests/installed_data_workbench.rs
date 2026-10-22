#[path = "support/core_brain.rs"]
mod support;
use forme_harness::ProductionRuntime;
use forme_protocol as p;
use support::*;

fn command(key: &str, action: p::DataWorkbenchAction) -> p::DataWorkbenchCommand {
    p::DataWorkbenchCommand {
        schema_version: p::SchemaVersion(1),
        command: p::IdempotencyKey(key.into()),
        action,
    }
}
fn page() -> p::DataPageQuery {
    p::DataPageQuery {
        schema_version: p::SchemaVersion(1),
        offset: 0,
        limit: 100,
        snapshot: None,
    }
}
fn query(
    runtime: &ProductionRuntime,
    f: &Fixture,
    query: p::DataWorkbenchRead,
) -> p::DataWorkbenchView {
    runtime
        .query_data_workbench(
            &f.owner(),
            p::DataWorkbenchQuery {
                schema_version: p::SchemaVersion(1),
                query,
            },
        )
        .unwrap()
}
fn catalog(runtime: &ProductionRuntime, f: &Fixture) -> p::DataCatalogView {
    let p::DataWorkbenchView::Catalog(value) =
        query(runtime, f, p::DataWorkbenchRead::Catalog { page: page() })
    else {
        panic!("catalog")
    };
    value
}
fn seed(runtime: &ProductionRuntime, f: &Fixture, scope: &str, text: &str) -> p::ContentRef {
    let protection = runtime
        .services()
        .store
        .content_protection()
        .unwrap()
        .unwrap();
    let (_, record) = protection
        .seal_record(
            text.as_bytes(),
            &p::Scope(scope.into()),
            &format!("fixture:{text}"),
            &p::sha256_content_digest(text.as_bytes()),
        )
        .unwrap();
    let id = record.content_ref.clone();
    protection
        .lifecycle_service(runtime.services().store.as_ref().clone())
        .put_protected_content(&f.owner(), record)
        .unwrap();
    id
}
fn execute(
    runtime: &ProductionRuntime,
    f: &Fixture,
    request: p::DataWorkbenchCommand,
) -> p::DataCommandReceipt {
    runtime.execute_data_workbench(&f.owner(), request).unwrap()
}

#[test]
fn catalog_is_readonly_owner_bound_paginated_and_hides_storage_locators() {
    let f = Fixture::new();
    let runtime = f.build();
    seed(&runtime, &f, "workspace:alpha", "private original");
    let before = events(&runtime);
    let view = catalog(&runtime, &f);
    assert_eq!(
        view.subjects
            .iter()
            .find(|s| s.scope.0 == "workspace:alpha")
            .unwrap()
            .objects,
        1
    );
    let encoded = serde_json::to_string(&view).unwrap();
    for forbidden in [
        "private original",
        "protected_content",
        "table_name",
        "row_key",
        "key_digest",
    ] {
        assert!(!encoded.contains(forbidden));
    }
    assert_eq!(events(&runtime), before);
    assert!(runtime
        .query_data_workbench(
            &p::VerifiedPrincipal("foreign-owner".into()),
            p::DataWorkbenchQuery {
                schema_version: p::SchemaVersion(1),
                query: p::DataWorkbenchRead::Catalog { page: page() }
            }
        )
        .is_err());
    seed(&runtime, &f, "workspace:beta", "second subject");
    let mut next = page();
    next.offset = 1;
    next.snapshot = Some(view.digest);
    assert!(runtime
        .query_data_workbench(
            &f.owner(),
            p::DataWorkbenchQuery {
                schema_version: p::SchemaVersion(1),
                query: p::DataWorkbenchRead::Catalog { page: next }
            }
        )
        .unwrap_err()
        .0
        .contains("snapshot changed"));
}

#[test]
fn backup_commands_are_immutable_durable_and_verification_is_readonly() {
    let f = Fixture::new();
    let runtime = f.build();
    seed(&runtime, &f, "workspace:alpha", "backup private original");
    let request = command(
        "backup:one",
        p::DataWorkbenchAction::Backup {
            target: "owner-one.sqlite".into(),
        },
    );
    let receipt = execute(&runtime, &f, request.clone());
    assert_eq!(receipt.state, p::DataCommandState::Confirmed);
    assert!(!receipt.backup.as_ref().unwrap().verified);
    let before = events(&runtime);
    assert_eq!(execute(&runtime, &f, request.clone()), receipt);
    let p::DataWorkbenchView::Verification(verified) = query(
        &runtime,
        &f,
        p::DataWorkbenchRead::VerifyBackup {
            source: "owner-one.sqlite".into(),
        },
    ) else {
        panic!("verification")
    };
    assert!(verified.verified);
    assert_eq!(verified.digest, receipt.backup.as_ref().unwrap().digest);
    assert_eq!(events(&runtime), before);
    assert!(runtime
        .execute_data_workbench(
            &f.owner(),
            command(
                "backup:one",
                p::DataWorkbenchAction::Backup {
                    target: "different.sqlite".into()
                }
            )
        )
        .unwrap_err()
        .0
        .contains("identity conflict"));
    drop(runtime);
    let runtime = f.build();
    assert_eq!(execute(&runtime, &f, request), receipt);
    let p::DataWorkbenchView::Operations(list) = query(
        &runtime,
        &f,
        p::DataWorkbenchRead::Operations { page: page() },
    ) else {
        panic!("operations")
    };
    assert_eq!(list.operations, vec![receipt]);
    let subjects = catalog(&runtime, &f).subjects;
    assert!(subjects.iter().any(|s| s.scope.0 == "workspace:alpha"));
    assert!(subjects
        .iter()
        .all(|s| !s.scope.0.starts_with("maintenance:data-workbench")));
    let raw = std::fs::read(&f.config.store_path).unwrap();
    assert!(!raw
        .windows(b"backup private original".len())
        .any(|v| v == b"backup private original"));
}

#[test]
fn restore_uses_the_private_original_plan_once_and_never_switches_the_brain() {
    let f = Fixture::new();
    let runtime = f.build();
    execute(
        &runtime,
        &f,
        command(
            "backup:restore",
            p::DataWorkbenchAction::Backup {
                target: "source.sqlite".into(),
            },
        ),
    );
    let prepared = execute(
        &runtime,
        &f,
        command(
            "prepare:restore",
            p::DataWorkbenchAction::PrepareRestore {
                source: "source.sqlite".into(),
                target: "new-restored.sqlite".into(),
            },
        ),
    );
    assert_eq!(prepared.state, p::DataCommandState::Prepared);
    let plan = prepared.plan.unwrap();
    assert!(!plan.irreversible);
    let encoded = serde_json::to_string(&plan).unwrap();
    for field in ["row_key", "objects\":[", "nonce", "key_digest"] {
        assert!(!encoded.contains(field));
    }
    let authority = runtime.services().authority.clone();
    let forged = command(
        "apply:forged",
        p::DataWorkbenchAction::Apply {
            prepared_by: p::IdempotencyKey("prepare:restore".into()),
            plan_digest: p::sha256_content_digest(b"wrong"),
        },
    );
    assert!(runtime.execute_data_workbench(&f.owner(), forged).is_err());
    let request = command(
        "apply:restore",
        p::DataWorkbenchAction::Apply {
            prepared_by: p::IdempotencyKey("prepare:restore".into()),
            plan_digest: plan.digest.clone(),
        },
    );
    let receipt = execute(&runtime, &f, request.clone());
    assert_eq!(receipt.state, p::DataCommandState::Confirmed);
    assert!(receipt.backup.as_ref().unwrap().verified);
    assert_eq!(
        runtime.services().authority.owner_digest,
        authority.owner_digest
    );
    assert_eq!(runtime.config().store_path, f.config.store_path);
    let second = command("apply:another-key", request.action.clone());
    assert!(runtime
        .execute_data_workbench(&f.owner(), second)
        .unwrap_err()
        .0
        .contains("already has an application"));
    assert_eq!(execute(&runtime, &f, request.clone()), receipt);
    drop(runtime);
    let runtime = f.build();
    assert_eq!(execute(&runtime, &f, request), receipt);
}

#[test]
fn lifecycle_preview_is_safe_and_delete_clears_real_content_with_honest_receipt() {
    let f = Fixture::new();
    let runtime = f.build();
    let id = seed(&runtime, &f, "workspace:erase", "erase this source");
    let view = catalog(&runtime, &f);
    let subject = view
        .subjects
        .iter()
        .find(|s| s.scope.0 == "workspace:erase")
        .unwrap();
    let prepared = execute(
        &runtime,
        &f,
        command(
            "prepare:erase",
            p::DataWorkbenchAction::PrepareLifecycle {
                scope: subject.scope.clone(),
                subject: subject.subject.clone(),
                operation: p::DataLifecycleOperation::Delete,
                expected_version: view.version,
            },
        ),
    );
    let plan = prepared.plan.unwrap();
    assert!(plan.irreversible);
    assert_eq!(plan.objects, 1);
    assert_eq!(
        runtime.read_protected_artifact(&f.owner(), &id).unwrap(),
        b"erase this source"
    );
    let applied = execute(
        &runtime,
        &f,
        command(
            "apply:erase",
            p::DataWorkbenchAction::Apply {
                prepared_by: p::IdempotencyKey("prepare:erase".into()),
                plan_digest: plan.digest,
            },
        ),
    );
    assert_eq!(applied.state, p::DataCommandState::Confirmed);
    assert_eq!(
        applied.lifecycle.as_ref().unwrap().remote,
        p::RemoteDeletionDisposition::Unknown
    );
    assert!(runtime.read_protected_artifact(&f.owner(), &id).is_err());
    let view = catalog(&runtime, &f);
    assert_eq!(view.version, 1);
    assert_eq!(
        view.subjects
            .iter()
            .find(|s| s.scope.0 == "workspace:erase")
            .unwrap()
            .deleted_objects,
        1
    );
    drop(runtime);
    let runtime = f.build();
    let p::DataWorkbenchView::Receipt(view) = query(
        &runtime,
        &f,
        p::DataWorkbenchRead::Inspect {
            command: p::IdempotencyKey("apply:erase".into()),
        },
    ) else {
        panic!("receipt")
    };
    assert_eq!(view.receipt, Some(applied));
}

#[test]
fn interrupted_data_commands_probe_the_original_receipt_without_replaying_effects() {
    let f = Fixture::new();
    let runtime = f.build();
    let request = command(
        "backup:interrupted",
        p::DataWorkbenchAction::Backup {
            target: "interrupted.sqlite".into(),
        },
    );
    let receipt = p::DataCommandReceipt {
        schema_version: p::SchemaVersion(1),
        command: request.command.clone(),
        binding: p::canonical_digest(&(&f.owner(), &request)).unwrap(),
        action: "Backup".into(),
        prepared_by: None,
        state: p::DataCommandState::Unknown,
        plan: None,
        backup: None,
        lifecycle: None,
    };
    let record =
        serde_json::json!({"schema_version":1,"request":request,"receipt":receipt,"plan":null});
    let object = format!(
        "data-workbench:{}:started",
        p::canonical_digest(&(&f.owner(), &request.command))
            .unwrap()
            .0
    );
    let protection = runtime
        .services()
        .store
        .content_protection()
        .unwrap()
        .unwrap();
    let (_, sealed) = protection
        .seal_record(
            &serde_json::to_vec(&record).unwrap(),
            &p::Scope("maintenance:data-workbench/started".into()),
            &object,
            &receipt.binding,
        )
        .unwrap();
    protection
        .lifecycle_service(runtime.services().store.as_ref().clone())
        .put_protected_content(&f.owner(), sealed)
        .unwrap();
    let before = events(&runtime);
    assert_eq!(execute(&runtime, &f, request.clone()), receipt);
    assert_eq!(events(&runtime), before);
    assert!(!f
        .config
        .store_path
        .parent()
        .unwrap()
        .join("data-operations/interrupted.sqlite")
        .exists());
    drop(runtime);
    let runtime = f.build();
    assert_eq!(execute(&runtime, &f, request), receipt);
}

#[test]
fn web_data_contract_rejects_paths_unknown_fields_and_internal_scopes_before_writes() {
    let f = Fixture::new();
    let runtime = f.build();
    let before = events(&runtime);
    for target in [
        "../escape.sqlite",
        "C:\\private.sqlite",
        "CON",
        "backup.",
        "a/b",
        "NUL.txt",
    ] {
        assert!(runtime
            .execute_data_workbench(
                &f.owner(),
                command(
                    "bad:backup",
                    p::DataWorkbenchAction::Backup {
                        target: target.into()
                    }
                )
            )
            .is_err());
    }
    let raw = serde_json::json!({"schema_version":1,"command":"forged","action":{"Backup":{"target":"valid.sqlite","actor":"Owner"}}});
    assert!(serde_json::from_value::<p::DataWorkbenchCommand>(raw).is_err());
    assert_eq!(events(&runtime), before);
}
