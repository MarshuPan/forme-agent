//! Actual login, CSRF and signed transport to the single production owner brain.
use super::*;

pub(super) async fn verify(
    address: SocketAddr,
    runtime: &ProductionRuntime,
    cookie: &str,
    csrf: &str,
) {
    let owner = p::VerifiedPrincipal(runtime.owner().0.clone());
    let workspace = runtime.config().workspace.clone();
    if runtime
        .workspace_charter(&owner, &workspace)
        .unwrap()
        .is_none()
    {
        let record = p::WorkspaceCharterRecord {
            schema_version: p::SchemaVersion(1),
            workspace: workspace.clone(),
            version: 0,
            goals: vec![],
            constraints: vec![p::Constraint("max-nodes:8".into())],
            prohibitions: vec![],
            done_contract: None,
            review_cadence: None,
            actor: p::Actor::Owner,
            digest: p::SchemaDigest(String::new()),
        };
        let change = runtime
            .prepare_workspace_charter(&owner, record, 0)
            .unwrap();
        runtime
            .apply_workspace_charter(
                &owner,
                p::IdempotencyKey("web-project-charter".into()),
                change,
            )
            .unwrap();
    }
    let project = p::WorkspaceProject {
        schema_version: p::SchemaVersion(1),
        reference: "project:web-control".into(),
        workspace: workspace.clone(),
        goal: p::GoalRef("goal:web-control".into()),
        done: p::DoneContractRef("done:web-control".into()),
        summary: "review the fixture project".into(),
        signature: p::TaskSignature {
            schema_version: p::SchemaVersion(1),
            partition: p::TaskPartition::Whole,
            dependency: p::TaskDependency::Independent,
            verification: p::TaskVerification::SelfCheck,
            clarity: p::TaskClarity::Clear,
            extent: p::TaskExtent::Single,
            reversibility: p::TaskReversibility::LowRiskReversible,
            anchors: vec![],
        },
        requirements: vec![p::RouteRequirement::ActionSucceeded],
        scope: p::Scope(workspace.0.clone()),
        capabilities: vec![],
        model: p::ModelProfileRef(runtime.config().models[0].id.clone()),
        budget_units: 8,
        node_units: 2,
        cadence_ms: 60_000,
        deadline: None,
    };
    runtime
        .register_project(
            &owner,
            p::IdempotencyKey("register-web-project".into()),
            project.clone(),
        )
        .unwrap();
    let stream = runtime.services().aggregates.charter.clone();
    let before = runtime.services().store.run_head(&stream).unwrap();
    let status_path = "/api/v1/brain/projects/status";
    assert_eq!(
        request(address, "GET", status_path, vec![], String::new())
            .await
            .status,
        401
    );
    let status = request(
        address,
        "GET",
        status_path,
        vec![("Cookie", cookie.into())],
        String::new(),
    )
    .await;
    assert_eq!(status.status, 200, "{}", status.body);
    let snapshot: p::WorkspaceControlSnapshot = serde_json::from_str(&status.body).unwrap();
    let view = snapshot
        .projects
        .iter()
        .find(|v| v.project == project.reference)
        .unwrap();
    assert_eq!(view.project_digest, p::canonical_digest(&project).unwrap());
    assert_eq!(view.supervision.version, 0);
    assert_eq!(view.review, p::ProjectReviewStatus::Due);
    let command = p::ProjectSupervisionCommand {
        schema_version: p::SchemaVersion(1),
        command: p::IdempotencyKey("supervision:web-pause".into()),
        project: project.reference,
        expected_project_digest: view.project_digest.clone(),
        expected_version: 0,
        enabled: false,
    };
    let path = "/api/v1/brain/projects/supervision";
    for (key, field, expected) in [
        ("wrong:key", None, 400),
        ("supervision:web-pause", Some("actor"), 400),
    ] {
        let mut body = serde_json::to_value(&command).unwrap();
        if let Some(field) = field {
            body[field] = json!("Owner");
        }
        let denied = request(
            address,
            "POST",
            path,
            mutation_headers(address, cookie, csrf, key),
            body.to_string(),
        )
        .await;
        assert_eq!(denied.status, expected, "{}", denied.body);
    }
    assert_eq!(
        request(
            address,
            "POST",
            path,
            mutation_headers(address, cookie, "wrong", &command.command.0),
            serde_json::to_string(&command).unwrap()
        )
        .await
        .status,
        403
    );
    assert_eq!(runtime.services().store.run_head(&stream).unwrap(), before);
    let applied = request(
        address,
        "POST",
        path,
        mutation_headers(address, cookie, csrf, &command.command.0),
        serde_json::to_string(&command).unwrap(),
    )
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["status"], "Confirmed");
    assert_eq!(
        runtime.services().store.run_head(&stream).unwrap(),
        before + 1
    );
    let repeated = request(
        address,
        "POST",
        path,
        mutation_headers(address, cookie, csrf, &command.command.0),
        serde_json::to_string(&command).unwrap(),
    )
    .await;
    assert_eq!(repeated.status, 200, "{}", repeated.body);
    assert_eq!(repeated.json(), applied.json());
    let mut stale = command.clone();
    stale.command = p::IdempotencyKey("supervision:stale".into());
    stale.enabled = true;
    let denied = request(
        address,
        "POST",
        path,
        mutation_headers(address, cookie, csrf, &stale.command.0),
        serde_json::to_string(&stale).unwrap(),
    )
    .await;
    assert_eq!(denied.status, 409, "{}", denied.body);
    let status = request(
        address,
        "GET",
        status_path,
        vec![("Cookie", cookie.into())],
        String::new(),
    )
    .await;
    assert_eq!(status.status, 200, "{}", status.body);
    assert_eq!(status.json()["projects"][0]["review"], "Paused");
    assert_eq!(
        runtime.services().store.run_head(&stream).unwrap(),
        before + 1
    );
    assert!(runtime
        .orchestration_state(&owner)
        .unwrap()
        .routes
        .is_empty());
}
