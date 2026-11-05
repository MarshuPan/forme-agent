//! Real owner HTTP boundaries; no installed services or native review authority.
use super::*;
mod transport;

const PREFIX: &str = "/api/v1/product/legacy-release-recovery";
fn body() -> Value {
    json!({"schema_version":1,"operation_id":"legacy:http","idempotency_key":"legacy:http",
        "expected_revision":28,"review_id":"review:http","review_digest":p::sha256_content_digest(b"review"),
        "requested_deadline":now()+60_000,"password":PASSWORD})
}
fn verification() -> Value {
    json!({"schema_version":1,"operation_id":"legacy:http","request_digest":p::sha256_content_digest(b"request"),
        "expected_revision":29,"requested_deadline":now()+60_000,"password":PASSWORD})
}
fn headers(f: &Fixture) -> [(&'static str, String); 4] {
    let (cookie, csrf) = f.login();
    [
        ("Origin", f.origin()),
        ("Cookie", cookie),
        ("X-Forme-Csrf", csrf),
        ("Idempotency-Key", "legacy:http".into()),
    ]
}

#[test]
fn legacy_http_requires_owner_csrf_closed_input_and_bound_password_before_host_io() {
    let f = Fixture::new(true);
    let headers = headers(&f);
    for (path, body) in [
        (PREFIX.into(), body()),
        (format!("{PREFIX}/verify"), verification()),
    ] {
        assert_eq!(
            f.request("POST", &path, &[("Origin", f.origin())], &body.to_string())
                .status,
            401
        );
        let no_csrf = [headers[0].clone(), headers[1].clone(), headers[3].clone()];
        assert_eq!(
            f.request("POST", &path, &no_csrf, &body.to_string()).status,
            403
        );
        let mut foreign_origin = headers.clone();
        foreign_origin[0].1 = "https://foreign.invalid".into();
        assert_eq!(
            f.request("POST", &path, &foreign_origin, &body.to_string())
                .status,
            403
        );
        let mut wrong_key = headers.clone();
        wrong_key[3].1 = "legacy:other".into();
        assert_eq!(
            f.request("POST", &path, &wrong_key, &body.to_string())
                .status,
            400
        );
        for field in [
            "owner",
            "principal",
            "path",
            "report",
            "evidence",
            "public_key",
            "verified",
            "program",
        ] {
            let mut forged = body.clone();
            forged[field] = "caller-cannot-certify".into();
            assert_eq!(
                f.request("POST", &path, &headers, &forged.to_string())
                    .status,
                400,
                "{field}"
            );
        }
        let mut wrong = body.clone();
        wrong["password"] = "incorrect-test-password".into();
        assert_eq!(
            f.request("POST", &path, &headers, &wrong.to_string())
                .status,
            401
        );
        wrong = body.clone();
        wrong["schema_version"] = 2.into();
        assert_eq!(
            f.request("POST", &path, &headers, &wrong.to_string())
                .status,
            400
        );
        let response = f.request("POST", &path, &headers, &body.to_string());
        assert_eq!(response.status, 503);
        assert!(!response.body.contains(PASSWORD));
    }
    for suffix in [
        "/review/upgrade:original",
        "/operations",
        "/operations/legacy:http",
    ] {
        let path = format!("{PREFIX}{suffix}");
        assert_eq!(f.request("GET", &path, &[], "").status, 401);
        assert_eq!(f.request("GET", &path, &headers, "").status, 503);
    }
    // There is no route/body fallback into ordinary recovery or maintenance.
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/product/release-recovery",
            &headers,
            &body().to_string()
        )
        .status,
        400
    );
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/product/maintenance",
            &headers,
            &body().to_string()
        )
        .status,
        400
    );
}
