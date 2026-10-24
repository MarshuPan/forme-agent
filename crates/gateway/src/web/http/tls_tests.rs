//! Real TLS transport tests; private fixture identities never leave temp dirs.
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use ed25519_dalek::{Signer, SigningKey};
use forme_protocol as p;
use rustls_pki_types::{CertificateDer, ServerName};

use crate::web::identity::{SensitivePassword, WebIdentityStore};
use crate::web::{WebServerConfig, WebTlsConfig};

struct Fixture {
    _root: tempfile::TempDir,
    address: SocketAddr,
    certificate: CertificateDer<'static>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn configuration(
    root: &std::path::Path,
    address: SocketAddr,
) -> (WebServerConfig, CertificateDer<'static>) {
    let identity = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let certificate = root.join("server.pem");
    let key = root.join("server.key");
    std::fs::write(&certificate, identity.cert.pem()).unwrap();
    std::fs::write(&key, identity.signing_key.serialize_pem()).unwrap();
    (
        WebServerConfig {
            network_config_path: None,
            network_material_directory: None,
            schema_version: p::SchemaVersion(1),
            install_ref: "installation:https-fixture".into(),
            identity_directory: root.to_path_buf(),
            claim_verifying_key: SigningKey::from_bytes(&[119; 32])
                .verifying_key()
                .to_bytes(),
            bind: address,
            tls: Some(WebTlsConfig {
                schema_version: p::SchemaVersion(1),
                certificate_path: certificate,
                private_key_path: key,
                allowed_hosts: vec![format!("localhost:{}", address.port())],
            }),
            bootstrap_bridge: None,
            configuration_bridge: None,
            core_bridge: None,
        },
        identity.cert.der().clone(),
    )
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().canonicalize().unwrap();
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = probe.local_addr().unwrap();
        let (config, certificate) = configuration(&path, address);
        let key = SigningKey::from_bytes(&[119; 32]);
        let identity =
            WebIdentityStore::open(&path, &config.install_ref, key.verifying_key()).unwrap();
        let commitment = identity
            .prepare_claim(
                "claim:https-fixture",
                "fixture",
                SensitivePassword::new("HTTPS fixture password 20".into()),
            )
            .unwrap();
        let receipt = p::WebClaimCommit {
            schema_version: p::SchemaVersion(1),
            install_ref: config.install_ref.clone(),
            operation_id: "claim:https-fixture".into(),
            candidate_ref: "data:https-fixture".into(),
            owner: p::OwnerPrincipalRef("owner:https-fixture".into()),
            account_commitment: commitment,
            expires_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
                + 60_000,
        };
        let digest = p::canonical_digest(&("forme-web-claim-commit-v1", &receipt)).unwrap();
        identity
            .commit_claim(&p::SignedWebClaimCommit {
                schema_version: p::SchemaVersion(1),
                receipt,
                signature: key.sign(digest.0.as_bytes()).to_bytes().to_vec(),
            })
            .unwrap();
        drop(identity);
        drop(probe);
        let (ready, bound) = mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(super::serve_until(
                    config,
                    |address| {
                        ready.send(address).unwrap();
                        Ok(())
                    },
                    async {
                        stopped.await.unwrap();
                        Ok(())
                    },
                ))
                .unwrap();
        });
        Self {
            _root: root,
            address: bound.recv_timeout(Duration::from_secs(10)).unwrap(),
            certificate,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    fn authority(&self) -> String {
        format!("localhost:{}", self.address.port())
    }

    fn client(&self, trust: bool) -> rustls::StreamOwned<rustls::ClientConnection, TcpStream> {
        let mut roots = rustls::RootCertStore::empty();
        if trust {
            roots.add(self.certificate.clone()).unwrap();
        }
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let client = rustls::ClientConnection::new(
            Arc::new(config),
            ServerName::try_from("localhost").unwrap(),
        )
        .unwrap();
        let socket = TcpStream::connect(self.address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        rustls::StreamOwned::new(client, socket)
    }

    fn request(&self, method: &str, path: &str, host: &str, extra: &str) -> String {
        let mut stream = self.client(true);
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: 0\r\n{extra}\r\n").as_bytes()).unwrap();
        let mut result = String::new();
        // Hyper closes the transport after a complete Connection:close response;
        // require the full declared body even if TLS close_notify is absent.
        if let Err(error) = stream.read_to_string(&mut result) {
            assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
        }
        let (headers, body) = result.split_once("\r\n\r\n").unwrap();
        let length = headers.lines().find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .map(str::to_owned)
        });
        if let Some(length) = length {
            assert_eq!(body.len(), length.parse::<usize>().unwrap());
        }
        result
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn https_real_handshake_enforces_host_origin_secure_cookie_and_plaintext_rejection() {
    let fixture = Fixture::new();
    let host = fixture.authority();
    let reply = fixture.request("GET", "/api/v1/auth/login-challenge", &host, "");
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.contains("; Secure"));
    assert!(reply.contains("HttpOnly; SameSite=Strict"));
    assert!(reply.contains("strict-transport-security: max-age=86400"));
    for (host, extra) in [
        ("attacker.invalid:8765".to_string(), String::new()),
        (host.clone(), format!("Origin: http://{host}\r\n")),
        (host.clone(), "Origin: https://attacker.invalid\r\n".into()),
        (
            host.clone(),
            "X-Forwarded-Host: attacker.invalid\r\n".into(),
        ),
    ] {
        let reply = fixture.request("POST", "/api/v1/auth/login", &host, &extra);
        assert!(reply.starts_with("HTTP/1.1 403"));
    }
    let mut raw = TcpStream::connect(fixture.address).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(7))).unwrap();
    raw.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut bytes = [0; 512];
    let amount = raw.read(&mut bytes).unwrap_or(0);
    assert!(!String::from_utf8_lossy(&bytes[..amount]).contains("HTTP/1.1 200"));
    let mut untrusted = fixture.client(false);
    assert!(
        untrusted.write_all(b"GET / HTTP/1.1\r\n\r\n").is_err()
            || untrusted.read(&mut bytes).is_err()
    );
}

#[test]
fn https_slow_handshake_does_not_block_other_clients() {
    let fixture = Fixture::new();
    let _slow = TcpStream::connect(fixture.address).unwrap();
    let began = std::time::Instant::now();
    let reply = fixture.request("GET", "/api/v1/bootstrap/status", &fixture.authority(), "");
    assert!(reply.starts_with("HTTP/1.1 200"));
    assert!(began.elapsed() < Duration::from_secs(4));
}

#[test]
fn https_configuration_rejects_ambiguous_hosts_bad_keys_and_plaintext_lan() {
    let root = tempfile::tempdir().unwrap();
    let (mut config, _) = configuration(
        &root.path().canonicalize().unwrap(),
        "127.0.0.1:8765".parse().unwrap(),
    );
    config.validate().unwrap();
    config.tls.as_ref().unwrap().load().unwrap();
    let valid = config.tls.clone().unwrap();
    for host in [
        "*.local:8765",
        "user@localhost:8765",
        "localhost",
        "LOCALHOST:8765",
        "localhost:80",
        "localhost.:8765",
        "0.0.0.0:8765",
    ] {
        config.tls.as_mut().unwrap().allowed_hosts = vec![host.into()];
        assert!(config.validate().is_err(), "{host}");
    }
    config.tls = Some(valid.clone());
    config
        .tls
        .as_mut()
        .unwrap()
        .allowed_hosts
        .push("localhost:8765".into());
    assert!(config.validate().is_err());
    config.tls = Some(valid);
    let other = rcgen::KeyPair::generate().unwrap();
    std::fs::write(
        &config.tls.as_ref().unwrap().private_key_path,
        other.serialize_pem(),
    )
    .unwrap();
    assert!(config.tls.as_ref().unwrap().load().is_err());
    config.tls = None;
    config.bind = "192.168.1.7:8765".parse().unwrap();
    assert!(config.validate().is_err());
}

#[test]
fn installed_network_overlay_controls_web_binding_and_rejects_plaintext_lan() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    let material = path.join("web-material");
    std::fs::create_dir(&material).unwrap();
    let address: SocketAddr = "127.0.0.1:8765".parse().unwrap();
    let (mut config, _) = configuration(&path, address);
    let certificate = material.join("server.pem");
    let key = material.join("server.key");
    std::fs::copy(path.join("server.pem"), &certificate).unwrap();
    std::fs::copy(path.join("server.key"), &key).unwrap();
    let network_path = path.join("web-network.json");
    config.network_config_path = Some(network_path.clone());
    config.network_material_directory = Some(material.clone());
    let network = p::ProductNetworkConfig {
        schema_version: p::PRODUCT_NETWORK_SCHEMA_VERSION,
        bind: address,
        allowed_hosts: vec![format!("localhost:{}", address.port())],
        tls: Some(p::ProductNetworkTls {
            schema_version: p::PRODUCT_NETWORK_SCHEMA_VERSION,
            certificate_path: certificate.clone(),
            private_key_path: key.clone(),
        }),
        firewall: None,
    };
    std::fs::write(&network_path, serde_json::to_vec(&network).unwrap()).unwrap();
    let config_path = path.join("web.json");
    std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let loaded = WebServerConfig::load(&config_path).unwrap();
    assert_eq!(loaded.bind, address);
    assert_eq!(loaded.tls.as_ref().unwrap().certificate_path, certificate);
    loaded.tls.as_ref().unwrap().load().unwrap();

    let mut plaintext_lan = network;
    plaintext_lan.bind = "192.168.1.20:8765".parse().unwrap();
    plaintext_lan.allowed_hosts = vec!["192.168.1.20:8765".into()];
    plaintext_lan.tls = None;
    plaintext_lan.firewall = Some(p::ProductFirewallBinding {
        schema_version: p::PRODUCT_NETWORK_SCHEMA_VERSION,
        rule_ref: "firewall:forme-web-lan".into(),
    });
    std::fs::write(&network_path, serde_json::to_vec(&plaintext_lan).unwrap()).unwrap();
    assert!(WebServerConfig::load(&config_path).is_err());
}

#[tokio::test]
async fn https_bad_material_fails_before_identity_store_creation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    let (config, _) = configuration(&path, "127.0.0.1:8765".parse().unwrap());
    std::fs::write(
        &config.tls.as_ref().unwrap().certificate_path,
        "invalid certificate fixture",
    )
    .unwrap();
    assert!(
        super::serve_until(config, |_| panic!("must not bind"), std::future::pending())
            .await
            .is_err()
    );
    assert!(!path.join("identity.sqlite").exists());
}
