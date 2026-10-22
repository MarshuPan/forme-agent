//! Real HTTP data maintenance; only temporary fixture data is changed.
use super::*;

async fn query(address: SocketAddr, cookie: &str, csrf: &str, key: &str, query: Value) -> Reply {
    request(
        address,
        "POST",
        "/api/v1/data/query",
        mutation_headers(address, cookie, csrf, key),
        json!({"schema_version":1,"query":query}).to_string(),
    )
    .await
}
async fn command(
    address: SocketAddr,
    cookie: &str,
    csrf: &str,
    key: &str,
    action: Value,
    password: &str,
) -> Reply {
    request(
        address,
        "POST",
        "/api/v1/data/command",
        mutation_headers(address, cookie, csrf, key),
        json!({"command":{"schema_version":1,"command":key,"action":action},"password":password})
            .to_string(),
    )
    .await
}
pub(super) async fn verify(address: SocketAddr, cookie: &str, csrf: &str) {
    let catalog =
        json!({"Catalog":{"page":{"schema_version":1,"offset":0,"limit":50,"snapshot":null}}});
    assert_eq!(
        query(address, cookie, "wrong", "data:csrf", catalog.clone())
            .await
            .status,
        403
    );
    let view = query(address, cookie, csrf, "data:catalog", catalog).await;
    assert_eq!(view.status, 200, "{}", view.body);
    assert_eq!(view.json()["kind"], "Catalog");
    for field in ["table_name", "row_key", "key_digest", "ciphertext"] {
        assert!(!view.body.contains(field));
    }
    let backup = json!({"Backup":{"target":"web-backup.sqlite"}});
    let denied = command(
        address,
        cookie,
        csrf,
        "data:denied",
        backup.clone(),
        "wrong password",
    )
    .await;
    assert_eq!(denied.status, 401);
    let probe = query(
        address,
        cookie,
        csrf,
        "data:denied-probe",
        json!({"Inspect":{"command":"data:denied"}}),
    )
    .await;
    assert_eq!(probe.status, 200);
    assert!(probe.json()["receipt"].is_null());
    let accepted = command(
        address,
        cookie,
        csrf,
        "data:backup",
        backup.clone(),
        PASSWORD,
    )
    .await;
    assert_eq!(accepted.status, 200, "{}", accepted.body);
    assert_eq!(accepted.json()["state"], "Confirmed");
    let repeated = command(address, cookie, csrf, "data:backup", backup, PASSWORD).await;
    assert_eq!(repeated.json(), accepted.json());
    let checked = query(
        address,
        cookie,
        csrf,
        "data:verify",
        json!({"VerifyBackup":{"source":"web-backup.sqlite"}}),
    )
    .await;
    assert_eq!(checked.status, 200, "{}", checked.body);
    assert_eq!(checked.json()["verified"], true);
    let prepared = command(
        address,
        cookie,
        csrf,
        "data:prepare",
        json!({"PrepareRestore":{"source":"web-backup.sqlite","target":"web-restored.sqlite"}}),
        PASSWORD,
    )
    .await;
    assert_eq!(prepared.status, 200, "{}", prepared.body);
    assert_eq!(prepared.json()["state"], "Prepared");
    for field in [
        "table_name",
        "row_key",
        "key_digest",
        "nonce",
        "confirmation",
    ] {
        assert!(!prepared.body.contains(field));
    }
    let digest = prepared.json()["plan"]["digest"].clone();
    let applied = command(
        address,
        cookie,
        csrf,
        "data:apply",
        json!({"Apply":{"prepared_by":"data:prepare","plan_digest":digest}}),
        PASSWORD,
    )
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    assert_eq!(applied.json()["state"], "Confirmed");
    assert_eq!(applied.json()["backup"]["verified"], true);
    let second = command(
        address,
        cookie,
        csrf,
        "data:second-apply",
        json!({"Apply":{"prepared_by":"data:prepare","plan_digest":digest}}),
        PASSWORD,
    )
    .await;
    assert_eq!(second.status, 409);
    let inspect = query(
        address,
        cookie,
        csrf,
        "data:inspect",
        json!({"Inspect":{"command":"data:apply"}}),
    )
    .await;
    assert_eq!(inspect.status, 200);
    assert_eq!(inspect.json()["receipt"], applied.json());
    assert!(!inspect.body.contains(PASSWORD));
    assert_eq!(
        request(
            address,
            "POST",
            "/api/v1/brain/data/workbench/command",
            mutation_headers(address, cookie, csrf, "data:bypass"),
            "{}".into()
        )
        .await
        .status,
        404
    );
}
