//! Explicit, disposable browser path through Web -> Gateway -> ProductionRuntime.
//! The counted local model proves routing and no duplicate calls, not model quality.
use super::*;
use std::time::Instant;

#[tokio::test]
#[ignore = "opt-in browser runtime fixture; submit exactly one conversation before stopping"]
async fn browser_runtime_preview_is_one_real_run_without_duplicate_model_calls() {
    assert_eq!(
        std::env::var("FORME_BROWSER_RUNTIME_PREVIEW").as_deref(),
        Ok("1")
    );
    let fixture = Fixture::new();
    // Bound the fixture's responses; it never calls an external provider.
    for _ in 0..16 {
        fixture.release.send(()).unwrap();
    }
    let runtime = fixture.build();
    let key = SigningKey::from_bytes(&[107; 32]);
    let identity_path = fixture.root.join("browser-auth");
    claim(&fixture, &key, &identity_path);
    let token_path = fixture.root.join("bridge-token");
    let key_path = fixture.root.join("bridge-signing-key");
    std::fs::write(&token_path, BRIDGE_TOKEN).unwrap();
    std::fs::write(&key_path, key.to_bytes()).unwrap();

    let mut config = fixture.gateway_config();
    config.install_ref = Some(INSTALL.into());
    config.web_bridge_token = Some(LocalBearerSecret::new(BRIDGE_TOKEN.into()).unwrap());
    config.web_assertion_verifying_key = Some(key.verifying_key());
    config.web_session_fence_path = Some(fixture.root.join("core-auth.sqlite"));
    let (core_stop, core_stopped) = tokio::sync::oneshot::channel();
    let (core_ready, core_address) = tokio::sync::oneshot::channel();
    let core = tokio::spawn(serve_runtime_until(
        config,
        runtime.clone(),
        GatewayLifecycleConfig {
            schema_version: p::SchemaVersion(1),
            drain_timeout: Duration::from_secs(5),
        },
        move |address| {
            let _ = core_ready.send(address);
            Ok(())
        },
        async {
            let _ = core_stopped.await;
            Ok(())
        },
    ));
    let core_address = core_address.await.unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let web_bind = probe.local_addr().unwrap();
    drop(probe);
    let (web_stop, web_stopped) = tokio::sync::oneshot::channel();
    let (web_ready, web_address) = tokio::sync::oneshot::channel();
    let web = tokio::spawn(http::serve_until(
        WebServerConfig {
            schema_version: p::SchemaVersion(1),
            install_ref: INSTALL.into(),
            identity_directory: identity_path,
            claim_verifying_key: key.verifying_key().to_bytes(),
            bind: web_bind,
            tls: None,
            network_config_path: None,
            network_material_directory: None,
            bootstrap_bridge: None,
            configuration_bridge: None,
            core_bridge: Some(WebCoreBridgeConfig {
                schema_version: p::SchemaVersion(1),
                address: core_address,
                credential_path: token_path,
                signing_key_path: key_path,
            }),
        },
        move |address| {
            let _ = web_ready.send(address);
            Ok(())
        },
        async {
            let _ = web_stopped.await;
            Ok(())
        },
    ));
    let web_address = web_address.await.unwrap();
    let stop = fixture.root.join("stop-browser-preview");
    println!("BROWSER_RUNTIME_URL=http://{web_address}");
    println!("BROWSER_RUNTIME_STOP={}", stop.display());
    println!("BROWSER_RUNTIME_SCOPE=preclaimed temporary Web + ProductionRuntime; counted local model; no installed services");
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
    std::io::stdout().flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(900);
    while !stop.is_file() && Instant::now() < deadline {
        if web.is_finished() || core.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let stopped_explicitly = stop.is_file();
    let _ = web_stop.send(());
    let web_result = tokio::time::timeout(Duration::from_secs(15), web).await;
    let _ = core_stop.send(());
    let core_result = tokio::time::timeout(Duration::from_secs(15), core).await;
    web_result.unwrap().unwrap().unwrap();
    core_result.unwrap().unwrap().unwrap();
    assert!(stopped_explicitly, "preview needs an explicit stop marker");

    let runs = GatewayControl::list_runs(runtime.harness().as_ref()).unwrap();
    assert_eq!(runs.len(), 1, "browser reload must not create another run");
    let recorded = events(&runtime, &runs[0].run);
    for kind in [
        p::EventKind::RunAccepted,
        p::EventKind::ContextBuildFinished,
        p::EventKind::ModelCallStarted,
        p::EventKind::ModelCallFinished,
        p::EventKind::RunComplete,
    ] {
        assert_eq!(recorded.iter().filter(|e| e.kind == kind).count(), 1);
    }
    assert!(!recorded
        .iter()
        .any(|e| e.kind == p::EventKind::ActionStarted));
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    println!("BROWSER_RUNTIME_EVIDENCE=1 run; 1 model call; 0 tool actions; context/model/complete events each once");
    let root = fixture.root.clone();
    drop(runtime);
    drop(fixture);
    assert!(!root.exists(), "temporary preview data must be removed");
}
