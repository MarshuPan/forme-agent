//! Real HTTP/SQLite/ProductionRuntime ingress; no installed-service claim.
use super::*;
const CHANNEL_TOKEN: &str = "local-connector-specific-token-0123456789abcdef";
fn provision(f: &Fixture) -> p::ProductChannelIngress {
    let source = p::ProductChannelSource {
        delivery: None,
        schema_version: p::SchemaVersion(1),
        channel_ref: "channel:guest".into(),
        connector_ref: "connector:guest".into(),
        scope: p::Scope(f.config.workspace.0.clone()),
        participant_binding: "mapping:guest".into(),
        participant: p::ParticipantId("participant:guest".into()),
        credential_ref: "credential:guest".into(),
        credential_digest: p::sha256_content_digest(CHANNEL_TOKEN.as_bytes()),
    };
    let ingress = p::ProductChannelIngress {
        schema_version: p::SchemaVersion(1),
        configuration_path: f.root.join("channel-core.json"),
        sources: vec![source.clone()],
    };
    let settings = p::ProductChannelConfig {
        schema_version: p::SchemaVersion(1),
        channel_ref: source.channel_ref.clone(),
        adapter_ref: p::ProductChannelAdapter::Text,
        connector_ref: source.connector_ref.clone(),
        scope: source.scope.clone(),
        enabled: true,
        participant_binding: source.participant_binding.clone(),
        credential_ref: Some(source.credential_ref.clone()),
    };
    let document = serde_json::json!({"brain":{"owner":f.config.owner,"workspace":f.config.workspace,"channels":{"schema_version":1,"channels":[settings]}},"channel_ingress":ingress});
    std::fs::write(
        &ingress.configuration_path,
        serde_json::to_vec(&document).unwrap(),
    )
    .unwrap();
    std::fs::write(
        ingress
            .configuration_path
            .with_extension("channel-sources.json"),
        serde_json::to_vec(&p::ProductChannelSourceCatalog {
            schema_version: p::SchemaVersion(1),
            sources: vec![source],
        })
        .unwrap(),
    )
    .unwrap();
    ingress
}
async fn send(address: SocketAddr, token: &str, body: serde_json::Value, extra: &str) -> String {
    let token = token.to_owned();
    let extra = extra.to_owned();
    tokio::task::spawn_blocking(move || {
        let body=body.to_string();let mut stream=TcpStream::connect_timeout(&address,Duration::from_secs(2)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        write!(stream,"POST /v1/channels/channel:guest/messages HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        stream.flush().unwrap();let mut response=String::new();stream.read_to_string(&mut response).unwrap();response
    }).await.unwrap()
}
#[tokio::test]
async fn channel_http_keeps_external_identity_deduplicates_after_restart_and_fences_revocation() {
    let f = Fixture::new();
    let ingress = provision(&f);
    let body = serde_json::json!({"schema_version":1,"message_id":"message:first","conversation":"conversation:first","text":"a message from the configured guest"});
    let mut original_run = None;
    for restart in 0..2 {
        let runtime = f.build();
        let mut config = f.gateway_config();
        config.channel_ingress = Some(ingress.clone());
        let (stop, receive) = tokio::sync::oneshot::channel();
        let (ready, address) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve_runtime_until(
            config,
            runtime.clone(),
            GatewayLifecycleConfig {
                schema_version: p::SchemaVersion(1),
                drain_timeout: Duration::from_secs(3),
            },
            move |addr| {
                ready.send(addr).unwrap();
                Ok(())
            },
            async {
                receive.await.unwrap();
                Ok(())
            },
        ));
        let address = address.await.unwrap();
        if restart == 0 {
            for (token, extra, bad) in [
                (TOKEN, "", body.clone()),
                ("wrong", "", body.clone()),
                (CHANNEL_TOKEN, "Cookie: owner=forged\r\n", body.clone()),
                (
                    CHANNEL_TOKEN,
                    "Origin: http://attacker.invalid\r\n",
                    body.clone(),
                ),
                (CHANNEL_TOKEN, "", {
                    let mut v = body.clone();
                    v["participant"] = "owner".into();
                    v
                }),
            ] {
                let response = send(address, token, bad, extra).await;
                assert!(!response.starts_with("HTTP/1.1 202"), "{response}");
            }
            assert_eq!(f.calls.load(Ordering::SeqCst), 0);
        }
        let response = send(address, CHANNEL_TOKEN, body.clone(), "").await;
        assert!(response.starts_with("HTTP/1.1 202"), "{response}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(response.split_once("\r\n\r\n").unwrap().1)
                .unwrap(),
            serde_json::json!({"schema_version":1,"accepted":true})
        );
        if restart == 0 {
            f.wait_for_model().await;
            f.release.send(()).unwrap();
        }
        let runs = runtime.harness().list_runs().unwrap();
        assert_eq!(runs.len(), 1);
        let run = runs[0].run.clone();
        if let Some(original) = &original_run {
            assert_eq!(original, &run);
        } else {
            original_run = Some(run.clone());
        }
        let harness = runtime.harness();
        let id = run.clone();
        tokio::task::spawn_blocking(move || harness.wait(id).unwrap())
            .await
            .unwrap();
        let trace = events(&runtime, &run);
        let received = trace
            .iter()
            .filter(|e| e.kind == p::EventKind::CommunicationEventReceived)
            .collect::<Vec<_>>();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].provenance.trust_tier, p::TrustTier::Untrusted);
        assert!(
            matches!(&received[0].payload,p::EventPayload::CommunicationEventReceived(v) if v.participant.0=="participant:guest")
        );
        assert!(!trace.iter().any(|e| e.kind == p::EventKind::ActionStarted));
        let mut changed = body.clone();
        changed["text"] = "altered same message".into();
        assert!(!send(address, CHANNEL_TOKEN, changed, "")
            .await
            .starts_with("HTTP/1.1 202"));
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        if restart == 1 {
            let mut current: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&ingress.configuration_path).unwrap())
                    .unwrap();
            current["brain"]["channels"]["channels"][0]["enabled"] = false.into();
            std::fs::write(
                &ingress.configuration_path,
                serde_json::to_vec(&current).unwrap(),
            )
            .unwrap();
            let mut next = body.clone();
            next["message_id"] = "message:after-disable".into();
            assert!(send(address, CHANNEL_TOKEN, next, "")
                .await
                .starts_with("HTTP/1.1 401"));
            assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        }
        stop.send(()).unwrap();
        server.await.unwrap().unwrap();
        drop(runtime);
    }
}
