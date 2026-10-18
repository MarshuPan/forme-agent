//! Loopback client for the local gateway daemon (architecture/09 §2.3).
//!
//! The CLI's default path: it never opens the authority store, so the one
//! production runtime held by `forme-gatewayd` stays the only writer. The
//! client speaks the daemon's owner-authenticated HTTP surface over a plain
//! loopback socket and refuses any non-loopback address.

use std::fmt;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use forme_harness::EventStream;
use forme_protocol as p;
use serde::Deserialize;

use crate::RunGateway;

const CSRF_HEADER: &str = "x-forme-csrf";
const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Owner bearer token for the local daemon; never printed.
#[derive(Clone)]
struct ClientToken(String);

impl fmt::Debug for ClientToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClientToken(<redacted>)")
    }
}

#[derive(Debug, Clone)]
pub struct HttpGatewayClient {
    address: SocketAddr,
    origin: String,
    token: ClientToken,
    agent_profile: p::AgentProfileRef,
    manifest_digest: p::SchemaDigest,
    request_timeout: Duration,
    wait_timeout: Duration,
}

#[derive(Deserialize)]
struct ManifestSummary {
    agent_profile: p::AgentProfileRef,
    digest: p::SchemaDigest,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

#[derive(Deserialize)]
struct AnswerBody {
    answer: Option<String>,
}

impl HttpGatewayClient {
    /// Connects to the daemon named by `FORME_GATEWAY_URL`, authenticating
    /// with `FORME_GATEWAY_TOKEN` or the token file the daemon provisioned.
    pub fn from_environment() -> p::Result<Self> {
        let url = std::env::var("FORME_GATEWAY_URL").map_err(|_| {
            p::Error(
                "no local gateway is configured: start forme-gatewayd and set FORME_GATEWAY_URL, \
                 or run the CLI with --embedded"
                    .into(),
            )
        })?;
        let token = match std::env::var("FORME_GATEWAY_TOKEN") {
            Ok(token) => token,
            Err(_) => {
                let path = std::env::var_os("FORME_GATEWAY_TOKEN_PATH")
                    .map(PathBuf::from)
                    .unwrap_or_else(crate::server::default_token_path);
                std::fs::read_to_string(path)
                    .map_err(|_| p::Error("the local gateway token is unreadable".into()))?
            }
        };
        let timeout = env_duration("FORME_GATEWAY_TIMEOUT_MS", 30_000)?;
        let wait = env_duration("FORME_GATEWAY_WAIT_MS", 600_000)?;
        Self::connect(&url, token.trim().to_owned(), timeout, wait)
    }

    pub fn connect(
        url: &str,
        token: String,
        request_timeout: Duration,
        wait_timeout: Duration,
    ) -> p::Result<Self> {
        let authority = url
            .strip_prefix("http://")
            .map(|rest| rest.trim_end_matches('/'))
            .filter(|rest| !rest.is_empty() && !rest.contains('/'))
            .ok_or_else(|| {
                p::Error("the local gateway URL must be http://<loopback-host>:<port>".into())
            })?;
        let address = authority
            .to_socket_addrs()
            .map_err(|_| p::Error("the local gateway address cannot be resolved".into()))?
            .find(|address| address.ip().is_loopback())
            .ok_or_else(|| p::Error("the local gateway must listen on loopback".into()))?;
        if token.len() < 32 || token.chars().any(char::is_whitespace) {
            return Err(p::Error("the local gateway token is malformed".into()));
        }
        if request_timeout.is_zero() || wait_timeout.is_zero() {
            return Err(p::Error("gateway client timeouts must be positive".into()));
        }
        let mut client = Self {
            address,
            origin: format!("http://{authority}"),
            token: ClientToken(token),
            agent_profile: p::AgentProfileRef("agent:forme-local".into()),
            manifest_digest: p::SchemaDigest(String::new()),
            request_timeout,
            wait_timeout,
        };
        let manifest: ManifestSummary = client.get_json("/v1/runtime/manifest")?;
        if manifest.agent_profile.0.trim().is_empty() || manifest.digest.0.trim().is_empty() {
            return Err(p::Error(
                "the local gateway returned an incomplete runtime manifest".into(),
            ));
        }
        client.agent_profile = manifest.agent_profile;
        client.manifest_digest = manifest.digest;
        Ok(client)
    }

    /// Digest of the daemon's composition manifest observed at connect time.
    pub fn manifest_digest(&self) -> &p::SchemaDigest {
        &self.manifest_digest
    }

    /// Data controls share the daemon's owner authentication and CSRF proof.
    pub fn data_control(
        &self,
        operation: &str,
        request: &serde_json::Value,
    ) -> p::Result<serde_json::Value> {
        if !matches!(
            operation,
            "backup" | "verify-backup" | "restore-plan" | "restore" | "lifecycle"
        ) {
            return Err(p::Error("unknown data operation".into()));
        }
        let encoded = serde_json::to_vec(request)
            .map_err(|_| p::Error("data command encoding failed".into()))?;
        let response = self.exchange("POST", &format!("/v1/data/{operation}"), Some(&encoded))?;
        serde_json::from_slice(&response)
            .map_err(|_| p::Error("data control response is invalid".into()))
    }

    pub fn brain_dashboard(&self, workspace: Option<&str>) -> p::Result<serde_json::Value> {
        let path = workspace
            .map(|s| format!("/v1/brain/dashboard?workspace={}", path_segment(s)))
            .unwrap_or_else(|| "/v1/brain/dashboard".into());
        self.get_json(&path)
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> p::Result<T> {
        let body = self.exchange("GET", path, None)?;
        serde_json::from_slice(&body)
            .map_err(|_| p::Error("the local gateway returned an unexpected response".into()))
    }

    fn exchange(&self, method: &str, path: &str, body: Option<&[u8]>) -> p::Result<Vec<u8>> {
        let mut stream = TcpStream::connect_timeout(&self.address, self.request_timeout)
            .map_err(|_| p::Error("the local gateway is unreachable".into()))?;
        stream
            .set_read_timeout(Some(self.request_timeout))
            .and_then(|_| stream.set_write_timeout(Some(self.request_timeout)))
            .map_err(|_| p::Error("the local gateway connection cannot be bounded".into()))?;
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\n\
             Accept: application/json\r\nConnection: close\r\n",
            self.address, self.token.0
        );
        if method != "GET" {
            head.push_str(&format!("Origin: {}\r\n{CSRF_HEADER}: 1\r\n", self.origin));
        }
        let body = body.unwrap_or_default();
        if method != "GET" {
            head.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            ));
        }
        head.push_str("\r\n");
        stream
            .write_all(head.as_bytes())
            .and_then(|_| stream.write_all(body))
            .and_then(|_| stream.flush())
            .map_err(|_| p::Error("the local gateway request could not be sent".into()))?;
        let mut response = Vec::new();
        stream
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut response)
            .map_err(|_| p::Error("the local gateway response could not be read".into()))?;
        if response.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(p::Error(
                "the local gateway response exceeds its bound".into(),
            ));
        }
        let (status, body) = parse_response(&response)?;
        if (200..300).contains(&status) {
            return Ok(body);
        }
        let reason = serde_json::from_slice::<ErrorBody>(&body)
            .map(|body| body.error)
            .unwrap_or_else(|_| "request failed".into());
        Err(p::Error(format!(
            "the local gateway rejected the request ({status}): {reason}"
        )))
    }
}

impl RunGateway for HttpGatewayClient {
    fn submit_run(&self, request: p::RunRequest) -> p::Result<p::RunId> {
        let body = serde_json::to_vec(&request)
            .map_err(|_| p::Error("run request could not be encoded".into()))?;
        let response = self.exchange("POST", "/v1/runs", Some(&body))?;
        serde_json::from_slice(&response)
            .map_err(|_| p::Error("the local gateway returned an unexpected run id".into()))
    }

    fn stream_run(&self, run: p::RunId) -> EventStream {
        let mut events = Vec::<p::Event>::new();
        loop {
            let after = events.last().map(|event| event.stream_seq).unwrap_or(0);
            let path = format!("/v1/runs/{}/events?after={after}", path_segment(&run.0));
            let Ok(page) = self.exchange("GET", &path, None) else {
                break;
            };
            let page = parse_event_page(&page)
                .into_iter()
                .filter(|event| event.stream_seq > after)
                .collect::<Vec<_>>();
            // A page that does not advance the cursor ends the stream rather
            // than looping on a misbehaving server.
            if page.is_empty() {
                break;
            }
            events.extend(page);
        }
        EventStream::new(events)
    }

    fn wait_run(&self, run: p::RunId) -> p::Result<p::RunResult> {
        let path = format!("/v1/runs/{}", path_segment(&run.0));
        let deadline = Instant::now() + self.wait_timeout;
        loop {
            let summary: p::RunSummary = self.get_json(&path)?;
            if let Some(result) = summary.result {
                return Ok(result);
            }
            if summary.status == p::RunStatus::Waiting {
                return Err(p::Error(
                    "run is waiting for owner action in the local gateway".into(),
                ));
            }
            if Instant::now() >= deadline {
                return Err(p::Error(
                    "run did not reach a terminal state before the wait bound".into(),
                ));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn answer(&self, run: p::RunId) -> p::Result<Option<String>> {
        let body: AnswerBody =
            self.get_json(&format!("/v1/runs/{}/answer", path_segment(&run.0)))?;
        Ok(body.answer)
    }

    fn agent_profile(&self) -> p::AgentProfileRef {
        self.agent_profile.clone()
    }

    fn runtime_manifest_digest(&self) -> p::Result<p::SchemaDigest> {
        Ok(self.manifest_digest.clone())
    }
}

fn parse_response(response: &[u8]) -> p::Result<(u16, Vec<u8>)> {
    let malformed = || p::Error("the local gateway response is malformed".into());
    let head_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(malformed)?;
    let head = std::str::from_utf8(&response[..head_end]).map_err(|_| malformed())?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(malformed)?;
    let mut chunked = false;
    let mut length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.eq_ignore_ascii_case("chunked");
        } else if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.parse::<usize>().map_err(|_| malformed())?);
        }
    }
    let raw = &response[head_end + 4..];
    let body = if chunked {
        decode_chunked(raw).ok_or_else(malformed)?
    } else if let Some(length) = length {
        raw.get(..length).ok_or_else(malformed)?.to_vec()
    } else {
        raw.to_vec()
    };
    Ok((status, body))
}

fn decode_chunked(mut raw: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line_end = raw.windows(2).position(|window| window == b"\r\n")?;
        let size_text = std::str::from_utf8(&raw[..line_end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        raw = &raw[line_end + 2..];
        if size == 0 {
            return Some(body);
        }
        body.extend_from_slice(raw.get(..size)?);
        raw = raw.get(size + 2..)?;
    }
}

fn parse_event_page(body: &[u8]) -> Vec<p::Event> {
    String::from_utf8_lossy(body)
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|json| serde_json::from_str::<p::Event>(json).ok())
        .collect()
}

/// Percent-encodes one path segment; identifiers keep `:` readable.
fn path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b':') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn env_duration(key: &str, fallback_ms: u64) -> p::Result<Duration> {
    match std::env::var(key) {
        Ok(value) => value
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .map(Duration::from_millis)
            .ok_or_else(|| p::Error(format!("environment variable {key} is invalid"))),
        Err(_) => Ok(Duration::from_millis(fallback_ms)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_decode_length_and_chunked_bodies() {
        let fixed = b"HTTP/1.1 202 Accepted\r\ncontent-length: 5\r\n\r\nhello";
        assert_eq!(parse_response(fixed).unwrap(), (202, b"hello".to_vec()));
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked).unwrap(), (200, b"abcde".to_vec()));
        assert!(parse_response(b"HTTP/1.1 200 OK\r\ncontent-length: 9\r\n\r\nshort").is_err());
        assert!(parse_response(b"not http").is_err());
    }

    #[test]
    fn only_plain_loopback_urls_are_accepted() {
        let token = "t".repeat(40);
        for url in [
            "https://127.0.0.1:1",
            "http://192.0.2.1:9",
            "http://127.0.0.1:1/v1",
            "127.0.0.1:1",
        ] {
            assert!(HttpGatewayClient::connect(
                url,
                token.clone(),
                Duration::from_millis(50),
                Duration::from_millis(50)
            )
            .is_err());
        }
    }

    #[test]
    fn path_segments_are_percent_encoded() {
        assert_eq!(path_segment("run:1:2"), "run:1:2");
        assert_eq!(path_segment("a/b?c"), "a%2Fb%3Fc");
    }
}
