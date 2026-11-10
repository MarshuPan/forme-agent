//! Actual HTTP rejection and forwarding checks; no native installation claim.
use super::*;
mod bridge;

fn command() -> p::ManagedMcpProgramCommand {
    p::ManagedMcpProgramCommand {
        schema_version: p::SchemaVersion(1),
        installation_ref: "installation:http-fixture".into(),
        owner_ref: p::OwnerPrincipalRef("owner:http-fixture".into()),
        authority_epoch: p::AuthorityEpoch(3),
        operation_id: "register:http".into(),
        expected_revision: 1,
        requested_deadline: now() + 60_000,
        action: p::ManagedMcpProgramAction::Register {
            source_ref: "source:http".into(),
            source_digest: p::sha256_content_digest(b"fixture-source"),
        },
    }
}

#[test]
fn mcp_program_http_requires_owner_csrf_exact_command_and_password_before_host_io() {
    let f = Fixture::new(true);
    let path = "/api/v1/product/mcp/programs/commands";
    let body = json!({"schema_version":1,"password":PASSWORD,"command":command()});
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
        ("Idempotency-Key", "register:http".into()),
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
    for index in 0..8 {
        let mut wrong = body.clone();
        match index {
            0 => wrong["command"]["owner_ref"] = "owner:forged".into(),
            1 => wrong["command"]["operation_id"] = "register:other".into(),
            2 => wrong["schema_version"] = 2.into(),
            3 => wrong["command"]["schema_version"] = 2.into(),
            4 => wrong["command"]["action"]["program"] = "C:/injected.exe".into(),
            5 => wrong["command"]["action"]["args"] = json!(["--injected"]),
            6 => wrong["command"]["action"]["credential"] = "injected".into(),
            _ => wrong["command"]["action"]["kind"] = "execute".into(),
        }
        assert_eq!(
            f.request("POST", path, &headers, &wrong.to_string()).status,
            400,
            "case {index}"
        );
    }
    let response = f.request("POST", path, &headers, &body.to_string());
    assert_eq!(response.status, 503);
    assert!(!response.body.contains(PASSWORD));
    for path in [
        "/api/v1/product/mcp/programs",
        "/api/v1/product/mcp/programs/operations/register:http",
    ] {
        assert_eq!(f.request("GET", path, &[], "").status, 401);
        assert_eq!(f.request("GET", path, &headers, "").status, 503);
    }
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/product/mcp/programs/execute",
            &headers,
            &body.to_string()
        )
        .status,
        404
    );
}
