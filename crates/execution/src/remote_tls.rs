use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use forme_protocol as p;
use rustls::{
    client::ClientConfig,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName},
    server::{ServerConfig, WebPkiClientVerifier},
    ClientConnection, RootCertStore, ServerConnection, StreamOwned,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    RemoteClock, RemoteExecutorService, RemoteReceiptSource, RemoteTransport, SystemRemoteClock,
};

const MAX_WIRE_BODY: usize = 1_048_576;
const MAX_HTTP_HEADER: usize = 16_384;

#[derive(Debug, Clone)]
pub struct TlsIdentityFiles {
    pub certificate_der: PathBuf,
    pub private_key_der: PathBuf,
    pub trust_anchor_der: PathBuf,
}

impl TlsIdentityFiles {
    fn load(&self) -> p::Result<LoadedIdentity> {
        let certificate = read_private_file(&self.certificate_der, "certificate")?;
        let private_key = read_private_file(&self.private_key_der, "private key")?;
        let trust_anchor = read_private_file(&self.trust_anchor_der, "trust anchor")?;
        Ok(LoadedIdentity {
            certificate: CertificateDer::from(certificate),
            private_key: PrivateKeyDer::try_from(private_key)
                .map_err(|_| p::Error("TLS private key is not valid DER".into()))?,
            trust_anchor: CertificateDer::from(trust_anchor),
        })
    }
}

struct LoadedIdentity {
    certificate: CertificateDer<'static>,
    private_key: PrivateKeyDer<'static>,
    trust_anchor: CertificateDer<'static>,
}

#[derive(Debug, Clone)]
pub struct TlsRemoteClientConfig {
    pub endpoint: String,
    pub authority: p::AuthorityRef,
    pub peer: p::FederatedPeerRef,
    pub expected_peer_identity: p::TransportIdentityDigest,
    pub identity: TlsIdentityFiles,
    pub timeout: p::DurationMs,
    pub request_ttl: p::DurationMs,
    pub max_body_bytes: usize,
}

pub struct TlsRemoteTransport {
    endpoint: Url,
    authority: p::AuthorityRef,
    peer: p::FederatedPeerRef,
    expected_peer_identity: p::TransportIdentityDigest,
    tls: Arc<ClientConfig>,
    timeout: Duration,
    request_ttl: i64,
    max_body_bytes: usize,
    sequence: AtomicU64,
    clock: Arc<dyn RemoteClock>,
}

impl TlsRemoteTransport {
    pub fn new(config: TlsRemoteClientConfig) -> p::Result<Self> {
        Self::with_clock(config, Arc::new(SystemRemoteClock))
    }

    pub fn with_clock(
        config: TlsRemoteClientConfig,
        clock: Arc<dyn RemoteClock>,
    ) -> p::Result<Self> {
        let endpoint = validate_endpoint(&config.endpoint)?;
        if config.authority.0.trim().is_empty()
            || config.peer.0.trim().is_empty()
            || config.expected_peer_identity.0.trim().is_empty()
            || config.timeout.0 == 0
            || config.request_ttl.0 == 0
            || config.request_ttl.0 > 60_000
            || config.max_body_bytes == 0
            || config.max_body_bytes > MAX_WIRE_BODY
        {
            return Err(p::Error(
                "remote TLS client configuration is incomplete".into(),
            ));
        }
        let loaded = config.identity.load()?;
        let mut roots = RootCertStore::empty();
        roots
            .add(loaded.trust_anchor)
            .map_err(|_| p::Error("remote TLS trust anchor is invalid".into()))?;
        let mut tls = ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
            .with_root_certificates(roots)
            .with_client_auth_cert(vec![loaded.certificate], loaded.private_key)
            .map_err(|_| p::Error("remote TLS client identity is invalid".into()))?;
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Self {
            endpoint,
            authority: config.authority,
            peer: config.peer,
            expected_peer_identity: config.expected_peer_identity,
            tls: Arc::new(tls),
            timeout: Duration::from_millis(config.timeout.0),
            request_ttl: i64::try_from(config.request_ttl.0)
                .map_err(|_| p::Error("remote request TTL is too large".into()))?,
            max_body_bytes: config.max_body_bytes,
            sequence: AtomicU64::new(1),
            clock,
        })
    }

    fn call(&self, command: p::RemoteWireCommand) -> p::Result<p::RemoteWireResponse> {
        command.validate()?;
        let number = self.sequence.fetch_add(1, Ordering::SeqCst);
        let now = self.clock.now_ms();
        let mut envelope = p::RemoteWireEnvelope {
            schema_version: p::M4_SCHEMA_VERSION,
            request: p::RemoteWireRequestRef(format!("wire:{}:{number}", self.authority.0)),
            authority: self.authority.clone(),
            peer: self.peer.clone(),
            nonce: p::Nonce(format!("nonce:{}:{number}", self.authority.0)),
            expires_at: now
                .checked_add(self.request_ttl)
                .ok_or_else(|| p::Error("remote request expiry overflowed".into()))?,
            command,
            digest: p::SchemaDigest(String::new()),
        };
        envelope.refresh_digest()?;
        envelope.validate(now)?;
        let body = serde_json::to_vec(&envelope)
            .map_err(|_| p::Error("remote request could not be encoded".into()))?;
        if body.len() > self.max_body_bytes {
            return Err(p::Error("remote request exceeded its byte limit".into()));
        }
        let tcp = endpoint_addresses(&self.endpoint)?
            .into_iter()
            .find_map(|address| TcpStream::connect_timeout(&address, self.timeout).ok())
            .ok_or_else(|| p::Error("remote TLS connection failed".into()))?;
        tcp.set_read_timeout(Some(self.timeout))
            .and_then(|_| tcp.set_write_timeout(Some(self.timeout)))
            .map_err(|_| p::Error("remote TLS socket limits could not be applied".into()))?;
        let host = self
            .endpoint
            .host_str()
            .ok_or_else(|| p::Error("remote TLS endpoint has no host".into()))?
            .to_owned();
        let server_name = ServerName::try_from(host.clone())
            .map_err(|_| p::Error("remote TLS server name is invalid".into()))?;
        let connection = ClientConnection::new(self.tls.clone(), server_name)
            .map_err(|_| p::Error("remote TLS client could not start".into()))?;
        let mut stream = StreamOwned::new(connection, tcp);
        while stream.conn.is_handshaking() {
            stream
                .conn
                .complete_io(&mut stream.sock)
                .map_err(|_| p::Error("remote TLS handshake failed".into()))?;
        }
        verify_peer_identity(
            stream.conn.peer_certificates(),
            &self.expected_peer_identity,
        )?;
        let path = command_path(&envelope.command);
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .and_then(|_| stream.write_all(&body))
            .and_then(|_| stream.flush())
            .map_err(|_| p::Error("remote TLS request failed".into()))?;
        let bytes = read_http_response_bytes(&mut stream, self.max_body_bytes)?;
        let response_body = parse_http_response(&bytes, self.max_body_bytes)?;
        let reply: p::RemoteWireReply = serde_json::from_slice(response_body)
            .map_err(|_| p::Error("remote TLS reply is malformed".into()))?;
        reply.validate()?;
        if reply.request != envelope.request {
            return Err(p::Error("remote TLS reply request binding changed".into()));
        }
        Ok(reply.response)
    }
}

impl RemoteTransport for TlsRemoteTransport {
    fn dispatch(
        &self,
        plan: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
    ) -> p::Result<p::RemoteDispatchAcceptance> {
        match self.call(p::RemoteWireCommand::Dispatch {
            plan: Box::new(plan.clone()),
            lease: lease.clone(),
        })? {
            p::RemoteWireResponse::Dispatch(acceptance) => {
                acceptance.validate_for(lease)?;
                Ok(acceptance)
            }
            _ => Err(p::Error(
                "remote TLS dispatch returned the wrong reply kind".into(),
            )),
        }
    }

    fn probe(&self, request: p::RemoteProbeRequest) -> p::Result<p::RemoteProbeResult> {
        let lease = request.lease.clone();
        match self.call(p::RemoteWireCommand::Probe(request))? {
            p::RemoteWireResponse::Probe(result) if result.lease == lease => {
                result.validate()?;
                Ok(result)
            }
            _ => Err(p::Error(
                "remote TLS probe returned the wrong reply kind".into(),
            )),
        }
    }

    fn cancel(&self, request: p::RemoteCancelRequest) -> p::Result<p::RemoteCancelResult> {
        let lease = request.lease.clone();
        match self.call(p::RemoteWireCommand::Cancel(request))? {
            p::RemoteWireResponse::Cancel(result) if result.lease == lease => {
                result.validate()?;
                Ok(result)
            }
            _ => Err(p::Error(
                "remote TLS cancel returned the wrong reply kind".into(),
            )),
        }
    }
}

impl RemoteReceiptSource for TlsRemoteTransport {
    fn receipt(&self, request: p::RemoteReceiptRequest) -> p::Result<p::RemoteDriverReceipt> {
        let expected = request.receipt.clone();
        request.validate()?;
        match self.call(p::RemoteWireCommand::Receipt(request))? {
            p::RemoteWireResponse::Receipt(receipt) if receipt.receipt == expected => {
                receipt.validate()?;
                Ok(receipt)
            }
            _ => Err(p::Error(
                "remote TLS receipt returned the wrong reply kind".into(),
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TlsRemoteServerConfig {
    pub bind: SocketAddr,
    pub authority: p::AuthorityRef,
    pub peer: p::FederatedPeerRef,
    pub expected_authority_identity: p::TransportIdentityDigest,
    pub identity: TlsIdentityFiles,
    pub replay_root: PathBuf,
    pub timeout: p::DurationMs,
    pub max_body_bytes: usize,
}

pub struct TlsRemoteExecutorServer {
    listener: TcpListener,
    authority: p::AuthorityRef,
    peer: p::FederatedPeerRef,
    expected_authority_identity: p::TransportIdentityDigest,
    tls: Arc<ServerConfig>,
    replay: WireReplayLedger,
    service: Arc<dyn RemoteExecutorService>,
    clock: Arc<dyn RemoteClock>,
    timeout: Duration,
    max_body_bytes: usize,
}

impl TlsRemoteExecutorServer {
    pub fn bind(
        config: TlsRemoteServerConfig,
        service: Arc<dyn RemoteExecutorService>,
    ) -> p::Result<Self> {
        Self::bind_with_clock(config, service, Arc::new(SystemRemoteClock))
    }

    pub fn bind_with_clock(
        config: TlsRemoteServerConfig,
        service: Arc<dyn RemoteExecutorService>,
        clock: Arc<dyn RemoteClock>,
    ) -> p::Result<Self> {
        if config.authority.0.trim().is_empty()
            || config.peer.0.trim().is_empty()
            || config.expected_authority_identity.0.trim().is_empty()
            || config.timeout.0 == 0
            || config.max_body_bytes == 0
            || config.max_body_bytes > MAX_WIRE_BODY
        {
            return Err(p::Error(
                "remote TLS server configuration is incomplete".into(),
            ));
        }
        let loaded = config.identity.load()?;
        let mut roots = RootCertStore::empty();
        roots
            .add(loaded.trust_anchor)
            .map_err(|_| p::Error("remote TLS client trust anchor is invalid".into()))?;
        let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|_| p::Error("remote TLS client verifier is invalid".into()))?;
        let mut tls = ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![loaded.certificate], loaded.private_key)
            .map_err(|_| p::Error("remote TLS server identity is invalid".into()))?;
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        let listener = TcpListener::bind(config.bind)
            .map_err(|_| p::Error("remote TLS server bind failed".into()))?;
        let replay = WireReplayLedger::open(config.replay_root)?;
        Ok(Self {
            listener,
            authority: config.authority,
            peer: config.peer,
            expected_authority_identity: config.expected_authority_identity,
            tls: Arc::new(tls),
            replay,
            service,
            clock,
            timeout: Duration::from_millis(config.timeout.0),
            max_body_bytes: config.max_body_bytes,
        })
    }

    pub fn local_addr(&self) -> p::Result<SocketAddr> {
        self.listener
            .local_addr()
            .map_err(|_| p::Error("remote TLS listener address is unavailable".into()))
    }

    pub fn serve_one(&self) -> p::Result<()> {
        let (tcp, _) = self
            .listener
            .accept()
            .map_err(|_| p::Error("remote TLS accept failed".into()))?;
        self.handle(tcp)
    }

    pub fn serve_until(&self, stop: &AtomicBool) -> p::Result<()> {
        self.listener
            .set_nonblocking(true)
            .map_err(|_| p::Error("remote TLS listener mode could not be set".into()))?;
        while !stop.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((tcp, _)) => self.handle(tcp)?,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return Err(p::Error("remote TLS accept failed".into())),
            }
        }
        Ok(())
    }

    fn handle(&self, tcp: TcpStream) -> p::Result<()> {
        tcp.set_nonblocking(false)
            .and_then(|_| tcp.set_read_timeout(Some(self.timeout)))
            .and_then(|_| tcp.set_write_timeout(Some(self.timeout)))
            .map_err(|_| p::Error("remote TLS socket limits could not be applied".into()))?;
        let connection = ServerConnection::new(self.tls.clone())
            .map_err(|_| p::Error("remote TLS server could not start".into()))?;
        let mut stream = StreamOwned::new(connection, tcp);
        while stream.conn.is_handshaking() {
            stream
                .conn
                .complete_io(&mut stream.sock)
                .map_err(|_| p::Error("remote TLS handshake failed".into()))?;
        }
        verify_peer_identity(
            stream.conn.peer_certificates(),
            &self.expected_authority_identity,
        )?;
        let bytes = read_http_request(&mut stream, self.max_body_bytes)?;
        let (path, body) = parse_http_request(&bytes, self.max_body_bytes)?;
        let envelope: p::RemoteWireEnvelope = serde_json::from_slice(body)
            .map_err(|_| p::Error("remote TLS request is malformed".into()))?;
        envelope.validate(self.clock.now_ms())?;
        if envelope.authority != self.authority
            || envelope.peer != self.peer
            || path != command_path(&envelope.command)
        {
            return Err(p::Error(
                "remote TLS request identity binding failed".into(),
            ));
        }
        self.replay.claim(&envelope)?;
        let response = self.apply_command(envelope.command)?;
        let mut reply = p::RemoteWireReply {
            schema_version: p::M4_SCHEMA_VERSION,
            request: envelope.request,
            response,
            digest: p::SchemaDigest(String::new()),
        };
        reply.refresh_digest()?;
        reply.validate()?;
        let body = serde_json::to_vec(&reply)
            .map_err(|_| p::Error("remote TLS reply could not be encoded".into()))?;
        write_http_response(&mut stream, &body, self.max_body_bytes)?;
        stream.conn.send_close_notify();
        stream
            .flush()
            .map_err(|_| p::Error("remote TLS reply shutdown failed".into()))
    }

    fn apply_command(&self, command: p::RemoteWireCommand) -> p::Result<p::RemoteWireResponse> {
        match command {
            p::RemoteWireCommand::Dispatch { plan, lease } => {
                let admission = self.service.admit(&plan, &lease)?;
                let receipt = self.service.execute(admission)?;
                Ok(p::RemoteWireResponse::Dispatch(
                    p::RemoteDispatchAcceptance {
                        schema_version: p::M4_SCHEMA_VERSION,
                        dispatch: lease.dispatch,
                        lease: lease.lease,
                        accepted: p::RequiredTrue,
                        receipt: receipt.receipt,
                        authority_epoch: lease.authority_epoch,
                        fence: lease.fence,
                    },
                ))
            }
            p::RemoteWireCommand::Probe(request) => {
                request.validate()?;
                let result = self.service.probe(&request.lease)?;
                result.validate()?;
                Ok(p::RemoteWireResponse::Probe(result))
            }
            p::RemoteWireCommand::Cancel(request) => {
                request.validate()?;
                let result = self.service.cancel(&request.lease)?;
                result.validate()?;
                Ok(p::RemoteWireResponse::Cancel(result))
            }
            p::RemoteWireCommand::Receipt(request) => {
                request.validate()?;
                let receipt = self.service.fetch_receipt(&request.receipt)?;
                if receipt.lease != request.lease
                    || receipt.dispatch != request.dispatch
                    || receipt.authority_epoch != request.authority_epoch
                    || receipt.fence != request.fence
                {
                    return Err(p::Error("remote receipt request binding failed".into()));
                }
                Ok(p::RemoteWireResponse::Receipt(receipt))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireReplayRecord {
    schema_version: p::SchemaVersion,
    request: p::RemoteWireRequestRef,
    nonce: p::Nonce,
    digest: p::SchemaDigest,
}

struct WireReplayLedger {
    root: PathBuf,
    records: Mutex<BTreeMap<p::RemoteWireRequestRef, WireReplayRecord>>,
    nonces: Mutex<BTreeMap<p::Nonce, p::RemoteWireRequestRef>>,
}

impl WireReplayLedger {
    fn open(root: impl AsRef<Path>) -> p::Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)
            .map_err(|_| p::Error("remote wire replay directory is unavailable".into()))?;
        let mut records = BTreeMap::new();
        let mut nonces = BTreeMap::new();
        let entries = fs::read_dir(&root)
            .map_err(|_| p::Error("remote wire replay directory cannot be listed".into()))?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|_| p::Error("remote wire replay entry is unreadable".into()))?;
        for entry in entries {
            if !entry
                .file_type()
                .map_err(|_| p::Error("remote wire replay type is unreadable".into()))?
                .is_file()
                || entry.path().extension().and_then(|value| value.to_str()) != Some("json")
            {
                return Err(p::Error(
                    "remote wire replay directory contains an unexpected entry".into(),
                ));
            }
            let bytes = fs::read(entry.path())
                .map_err(|_| p::Error("remote wire replay record cannot be read".into()))?;
            let record: WireReplayRecord = serde_json::from_slice(&bytes)
                .map_err(|_| p::Error("remote wire replay record is malformed".into()))?;
            validate_wire_record(&record)?;
            if records
                .insert(record.request.clone(), record.clone())
                .is_some()
                || nonces
                    .insert(record.nonce.clone(), record.request.clone())
                    .is_some()
            {
                return Err(p::Error(
                    "remote wire replay ledger has duplicate identity".into(),
                ));
            }
        }
        Ok(Self {
            root,
            records: Mutex::new(records),
            nonces: Mutex::new(nonces),
        })
    }

    fn claim(&self, envelope: &p::RemoteWireEnvelope) -> p::Result<()> {
        let record = WireReplayRecord {
            schema_version: p::M4_SCHEMA_VERSION,
            request: envelope.request.clone(),
            nonce: envelope.nonce.clone(),
            digest: envelope.digest.clone(),
        };
        validate_wire_record(&record)?;
        let mut records = self
            .records
            .lock()
            .map_err(|_| p::Error("remote wire replay ledger is unavailable".into()))?;
        let mut nonces = self
            .nonces
            .lock()
            .map_err(|_| p::Error("remote wire nonce ledger is unavailable".into()))?;
        if let Some(previous) = records.get(&record.request) {
            return if previous == &record {
                Ok(())
            } else {
                Err(p::Error("remote wire request id changed semantics".into()))
            };
        }
        if nonces.contains_key(&record.nonce) {
            return Err(p::Error("remote wire nonce was replayed".into()));
        }
        let identity = p::canonical_digest(&record.request)?;
        let name = identity
            .0
            .strip_prefix("sha256:")
            .ok_or_else(|| p::Error("remote wire request digest is invalid".into()))?;
        let path = self.root.join(format!("{name}.json"));
        let bytes = serde_json::to_vec(&record)
            .map_err(|_| p::Error("remote wire replay record cannot be encoded".into()))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| p::Error("remote wire replay record cannot be created".into()))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| p::Error("remote wire replay record cannot be persisted".into()))?;
        records.insert(record.request.clone(), record.clone());
        nonces.insert(record.nonce.clone(), record.request);
        Ok(())
    }
}

fn validate_wire_record(record: &WireReplayRecord) -> p::Result<()> {
    if record.schema_version.0 == 0
        || record.request.0.trim().is_empty()
        || record.nonce.0.trim().is_empty()
        || record.digest.0.trim().is_empty()
    {
        return Err(p::Error("remote wire replay record is incomplete".into()));
    }
    Ok(())
}

pub fn transport_identity_digest(certificate_der: &[u8]) -> p::TransportIdentityDigest {
    let digest = Sha256::digest(certificate_der);
    p::TransportIdentityDigest(format!("sha256:{digest:x}"))
}

fn verify_peer_identity(
    certificates: Option<&[CertificateDer<'static>]>,
    expected: &p::TransportIdentityDigest,
) -> p::Result<()> {
    let certificate = certificates
        .and_then(|certificates| certificates.first())
        .ok_or_else(|| p::Error("remote TLS peer did not present an identity".into()))?;
    if transport_identity_digest(certificate.as_ref()) != *expected {
        return Err(p::Error("remote TLS peer identity drifted".into()));
    }
    Ok(())
}

fn read_private_file(path: &Path, label: &str) -> p::Result<Vec<u8>> {
    if path.as_os_str().is_empty() || !path.is_file() {
        return Err(p::Error(format!("TLS {label} file is unavailable")));
    }
    fs::read(path).map_err(|_| p::Error(format!("TLS {label} file cannot be read")))
}

fn validate_endpoint(value: &str) -> p::Result<Url> {
    let endpoint =
        Url::parse(value).map_err(|_| p::Error("remote TLS endpoint cannot be parsed".into()))?;
    if endpoint.scheme() != "https"
        || endpoint.host_str().is_none()
        || endpoint.port_or_known_default().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || (endpoint.path() != "/" && !endpoint.path().is_empty())
    {
        return Err(p::Error(
            "remote TLS endpoint is outside its profile".into(),
        ));
    }
    Ok(endpoint)
}

fn endpoint_addresses(endpoint: &Url) -> p::Result<Vec<SocketAddr>> {
    let host = endpoint
        .host_str()
        .ok_or_else(|| p::Error("remote TLS endpoint has no host".into()))?;
    let port = endpoint
        .port_or_known_default()
        .ok_or_else(|| p::Error("remote TLS endpoint has no port".into()))?;
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| p::Error("remote TLS endpoint could not be resolved".into()))?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        Err(p::Error("remote TLS endpoint has no address".into()))
    } else {
        Ok(addresses)
    }
}

fn command_path(command: &p::RemoteWireCommand) -> &'static str {
    match command {
        p::RemoteWireCommand::Dispatch { .. } => "/v1/dispatch",
        p::RemoteWireCommand::Probe(_) => "/v1/probe",
        p::RemoteWireCommand::Cancel(_) => "/v1/cancel",
        p::RemoteWireCommand::Receipt(_) => "/v1/receipt",
    }
}

fn read_http_request(reader: &mut impl Read, max_body: usize) -> p::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let mut expected = None;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| p::Error("remote HTTP request could not be read".into()))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > max_body + MAX_HTTP_HEADER {
            return Err(p::Error(
                "remote HTTP request exceeded its byte limit".into(),
            ));
        }
        if expected.is_none() {
            if let Some(header_end) = find_header_end(&bytes) {
                if header_end > MAX_HTTP_HEADER {
                    return Err(p::Error(
                        "remote HTTP header exceeded its byte limit".into(),
                    ));
                }
                let content_length = request_content_length(&bytes[..header_end])?;
                if content_length > max_body {
                    return Err(p::Error("remote HTTP request body is too large".into()));
                }
                expected = Some(header_end + content_length);
            }
        }
        if expected.is_some_and(|expected| bytes.len() >= expected) {
            break;
        }
    }
    if expected != Some(bytes.len()) {
        return Err(p::Error("remote HTTP request length is invalid".into()));
    }
    Ok(bytes)
}

fn read_http_response_bytes(reader: &mut impl Read, max_body: usize) -> p::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let mut expected = None;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| p::Error("remote HTTP reply could not be read".into()))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > max_body + MAX_HTTP_HEADER {
            return Err(p::Error("remote HTTP reply exceeded its byte limit".into()));
        }
        if expected.is_none() {
            if let Some(header_end) = find_header_end(&bytes) {
                if header_end > MAX_HTTP_HEADER {
                    return Err(p::Error(
                        "remote HTTP reply header exceeded its byte limit".into(),
                    ));
                }
                let content_length = response_content_length(&bytes[..header_end])?;
                if content_length > max_body {
                    return Err(p::Error("remote HTTP reply body is too large".into()));
                }
                expected = Some(header_end + content_length);
            }
        }
        if expected.is_some_and(|expected| bytes.len() >= expected) {
            break;
        }
    }
    if expected != Some(bytes.len()) {
        return Err(p::Error("remote HTTP reply length is invalid".into()));
    }
    Ok(bytes)
}

fn request_content_length(header: &[u8]) -> p::Result<usize> {
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut request = httparse::Request::new(&mut headers);
    if !request
        .parse(header)
        .map_err(|_| p::Error("remote HTTP request header is malformed".into()))?
        .is_complete()
    {
        return Err(p::Error("remote HTTP request header is incomplete".into()));
    }
    content_length(request.headers)
}

fn response_content_length(header: &[u8]) -> p::Result<usize> {
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut response = httparse::Response::new(&mut headers);
    if !response
        .parse(header)
        .map_err(|_| p::Error("remote HTTP reply header is malformed".into()))?
        .is_complete()
    {
        return Err(p::Error("remote HTTP reply header is incomplete".into()));
    }
    content_length(response.headers)
}

fn parse_http_request(bytes: &[u8], max_body: usize) -> p::Result<(&str, &[u8])> {
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut request = httparse::Request::new(&mut headers);
    let status = request
        .parse(bytes)
        .map_err(|_| p::Error("remote HTTP request header is malformed".into()))?;
    let header_end = match status {
        httparse::Status::Complete(length) => length,
        httparse::Status::Partial => {
            return Err(p::Error("remote HTTP request header is incomplete".into()))
        }
    };
    if request.method != Some("POST") || request.version != Some(1) {
        return Err(p::Error(
            "remote HTTP request method or version is invalid".into(),
        ));
    }
    let length = content_length(request.headers)?;
    if length > max_body || bytes.len() != header_end + length {
        return Err(p::Error(
            "remote HTTP request body length is invalid".into(),
        ));
    }
    let path = request
        .path
        .ok_or_else(|| p::Error("remote HTTP request path is absent".into()))?;
    Ok((path, &bytes[header_end..]))
}

fn parse_http_response(bytes: &[u8], max_body: usize) -> p::Result<&[u8]> {
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut response = httparse::Response::new(&mut headers);
    let status = response
        .parse(bytes)
        .map_err(|_| p::Error("remote HTTP reply header is malformed".into()))?;
    let header_end = match status {
        httparse::Status::Complete(length) => length,
        httparse::Status::Partial => {
            return Err(p::Error("remote HTTP reply header is incomplete".into()))
        }
    };
    if response.code != Some(200) || response.version != Some(1) {
        return Err(p::Error("remote HTTP reply was not successful".into()));
    }
    let length = content_length(response.headers)?;
    if length > max_body || bytes.len() != header_end + length {
        return Err(p::Error("remote HTTP reply body length is invalid".into()));
    }
    Ok(&bytes[header_end..])
}

fn content_length(headers: &[httparse::Header<'_>]) -> p::Result<usize> {
    let values = headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("content-length"))
        .collect::<Vec<_>>();
    if values.len() != 1
        || headers
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case("transfer-encoding"))
    {
        return Err(p::Error("remote HTTP framing is ambiguous".into()));
    }
    std::str::from_utf8(values[0].value)
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| p::Error("remote HTTP content length is invalid".into()))
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

fn write_http_response(writer: &mut impl Write, body: &[u8], max_body: usize) -> p::Result<()> {
    if body.len() > max_body {
        return Err(p::Error("remote HTTP reply body is too large".into()));
    }
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    writer
        .write_all(header.as_bytes())
        .and_then(|_| writer.write_all(body))
        .and_then(|_| writer.flush())
        .map_err(|_| p::Error("remote HTTP reply could not be written".into()))
}
