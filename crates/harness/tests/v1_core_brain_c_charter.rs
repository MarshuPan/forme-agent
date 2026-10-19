#[path = "support/core_brain.rs"]
mod support;
use forme_context::{ContextBudget, ContextHydrationRequest, ContextHydrator};
use forme_protocol as p;
use forme_store::EventStore;
use support::*;

fn charter(workspace: p::WorkspaceRef) -> p::WorkspaceCharterRecord {
    let mut record = p::WorkspaceCharterRecord {
        schema_version: p::SchemaVersion(1),
        workspace,
        version: 1,
        goals: vec![],
        constraints: vec![p::Constraint("max-nodes:6".into())],
        prohibitions: vec![p::Constraint("deny-capability:file-write".into())],
        done_contract: None,
        review_cadence: Some(p::DurationMs(60_000)),
        actor: p::Actor::Owner,
        digest: p::SchemaDigest(String::new()),
    };
    record.refresh_digest().unwrap();
    record
}

#[test]
fn c13_charter_is_owner_bound_atomic_versioned_replayable_and_in_formal_context() {
    let f = Fixture::new();
    let runtime = f.build();
    let change = runtime
        .prepare_workspace_charter(&f.owner(), charter(f.config.workspace.clone()), 0)
        .unwrap();
    let id = runtime
        .apply_workspace_charter(
            &f.owner(),
            p::IdempotencyKey("charter-1".into()),
            change.clone(),
        )
        .unwrap();
    assert_eq!(
        id,
        runtime
            .apply_workspace_charter(
                &f.owner(),
                p::IdempotencyKey("charter-1".into()),
                change.clone()
            )
            .unwrap()
    );
    let store = runtime.services().store.clone();
    let stream = runtime.services().aggregates.charter.clone();
    assert_eq!(store.run_head(&stream).unwrap(), 1);
    assert!(runtime
        .apply_workspace_charter(
            &f.owner(),
            p::IdempotencyKey("stale".into()),
            change.clone()
        )
        .is_err());
    assert!(runtime
        .apply_workspace_charter(
            &p::VerifiedPrincipal("owner:other".into()),
            p::IdempotencyKey("foreign".into()),
            change.clone()
        )
        .is_err());
    let mut bad = change.clone();
    bad.charter.workspace.0 = "workspace:other".into();
    assert!(runtime
        .apply_workspace_charter(
            &f.owner(),
            p::IdempotencyKey("changed-with-old-digest".into()),
            bad
        )
        .is_err());
    let mut bad = change.clone();
    bad.charter.constraints.clear();
    bad.charter.prohibitions.clear();
    bad.charter.refresh_digest().unwrap();
    assert!(runtime
        .apply_workspace_charter(&f.owner(), p::IdempotencyKey("empty".into()), bad)
        .is_err());
    let mut bad = store.read_run(stream.clone()).next().unwrap().unwrap();
    bad.event_id.0 = "forged".into();
    bad.provenance.actor = p::Actor::System;
    assert!(store.append(bad).is_err());
    assert_eq!(store.run_head(&stream).unwrap(), 1);
    let context = runtime
        .services()
        .context_hydrator
        .hydrate(&ContextHydrationRequest {
            schema_version: p::SchemaVersion(1),
            run: p::RunId("read-context".into()),
            session: p::SessionId("read-charter".into()),
            workspace: f.config.workspace.clone(),
            scope: p::Scope(f.config.workspace.0.clone()),
            brain_call: true,
            selected_skills: vec![],
            model: p::ModelProfileRef("model:core-b".into()),
            budget: ContextBudget {
                schema_version: p::SchemaVersion(1),
                max_tokens: 16_000,
                reserve: 1024,
            },
        })
        .unwrap();
    assert!(format!("{context:?}").contains("deny-capability:file-write"));
    assert!(runtime.services().cognition.stable_objects().is_empty());
    let before = runtime
        .workspace_charter(&f.owner(), &f.config.workspace)
        .unwrap();
    store.rebuild_charters().unwrap();
    assert_eq!(
        before,
        store.workspace_charter(&f.config.workspace).unwrap()
    );
    drop(store);
    drop(runtime);
    let runtime = f.build();
    assert_eq!(
        before,
        runtime
            .workspace_charter(&f.owner(), &f.config.workspace)
            .unwrap()
    );
    assert_eq!(runtime.services().store.run_head(&stream).unwrap(), 1);
}

#[test]
fn c13_concurrent_charter_cas_has_one_winner_and_identical_commands_are_durable() {
    let f = Fixture::new();
    let runtime = std::sync::Arc::new(f.build());
    let change = runtime
        .prepare_workspace_charter(&f.owner(), charter(f.config.workspace.clone()), 0)
        .unwrap();
    let results = std::thread::scope(|scope| {
        let handles = (0..4)
            .map(|n| {
                let runtime = runtime.clone();
                let change = change.clone();
                let owner = f.owner();
                scope.spawn(move || {
                    runtime.apply_workspace_charter(
                        &owner,
                        p::IdempotencyKey(format!("cas-{n}")),
                        change,
                    )
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        runtime
            .services()
            .store
            .run_head(&runtime.services().aggregates.charter)
            .unwrap(),
        1
    );
}
