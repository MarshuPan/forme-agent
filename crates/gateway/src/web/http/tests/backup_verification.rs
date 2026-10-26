//! The offline read has its own exact route and still requires owner + CSRF.
use super::*;

#[test]
fn offline_verification_requires_authenticated_owner_and_csrf_before_configuration_io() {
    let fixture = Fixture::new(true);
    let path = "/api/v1/product/data/verify";
    let body = json!({"schema_version":1,"source":"backup.sqlite","config_revision":1}).to_string();
    assert_eq!(
        fixture
            .request("POST", path, &[("Origin", fixture.origin())], &body)
            .status,
        401
    );
    let (cookie, csrf) = fixture.login();
    let without_csrf = [
        ("Origin", fixture.origin()),
        ("Cookie", cookie.clone()),
        ("Idempotency-Key", "query:offline".into()),
    ];
    assert_eq!(
        fixture.request("POST", path, &without_csrf, &body).status,
        403
    );
    let wrong_csrf = [
        ("Origin", fixture.origin()),
        ("Cookie", cookie.clone()),
        ("X-Forme-Csrf", "0".repeat(64)),
        ("Idempotency-Key", "query:offline".into()),
    ];
    assert_eq!(
        fixture.request("POST", path, &wrong_csrf, &body).status,
        403
    );
    let headers = [
        ("Origin", fixture.origin()),
        ("Cookie", cookie),
        ("X-Forme-Csrf", csrf),
        ("Idempotency-Key", "query:offline".into()),
    ];
    // No configuration bridge in this fixture, so no worker or empty brain can start.
    assert_eq!(fixture.request("POST", path, &headers, &body).status, 503);
    for invalid in [
        json!({"schema_version":1,"source":"../backup.sqlite","config_revision":1}),
        json!({"schema_version":1,"source":"backup.sqlite","config_revision":0}),
        json!({"schema_version":1,"source":"backup.sqlite","config_revision":1,"owner":"owner:other"}),
    ] {
        assert_eq!(
            fixture
                .request("POST", path, &headers, &invalid.to_string())
                .status,
            400
        );
    }
    // Existing online query never falls through to installed maintenance.
    let online = json!({"schema_version":1,"query":{"VerifyBackup":{"source":"backup.sqlite"}}});
    assert_eq!(
        fixture
            .request("POST", "/api/v1/data/query", &headers, &online.to_string())
            .status,
        503
    );
}
