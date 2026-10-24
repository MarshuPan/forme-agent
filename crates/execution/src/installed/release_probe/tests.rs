use super::*;
use std::net::TcpListener;

fn binding() -> p::ProductReleaseBinding {
    let hash = |s: &str| p::sha256_content_digest(s.as_bytes());
    p::ProductReleaseBinding {
        schema_version: p::SchemaVersion(1),
        install_ref: "install:live-probe".into(),
        operation_id: "upgrade:live-probe".into(),
        command_digest: hash("command"),
        target_release_ref: "0.2.0".into(),
        target_release_digest: hash("release"),
        core_image_digest: hash("image"),
        owner_digest: hash("owner"),
        store_identity: hash("store"),
        config_revision: 2,
        readiness_nonce: "a".repeat(64),
    }
}

#[test]
fn real_probe_socket_rejects_forgery_replay_and_wrong_binding_without_disclosing_bearer() {
    let credential = "t".repeat(64);
    for mode in ["valid", "forged", "replayed", "foreign-binding"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let expected = binding();
        let response_binding = expected.clone();
        let server_credential = credential.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut block = [0; 2048];
            let (offset, length) = loop {
                let n = stream.read(&mut block).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&block[..n]);
                assert!(bytes.len() < 8192);
                let mut headers = [httparse::EMPTY_HEADER; 16];
                let mut request = httparse::Request::new(&mut headers);
                if let Ok(httparse::Status::Complete(offset)) = request.parse(&bytes) {
                    let length = request
                        .headers
                        .iter()
                        .find(|h| h.name.eq_ignore_ascii_case("content-length"))
                        .map(|h| {
                            std::str::from_utf8(h.value)
                                .unwrap()
                                .parse::<usize>()
                                .unwrap()
                        })
                        .unwrap();
                    if bytes.len() >= offset + length {
                        break (offset, length);
                    }
                }
            };
            assert!(!String::from_utf8_lossy(&bytes).contains(&server_credential));
            assert!(!String::from_utf8_lossy(&bytes)
                .to_ascii_lowercase()
                .contains("authorization:"));
            let request: p::ProductReleaseReadinessProbe =
                serde_json::from_slice(&bytes[offset..offset + length]).unwrap();
            let key = SigningKey::from_bytes(
                &p::product_release_probe_seed(server_credential.as_bytes()).unwrap(),
            );
            key.verifying_key()
                .verify_strict(
                    request.signing_digest().unwrap().0.as_bytes(),
                    &Signature::from_slice(&request.signature).unwrap(),
                )
                .unwrap();
            let mut proof = p::ProductReleaseReadinessProof {
                schema_version: p::SchemaVersion(1),
                nonce: request.nonce,
                readiness: p::ProductReleaseReadiness {
                    schema_version: p::SchemaVersion(1),
                    config_revision: 2,
                    actual_image_digest: response_binding.core_image_digest.clone(),
                    data_identity: p::ProductDataAcknowledgement {
                        schema_version: p::SchemaVersion(1),
                        owner_digest: response_binding.owner_digest.clone(),
                        store_identity: response_binding.store_identity.clone(),
                        execution_fence_digest: None,
                        composition_digest: p::sha256_content_digest(b"composition"),
                    },
                    binding: response_binding,
                },
                signature: vec![],
            };
            if mode == "replayed" {
                proof.nonce = "b".repeat(64);
            }
            if mode == "foreign-binding" {
                proof.readiness.binding.operation_id = "upgrade:foreign".into();
            }
            let signing = if mode == "forged" {
                SigningKey::from_bytes(&[21; 32])
            } else {
                key
            };
            proof.signature = signing
                .sign(proof.signing_digest().unwrap().0.as_bytes())
                .to_bytes()
                .to_vec();
            let body = serde_json::to_vec(&proof).unwrap();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
        });
        let result = probe(
            address,
            credential.as_bytes(),
            &expected,
            2,
            Instant::now() + Duration::from_secs(5),
        );
        assert_eq!(result.is_ok(), mode == "valid", "{mode}");
        server.join().unwrap();
    }
}

#[test]
fn non_loopback_or_expired_probe_has_no_connection_attempt() {
    assert!(probe(
        "192.0.2.1:8768".parse().unwrap(),
        b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        &binding(),
        2,
        Instant::now()
    )
    .is_err());
    assert!(probe(
        "127.0.0.1:1".parse().unwrap(),
        b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        &binding(),
        2,
        Instant::now()
    )
    .is_err());
}
