//! Authenticated Web-to-core transport. The browser never sees this channel.
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use forme_protocol as p;

use crate::web::config::WebCoreBridgeConfig;
use crate::web::identity::CoreSessionBinding;

mod response;
use response::{parse_response, MAX_WIRE_BYTES};
#[cfg(test)]
#[path = "core/credential_tests.rs"]
mod credential_tests;

const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const ASSERTION_TTL_MS: i64 = 30_000;

pub(super) struct CoreBridgeClient {
    address: SocketAddr,
    service_token: String,
    signing_key: SigningKey,
    install_ref: String,
}

pub(super) struct CoreBridgeResponse {
    pub(super) status: u16,
    pub(super) content_type: Option<String>,
    pub(super) body: Vec<u8>,
}

impl CoreBridgeClient {
    pub(super) fn connect(config: &WebCoreBridgeConfig, install_ref: &str) -> p::Result<Self> {
        let service_token = read_secret(&config.credential_path, "core bridge credential")?;
        let signing_key = read_signing_key(&config.signing_key_path)?;
        Ok(Self {
            address: config.address,
            service_token,
            signing_key,
            install_ref: install_ref.to_owned(),
        })
    }

    pub(super) fn request(
        &self,
        binding: &CoreSessionBinding,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> p::Result<CoreBridgeResponse> {
        if !matches!(method, "GET" | "POST" | "PUT" | "DELETE")
            || !path.starts_with("/v1/")
            || path.len() > 2048
            || path.bytes().any(|byte| byte == b'\r' || byte == b'\n')
        {
            return Err(p::Error("core bridge route is invalid".into()));
        }
        let limit = if method == "POST" && path == "/v1/artifacts/upload" {
            p::ArtifactUploadCommand::MAX_REQUEST_BYTES
        } else {
            256 * 1024
        };
        if body.len() > limit {
            return Err(p::Error("core bridge request exceeds its bound".into()));
        }
        let request_target = path;
        let request_digest = p::canonical_digest(&(
            p::SchemaVersion(1),
            "forme-web-core-request-v1",
            method,
            &request_target,
            body,
        ))?;
        let assertion = self.assertion(binding, request_digest.clone())?;
        let encoded_assertion = hex_encode(
            &serde_json::to_vec(&assertion)
                .map_err(|_| p::Error("core session assertion could not be encoded".into()))?,
        );
        let mut stream = TcpStream::connect_timeout(&self.address, REQUEST_TIMEOUT)
            .map_err(|_| p::Error("the local core is unreachable".into()))?;
        stream
            .set_read_timeout(Some(REQUEST_TIMEOUT))
            .and_then(|_| stream.set_write_timeout(Some(REQUEST_TIMEOUT)))
            .map_err(|_| p::Error("core bridge socket limits could not be applied".into()))?;
        let host = self.address.to_string();
        let mut head = format!(
            "{method} {request_target} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\n\
             X-Forme-Core-Assertion: {encoded_assertion}\r\nX-Forme-Request-Digest: {}\r\n\
             Accept: application/json\r\nConnection: close\r\n",
            self.service_token, request_digest.0
        );
        if method != "GET" {
            head.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            ));
        }
        if method == "POST" && path == "/v1/brain/projects/supervision" {
            let command: p::ProjectSupervisionCommand = serde_json::from_slice(body)
                .map_err(|_| p::Error("project supervision command is invalid".into()))?;
            command.validate()?;
            head.push_str(&format!("Idempotency-Key: {}\r\n", command.command.0));
        }
        head.push_str("\r\n");
        stream
            .write_all(head.as_bytes())
            .and_then(|_| stream.write_all(body))
            .and_then(|_| stream.flush())
            .map_err(|_| p::Error("core bridge request could not be sent".into()))?;
        let mut response = Vec::new();
        stream
            .take((MAX_WIRE_BYTES + 1) as u64)
            .read_to_end(&mut response)
            .map_err(|_| p::Error("core bridge response could not be read".into()))?;
        parse_response(&response)
    }

    pub(super) fn fence_session(
        &self,
        binding: &CoreSessionBinding,
        session_digest: Option<p::SchemaDigest>,
        minimum_auth_epoch: u64,
    ) -> p::Result<()> {
        let update = p::WebSessionFenceUpdate {
            schema_version: p::SchemaVersion(1),
            install_ref: self.install_ref.clone(),
            owner: binding.owner.clone(),
            session_digest,
            minimum_auth_epoch,
        };
        update.validate()?;
        let body = serde_json::to_vec(&update)
            .map_err(|_| p::Error("core session fence could not be encoded".into()))?;
        let response = self.request(binding, "POST", "/v1/web/session-fence", &body)?;
        if response.status != 204 && response.status != 200 {
            return Err(p::Error("core session fence was not accepted".into()));
        }
        Ok(())
    }

    fn assertion(
        &self,
        binding: &CoreSessionBinding,
        request_digest: p::SchemaDigest,
    ) -> p::Result<p::CoreSessionAssertion> {
        let mut assertion = p::CoreSessionAssertion {
            schema_version: p::SchemaVersion(1),
            install_ref: self.install_ref.clone(),
            owner: binding.owner.clone(),
            session_digest: p::SchemaDigest(binding.session_digest.clone()),
            auth_epoch: binding.auth_epoch,
            audience: "forme-core".into(),
            request_digest,
            expires_at: now_ms().saturating_add(ASSERTION_TTL_MS),
            nonce: random_nonce()?,
            signature: Vec::new(),
        };
        assertion.signature = vec![0; 64];
        assertion.validate()?;
        let digest = assertion.signing_digest()?;
        assertion.signature = self
            .signing_key
            .sign(digest.0.as_bytes())
            .to_bytes()
            .to_vec();
        Ok(assertion)
    }
}

fn read_secret(path: &Path, name: &str) -> p::Result<String> {
    let value = String::from_utf8(read_credential_file(path, 258)?)
        .map_err(|_| p::Error(format!("{name} is unavailable")))?;
    let value = value.trim_end_matches(['\r', '\n']).to_owned();
    if !(32..=256).contains(&value.len()) || value.chars().any(char::is_whitespace) {
        return Err(p::Error(format!("{name} is malformed")));
    }
    Ok(value)
}

fn read_signing_key(path: &Path) -> p::Result<SigningKey> {
    let bytes = read_credential_file(path, 64)?;
    let raw = if bytes.len() == 32 {
        bytes
    } else if bytes.len() == 64 && bytes.iter().all(u8::is_ascii_hexdigit) {
        hex_decode(&bytes)
            .ok_or_else(|| p::Error("Web assertion signing key is malformed".into()))?
    } else {
        return Err(p::Error("Web assertion signing key is malformed".into()));
    };
    let raw: [u8; 32] = raw
        .try_into()
        .map_err(|_| p::Error("Web assertion signing key is malformed".into()))?;
    Ok(SigningKey::from_bytes(&raw))
}

fn read_credential_file(path: &Path, limit: u64) -> p::Result<Vec<u8>> {
    crate::web::config::check_path(path)?;
    let unavailable = || p::Error("Web bridge credential file is unavailable or invalid".into());
    let file = std::fs::File::open(path).map_err(|_| unavailable())?;
    let metadata = file.metadata().map_err(|_| unavailable())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(unavailable());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable())?;
    if bytes.len() as u64 != metadata.len() {
        return Err(unavailable());
    }
    Ok(bytes)
}

fn random_nonce() -> p::Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| p::Error("core assertion nonce entropy is unavailable".into()))?;
    Ok(hex_encode(&bytes))
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn hex_decode(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    bytes
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some(((high << 4) | low) as u8)
        })
        .collect()
}
