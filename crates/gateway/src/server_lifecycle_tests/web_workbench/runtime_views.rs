//! Read-only authenticated diagnostics must not become a runtime command proxy.
use super::*;

pub(super) async fn verify(
    address: SocketAddr,
    runtime: &ProductionRuntime,
    calls: &AtomicUsize,
    cookie: &str,
    csrf: &str,
) {
    let heads: Vec<_> = runtime
        .services()
        .aggregates
        .as_map()
        .values()
        .map(|run| (run.clone(), runtime.services().store.run_head(run).unwrap()))
        .collect();
    let model_calls = calls.load(Ordering::SeqCst);
    let path = "/api/v1/brain/runtime/doctor";
    assert_eq!(
        request(address, "GET", path, vec![], String::new())
            .await
            .status,
        401
    );
    for _ in 0..2 {
        let reply = request(
            address,
            "GET",
            path,
            vec![("Cookie", cookie.into())],
            String::new(),
        )
        .await;
        assert_eq!(reply.status, 200, "{}", reply.body);
        let view = reply.json();
        let doctor = runtime.doctor();
        assert_eq!(view["schema_version"], 1);
        assert_eq!(view["blocking"], doctor.blocking);
        assert_eq!(view["digest"], doctor.digest.0);
        let checks = view["checks"].as_array().unwrap();
        assert_eq!(checks.len(), doctor.checks.len());
        for check in checks {
            assert_eq!(check["schema_version"], 1);
            assert!(matches!(
                check["status"].as_str(),
                Some("ready" | "disabled" | "unsupported" | "blocking")
            ));
            assert!(check["reason"].is_null() || check["reason"].is_string());
        }
        assert!(!reply.body.contains(BRIDGE_TOKEN));
    }
    let denied = request(
        address,
        "POST",
        path,
        mutation_headers(address, cookie, csrf, "doctor:no-mutation"),
        "{}".into(),
    )
    .await;
    assert_eq!(denied.status, 405);
    for forbidden in [
        "/api/v1/brain/runtime/tick",
        "/api/v1/brain/runtime/reload",
        "/api/v1/brain/runtime/doctor?tick=1",
    ] {
        let reply = request(
            address,
            "GET",
            forbidden,
            vec![("Cookie", cookie.into())],
            String::new(),
        )
        .await;
        assert!(matches!(reply.status, 400 | 404), "{}", reply.body);
    }
    for (run, head) in heads {
        assert_eq!(runtime.services().store.run_head(&run).unwrap(), head);
    }
    assert_eq!(calls.load(Ordering::SeqCst), model_calls);
}
