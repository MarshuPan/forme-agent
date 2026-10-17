#[path = "support/core_brain.rs"]
mod support;
use forme_context::{ContextBudget, ContextHydrationRequest, ContextHydrator};
use forme_harness::*;
use forme_protocol as p;
use support::*;

#[test]
fn c9_owner_attention_and_formal_context_rebuild_across_workspace_restart() {
    let mut f = Fixture::new();
    let runtime = f.build();
    runtime
        .register_learning_goal(
            &f.owner(),
            p::IdempotencyKey("goal-a".into()),
            p::GoalFrameRef("goal:a".into()),
            knowledge("risk", p::Impact::High),
        )
        .unwrap();
    let query = AgentWorkspaceQuery {
        schema_version: p::SchemaVersion(1),
        scope: None,
        capacity: 10,
        now: now(),
        config_version: p::Version(1),
    };
    let first = runtime.agent_workspace(&f.owner(), &query).unwrap();
    assert_eq!(first.items.len(), 1);
    assert!(runtime
        .agent_workspace(&p::VerifiedPrincipal("other".into()), &query)
        .is_err());
    drop(runtime);
    let runtime = f.build();
    assert_eq!(first, runtime.agent_workspace(&f.owner(), &query).unwrap());
    drop(runtime);
    f.config.workspace = p::WorkspaceRef("workspace:other".into());
    let runtime = f.build();
    let mut other = knowledge("style", p::Impact::High);
    other.scope = p::Scope("workspace:other".into());
    runtime
        .register_learning_goal(
            &f.owner(),
            p::IdempotencyKey("goal-b".into()),
            p::GoalFrameRef("goal:b".into()),
            other,
        )
        .unwrap();
    let all = runtime.agent_workspace(&f.owner(), &query).unwrap();
    assert_eq!(all.items.len(), 2);
    let scoped = AgentWorkspaceQuery {
        scope: Some(p::Scope("workspace:other".into())),
        capacity: 1,
        ..query.clone()
    };
    assert_eq!(
        runtime.agent_workspace(&f.owner(), &scoped).unwrap().items[0].id,
        "goal:goal:b"
    );
    let context = runtime
        .services()
        .context_hydrator
        .hydrate(&ContextHydrationRequest {
            schema_version: p::SchemaVersion(1),
            run: p::RunId("run:new".into()),
            session: p::SessionId("session:new".into()),
            workspace: f.config.workspace.clone(),
            scope: p::Scope("workspace:other".into()),
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
    let panel = context.sources.agent_workspace.unwrap();
    assert!(panel.items.iter().any(|item| item.id == "goal:goal:b"));
    assert!(panel.items.iter().all(|item| !item.id.contains("goal:a")));
    drop(runtime);
    let runtime = f.build();
    assert_eq!(all, runtime.agent_workspace(&f.owner(), &query).unwrap());
}
