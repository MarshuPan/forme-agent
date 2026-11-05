//! Real public HTTP/authentication boundaries without installed service claims.
use super::*;
mod legacy;
mod transport;

fn recovery() -> Value {
    let hash = p::sha256_content_digest(b"fixture");
    json!({"schema_version":1,"operation_id":"recover:http","idempotency_key":"key:http","expected_revision":28,
        "decision":"abort_before_switch","requested_deadline":now()+60_000,"password":PASSWORD,
        "candidate":{"schema_version":1,"product_revision":28,"owner":"owner:http-fixture",
            "original_operation_id":"upgrade:original","original_command_digest":hash,"original_release_ref":"0.1.0",
            "original_release_digest":hash,"config_revision":1,"config_digest":hash,"evidence_digest":hash}})
}

#[test]
fn release_recovery_http_owner_csrf_reauthentication_and_closed_input_precede_host_io() {
    let f = Fixture::new(true);
    let path = "/api/v1/product/release-recovery";
    let body = recovery();
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
        ("Idempotency-Key", "recover:http".into()),
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
    for changed in ["owner", "program", "principal", "verified", "stage"] {
        let mut wrong = body.clone();
        wrong[changed] = "forged".into();
        assert_eq!(
            f.request("POST", path, &headers, &wrong.to_string()).status,
            400
        );
    }
    let mut wrong = body.clone();
    wrong["candidate"]["owner"] = "owner:other".into();
    assert_eq!(
        f.request("POST", path, &headers, &wrong.to_string()).status,
        400
    );
    wrong = body.clone();
    wrong["expected_revision"] = 27.into();
    assert_eq!(
        f.request("POST", path, &headers, &wrong.to_string()).status,
        400
    );
    wrong = body.clone();
    wrong["schema_version"] = 2.into();
    assert_eq!(
        f.request("POST", path, &headers, &wrong.to_string()).status,
        400
    );
    let mut mismatch = headers.clone();
    mismatch[3].1 = "recover:other".into();
    assert_eq!(
        f.request("POST", path, &mismatch, &body.to_string()).status,
        400
    );
    // Valid owner password reaches only the configured Host facade; this
    // fixture intentionally has none. No model/brain/store fallback exists.
    let response = f.request("POST", path, &headers, &body.to_string());
    assert_eq!(response.status, 503);
    assert!(!response.body.contains(PASSWORD));
    for path in [
        "/api/v1/product/release-recovery/operations",
        "/api/v1/product/release-recovery/operations/recover:http",
        "/api/v1/product/release-recovery/candidates/upgrade:original",
    ] {
        assert_eq!(f.request("GET", path, &[], "").status, 401);
        assert_eq!(f.request("GET", path, &headers, "").status, 503);
    }
}

#[test]
fn release_recovery_verification_is_separate_closed_owner_action_and_plain_maintenance_cannot_adjudicate(
) {
    let f = Fixture::new(true);
    let (cookie, csrf) = f.login();
    let headers = [
        ("Origin", f.origin()),
        ("Cookie", cookie),
        ("X-Forme-Csrf", csrf),
        ("Idempotency-Key", "recover:http".into()),
    ];
    let body = json!({"schema_version":1,"operation_id":"recover:http","request_digest":p::sha256_content_digest(b"original-request"),
        "expected_revision":29,"requested_deadline":now()+60_000,"password":PASSWORD});
    let path = "/api/v1/product/release-recovery/verify";
    let mut invalid = body.clone();
    invalid["candidate"] = recovery()["candidate"].clone();
    assert_eq!(
        f.request("POST", path, &headers, &invalid.to_string())
            .status,
        400
    );
    invalid = body.clone();
    invalid["owner"] = "owner:http-fixture".into();
    assert_eq!(
        f.request("POST", path, &headers, &invalid.to_string())
            .status,
        400
    );
    assert_eq!(
        f.request("POST", path, &headers, &body.to_string()).status,
        503
    );
    let ordinary = json!({"schema_version":1,"operation_id":"recover:http","expected_revision":29,"action":{"kind":"abort_before_switch"},
        "requested_deadline":now()+60_000,"password":PASSWORD});
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/product/maintenance",
            &headers,
            &ordinary.to_string()
        )
        .status,
        400
    );
}
