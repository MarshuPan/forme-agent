//! Actual authenticated HTTP boundary for the finite MCP edit.
use super::*;
#[test]
fn managed_settings_require_csrf_reauthentication_and_server_owned_identity() {
    check_settings("mcp", "mcp_servers");
    check_settings("plugin", "plugins");
    check_settings("channel", "channels");
}
fn check_settings(kind: &str, field: &str) {
    let f = Fixture::new(true);
    let path = format!("/api/v1/product/{kind}/settings");
    let path = path.as_str();
    let body = json!({"schema_version":1,"operation_id":"settings:http","expected_revision":1,
        field:[],
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
    wrong[field] = json!([{"schema_version":1,"program":"forged"}]);
    assert_eq!(
        f.request("POST", path, &headers, &wrong.to_string()).status,
        400
    );
    let result = f.request("POST", path, &headers, &body.to_string());
    assert_eq!(result.status, 503);
    assert!(!result.body.contains(PASSWORD));
    let operation = format!("{path}/operations/settings:http");
    for path in [path, operation.as_str()] {
        assert_eq!(f.request("GET", path, &[], "").status, 401);
        assert_eq!(f.request("GET", path, &headers, "").status, 503);
    }
}
