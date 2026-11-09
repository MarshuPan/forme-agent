use super::*;

fn owner() -> p::WebSessionView {
    p::WebSessionView {
        schema_version: p::SchemaVersion(1),
        account_ref: "account:fixture".into(),
        owner: p::OwnerPrincipalRef("owner:actual".into()),
        login_name: "fixture".into(),
        role: Default::default(),
        auth_epoch: 1,
        idle_expires_at: 100,
        absolute_expires_at: 200,
    }
}
fn change() -> serde_json::Value {
    let mut plan = p::CapabilityInstallPlan {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityInstallPlanRef("plan:fixture".into()),
        operation: p::CapabilityPackageOperation::Enable,
        package: p::CapabilityPackageRef("package:fixture".into()),
        release: p::CapabilityReleaseRef("release:fixture".into()),
        package_digest: p::sha256_content_digest(b"fixture"),
        admission: p::CapabilityAdmissionRef("admission:fixture".into()),
        scope: p::Scope("workspace:fixture".into()),
        policy: p::CapabilityPolicyRef("policy:fixture".into()),
        policy_version: p::Version(1),
        expected_version: p::EcosystemAggregateVersion::zero(),
        previous_release: None,
        rollback_boundary: p::RollbackBoundary("fixture boundary".into()),
        digest: p::PlanDigest(String::new()),
    };
    plan.refresh_digest().unwrap();
    serde_json::json!({"schema_version":1,"run":"run:fixture","plan":plan,
        "approval":{"schema_version":1,"approval":"approval:fixture","plan_digest":plan.digest,"nonce":"nonce:fixture","expires_at":100},
        "reason":"reason:fixture"})
}
#[test]
fn package_approval_uses_authenticated_owner_and_exact_original_plan_and_nonce() {
    let request = change();
    let encoded = serde_json::to_vec(&request).unwrap();
    let result = body(
        "POST",
        "ecosystem/changes",
        &encoded,
        &owner(),
        Some("nonce:fixture"),
    )
    .unwrap();
    let bound: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(bound["approval"]["principal"], "owner:actual");
    assert_eq!(bound["plan"], request["plan"]);
    for case in 0..5 {
        let mut wrong = request.clone();
        match case {
            0 => wrong["approval"]["principal"] = "owner:forged".into(),
            1 => wrong["plan"]["scope"] = "workspace:other".into(),
            2 => wrong["approval"]["plan_digest"] = "sha256:other".into(),
            3 => wrong["schema_version"] = 2.into(),
            _ => wrong["approval"]["nonce"] = "nonce:other".into(),
        }
        assert!(body(
            "POST",
            "ecosystem/changes",
            &serde_json::to_vec(&wrong).unwrap(),
            &owner(),
            Some("nonce:fixture")
        )
        .is_err());
    }
}
#[test]
fn ecosystem_routes_are_finite_and_scope_cannot_be_a_second_query_or_path() {
    assert_eq!(
        path(
            "GET",
            "ecosystem/snapshot",
            Some("scope=workspace%3Afixture")
        )
        .unwrap(),
        "/v1/ecosystem/snapshot?scope=workspace:fixture"
    );
    for (method, route, query) in [
        ("POST", "ecosystem/unknown", None),
        ("GET", "ecosystem/changes", None),
        ("POST", "ecosystem/changes/extra", None),
        ("POST", "ecosystem/plans", Some("scope=workspace:fixture")),
        (
            "GET",
            "ecosystem/snapshot",
            Some("scope=workspace:fixture&scope=workspace:other"),
        ),
        ("GET", "ecosystem/snapshot", Some("scope=../keys")),
        ("GET", "ecosystem/snapshot", None),
    ] {
        assert!(path(method, route, query).is_err());
    }
}
