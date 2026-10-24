//! Typed Web-to-host product configuration bridge.
//!
//! This channel can stage configuration and fixed owner maintenance. It cannot call a model,
//! inspect the brain store, or submit an AgentExecution request.
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

use forme_protocol as p;
use serde::{de::DeserializeOwned, Serialize};
use zeroize::Zeroizing;

const MAX_WIRE_BYTES: usize = 512 * 1024;
const MAX_BODY_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct LocalConfigurationClient {
    address: SocketAddr,
    credential: Zeroizing<String>,
}

impl LocalConfigurationClient {
    pub(super) fn connect(address: SocketAddr, credential_path: &Path) -> p::Result<Self> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(p::Error("configuration bridge address is invalid".into()));
        }
        super::config::check_path(credential_path)?;
        let file = std::fs::File::open(credential_path)
            .map_err(|_| p::Error("configuration bridge credential is unavailable".into()))?;
        if !file.metadata().map_err(|_| invalid())?.is_file() {
            return Err(invalid());
        }
        let mut value = Zeroizing::new(String::new());
        file.take(257)
            .read_to_string(&mut value)
            .map_err(|_| invalid())?;
        let value = value.trim_end_matches(['\r', '\n']);
        if !(32..=256).contains(&value.len()) || value.chars().any(char::is_whitespace) {
            return Err(invalid());
        }
        Ok(Self {
            address,
            credential: Zeroizing::new(value.to_owned()),
        })
    }

    pub(super) fn snapshot(&self) -> p::Result<p::ProductConfigView> {
        self.request("GET", "/v1/host/configuration", None::<&()>)
    }

    pub(super) fn snapshot_v2(&self) -> p::Result<p::ProductConfigViewV2> {
        self.request("GET", "/v1/host/configuration/v2", None::<&()>)
    }

    pub(super) fn bind_owner(
        &self,
        operation_id: &str,
        expected_revision: u64,
        owner: p::OwnerPrincipalRef,
    ) -> p::Result<p::ProductConfigResponse> {
        self.request(
            "POST",
            "/v1/host/configuration/owner",
            Some(&p::ProductConfigOwnerBinding {
                schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
                operation_id: operation_id.to_owned(),
                expected_revision,
                owner,
            }),
        )
    }

    pub(super) fn apply(
        &self,
        patch: p::ProductConfigPatch,
    ) -> p::Result<p::ProductConfigResponse> {
        self.request("POST", "/v1/host/configuration", Some(&patch))
    }

    pub(super) fn apply_v2(
        &self,
        patch: p::ProductConfigPatchV2,
    ) -> p::Result<p::ProductConfigResponseV2> {
        self.request("POST", "/v1/host/configuration", Some(&patch))
    }

    pub(super) fn validate(
        &self,
        request: p::ProductConfigValidationRequest,
    ) -> p::Result<p::ProductConfigValidationReport> {
        self.request("POST", "/v1/host/configuration/validate", Some(&request))
    }

    pub(super) fn activate(
        &self,
        request: p::ProductCoreActivation,
    ) -> p::Result<p::ProductCoreActivationResponse> {
        self.request("POST", "/v1/host/configuration/activate", Some(&request))
    }

    pub(super) fn operation(&self, id: &str) -> p::Result<p::MaintenanceOperation> {
        p::validate_product_id(id)?;
        self.request(
            "GET",
            &format!("/v1/host/configuration/operations/{id}"),
            None::<&()>,
        )
    }

    pub(super) fn maintenance(&self) -> p::Result<p::ProductMaintenanceSnapshot> {
        self.request("GET", "/v1/host/configuration/maintenance", None::<&()>)
    }

    pub(super) fn maintain(
        &self,
        request: p::ProductMaintenanceRequest,
    ) -> p::Result<p::MaintenanceOperation> {
        self.request("POST", "/v1/host/configuration/maintenance", Some(&request))
    }

    pub(super) fn maintenance_operation(&self, id: &str) -> p::Result<p::MaintenanceOperation> {
        p::validate_product_id(id)?;
        self.request(
            "GET",
            &format!("/v1/host/configuration/maintenance/operations/{id}"),
            None::<&()>,
        )
    }

    fn request<T: DeserializeOwned, B: Serialize>(
        &self,
        method: &str,
        path: &str,
        body: Option<&B>,
    ) -> p::Result<T> {
        if !matches!(
            (method, path),
            ("GET", "/v1/host/configuration")
                | ("GET", "/v1/host/configuration/v2")
                | ("POST", "/v1/host/configuration")
                | ("POST", "/v1/host/configuration/owner")
                | ("POST", "/v1/host/configuration/validate")
                | ("POST", "/v1/host/configuration/activate")
                | ("GET", "/v1/host/configuration/maintenance")
                | ("POST", "/v1/host/configuration/maintenance")
        ) && !(method == "GET"
            && path
                .strip_prefix("/v1/host/configuration/operations/")
                .or_else(|| path.strip_prefix("/v1/host/configuration/maintenance/operations/"))
                .is_some_and(|id| p::validate_product_id(id).is_ok()))
        {
            return Err(invalid());
        }
        let encoded = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| invalid())?
            .unwrap_or_default();
        if encoded.len() > MAX_BODY_BYTES {
            return Err(invalid());
        }
        let mut stream = TcpStream::connect_timeout(&self.address, REQUEST_TIMEOUT)
            .map_err(|_| unavailable())?;
        stream
            .set_read_timeout(Some(REQUEST_TIMEOUT))
            .and_then(|_| stream.set_write_timeout(Some(REQUEST_TIMEOUT)))
            .map_err(|_| unavailable())?;
        let content_type = if body.is_some() {
            "Content-Type: application/json\r\n"
        } else {
            ""
        };
        let header = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\n{content_type}Content-Length: {}\r\nConnection: close\r\n\r\n",
            self.address,
            self.credential.as_str(),
            encoded.len()
        );
        stream
            .write_all(header.as_bytes())
            .and_then(|_| stream.write_all(&encoded))
            .map_err(|_| unavailable())?;
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let mut wire = Vec::new();
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(unavailable)?;
            stream
                .set_read_timeout(Some(remaining))
                .map_err(|_| unavailable())?;
            let mut chunk = [0_u8; 4096];
            let count = stream.read(&mut chunk).map_err(|_| unavailable())?;
            if count == 0 {
                break;
            }
            if wire.len().saturating_add(count) > MAX_WIRE_BYTES {
                return Err(invalid());
            }
            wire.extend_from_slice(&chunk[..count]);
        }
        let (status, response_body) = parse_response(&wire)?;
        if status == 422 {
            return Err(p::Error(
                "configuration patch rejected before publication".into(),
            ));
        }
        if status == 409 {
            return Err(p::Error(
                "configuration revision is stale or operation id was reused".into(),
            ));
        }
        if status != 200 {
            return Err(p::Error(
                "managed product configuration request was rejected".into(),
            ));
        }
        serde_json::from_slice(response_body).map_err(|_| invalid())
    }
}

fn parse_response(bytes: &[u8]) -> p::Result<(u16, &[u8])> {
    let header_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(unavailable)?;
    if header_end > 8192 {
        return Err(invalid());
    }
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(|_| invalid())?;
    let mut lines = header.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(invalid)?;
    let mut content_length = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && content_length.is_some())
        {
            return Err(invalid());
        }
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(value.trim().parse::<usize>().map_err(|_| invalid())?);
        }
    }
    let body = &bytes[header_end + 4..];
    if content_length != Some(body.len()) {
        return Err(invalid());
    }
    Ok((status, body))
}

fn invalid() -> p::Error {
    p::Error("managed product configuration response is invalid".into())
}

fn unavailable() -> p::Error {
    p::Error("managed product configuration service is unavailable".into())
}
