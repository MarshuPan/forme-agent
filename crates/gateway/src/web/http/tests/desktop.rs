//! Real HTTP rejection checks before any Host session selection or grant.
use super::*;
#[test]
fn desktop_controls_require_owner_csrf_exact_operation_and_password_before_host_io() {
    let f = Fixture::new(true);
    let path = "/api/v1/desktop/consents";
    let body = json!({"schema_version":1,"password":PASSWORD,"request":{
        "schema_version":1,"operation_id":"consent:http","expected_revision":2,
        "pending_ref":"pending:original","catalog_revision":2,"catalog_digest":p::sha256_content_digest(b"catalog"),
        "session_ref":"session:opaque","expires_at":now()+60_000}});
    assert_eq!(
        f.request("POST", path, &[("Origin", f.origin())], &body.to_string())
            .status,
        401
    );
    let (cookie, csrf) = f.login();
    let headers = [
        ("Origin", f.origin()),
        ("Cookie", cookie),
        ("X-Forme-Csrf", csrf),
        ("Idempotency-Key", "consent:http".into()),
    ];
    let no_csrf = [headers[0].clone(), headers[1].clone(), headers[3].clone()];
    assert_eq!(
        f.request("POST", path, &no_csrf, &body.to_string()).status,
        403
    );
    let mut wrong = body.clone();
    wrong["password"] = "wrong-fixture-password".into();
    assert_eq!(
        f.request("POST", path, &headers, &wrong.to_string()).status,
        401
    );
    for field in [
        "owner",
        "principal",
        "os_user_ref",
        "session_generation",
        "grant_ref",
    ] {
        let mut wrong = body.clone();
        wrong["request"][field] = "forged".into();
        assert_eq!(
            f.request("POST", path, &headers, &wrong.to_string()).status,
            400
        );
    }
    for index in 0..3 {
        let mut wrong = body.clone();
        match index {
            0 => wrong["request"]["operation_id"] = "consent:other".into(),
            1 => wrong["request"]["schema_version"] = 2.into(),
            _ => wrong["schema_version"] = 2.into(),
        };
        assert_eq!(
            f.request("POST", path, &headers, &wrong.to_string()).status,
            400
        );
    }
    let response = f.request("POST", path, &headers, &body.to_string());
    assert_eq!(response.status, 503);
    assert!(!response.body.contains(PASSWORD));
    for path in [
        "/api/v1/desktop/sessions",
        "/api/v1/desktop/consents/consent:http",
    ] {
        assert_eq!(f.request("GET", path, &[], "").status, 401);
        assert_eq!(f.request("GET", path, &headers, "").status, 503);
    }
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/desktop/grants",
            &headers,
            &body.to_string()
        )
        .status,
        404
    );
}

#[test]
fn desktop_settings_require_csrf_reauthentication_and_server_owned_identity() {
    let f = Fixture::new(true);
    let path = "/api/v1/desktop/settings";
    let body = json!({"schema_version":1,"operation_id":"settings:http","expected_revision":1,
        "computer":{"schema_version":1,"enabled":true,"scope":"workspace:default","surface_refs":["surface:windows-default"]},
        "password":PASSWORD});
    assert_eq!(
        f.request("POST", path, &[("Origin", f.origin())], &body.to_string())
            .status,
        401
    );
    let (cookie, csrf) = f.login();
    let headers = [
        ("Origin", f.origin()),
        ("Cookie", cookie),
        ("X-Forme-Csrf", csrf),
        ("Idempotency-Key", "settings:http".into()),
    ];
    let no_csrf = [headers[0].clone(), headers[1].clone(), headers[3].clone()];
    assert_eq!(
        f.request("POST", path, &no_csrf, &body.to_string()).status,
        403
    );
    let mut wrong = body.clone();
    wrong["password"] = "incorrect-password".into();
    assert_eq!(
        f.request("POST", path, &headers, &wrong.to_string()).status,
        401
    );
    for field in ["owner", "principal", "publisher_keys", "models"] {
        let mut wrong = body.clone();
        wrong[field] = "forged".into();
        assert_eq!(
            f.request("POST", path, &headers, &wrong.to_string()).status,
            400
        );
    }
    let mut wrong = body.clone();
    wrong["operation_id"] = "settings:other".into();
    assert_eq!(
        f.request("POST", path, &headers, &wrong.to_string()).status,
        400
    );
    let mut wrong = body.clone();
    wrong["computer"]["surface_refs"] = json!([]);
    assert_eq!(
        f.request("POST", path, &headers, &wrong.to_string()).status,
        400
    );
    let result = f.request("POST", path, &headers, &body.to_string());
    assert_eq!(result.status, 503);
    assert!(!result.body.contains(PASSWORD));
    for path in [path, "/api/v1/desktop/settings/operations/settings:http"] {
        assert_eq!(f.request("GET", path, &[], "").status, 401);
        assert_eq!(f.request("GET", path, &headers, "").status, 503);
    }
}
