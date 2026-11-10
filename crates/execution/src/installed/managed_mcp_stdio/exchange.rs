//! Bounded discovery exchange. This state machine never creates a process.
use forme_protocol as p;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

const VERSION: &str = "2025-06-18";
const MAX_PAGES: usize = 8;

pub(super) struct ManagedMcpDiscoveryExchange {
    spec: p::ManagedMcpDiscoverySpec,
    deadline: Instant,
    phase: Phase,
    next_id: u64,
    received: u64,
    sent: usize,
    has_tools: bool,
    has_resources: bool,
    tool_pages: usize,
    resource_pages: usize,
    tool_cursors: BTreeSet<String>,
    resource_cursors: BTreeSet<String>,
    names: BTreeSet<String>,
    uris: BTreeSet<String>,
    tools: Vec<p::ManagedMcpDiscoveryToolV1>,
    resources: Vec<p::ManagedMcpDiscoveryResourceV1>,
}
enum Phase {
    Initialize,
    Initialized,
    Tools(Option<String>),
    Resources(Option<String>),
    Awaiting(u64, ReplyKind),
    Complete,
    Failed,
}
enum ReplyKind {
    Initialize,
    Tools,
    Resources,
}

impl ManagedMcpDiscoveryExchange {
    /// The controller supplies the already bounded original deadline. Passing
    /// a deadline beyond the immutable spec is rejected, never shortened silently.
    pub(super) fn new(spec: p::ManagedMcpDiscoverySpec, deadline: Instant) -> p::Result<Self> {
        spec.validate()?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(invalid)?;
        if remaining.is_zero() || remaining > Duration::from_millis(spec.timeout.0) {
            return Err(invalid());
        }
        Ok(Self {
            spec,
            deadline,
            phase: Phase::Initialize,
            next_id: 1,
            received: 0,
            sent: 0,
            has_tools: false,
            has_resources: false,
            tool_pages: 0,
            resource_pages: 0,
            tool_cursors: BTreeSet::new(),
            resource_cursors: BTreeSet::new(),
            names: BTreeSet::new(),
            uris: BTreeSet::new(),
            tools: vec![],
            resources: vec![],
        })
    }

    /// The caller must recheck live authority at actual I/O egress. Encoding a
    /// frame is not proof of dispatch. A failed write discards this exchange.
    pub(super) fn next_frame(&mut self) -> p::Result<Option<Vec<u8>>> {
        let phase = std::mem::replace(&mut self.phase, Phase::Failed);
        self.live()?;
        let request = match phase {
            Phase::Initialize => self.request(
                "initialize",
                json!({"protocolVersion":VERSION,
                "capabilities":{},"clientInfo":{"name":"forme","version":"0.0.1"}}),
                ReplyKind::Initialize,
            )?,
            Phase::Initialized => {
                self.phase = if self.has_tools {
                    Phase::Tools(None)
                } else {
                    self.after_tools()
                };
                json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
            }
            Phase::Tools(cursor) => {
                self.tool_pages += 1;
                if self.tool_pages > MAX_PAGES {
                    return Err(invalid());
                }
                self.request("tools/list", cursor_params(cursor), ReplyKind::Tools)?
            }
            Phase::Resources(cursor) => {
                self.resource_pages += 1;
                if self.resource_pages > MAX_PAGES {
                    return Err(invalid());
                }
                self.request(
                    "resources/list",
                    cursor_params(cursor),
                    ReplyKind::Resources,
                )?
            }
            Phase::Complete => {
                self.phase = Phase::Complete;
                return Ok(None);
            }
            Phase::Awaiting(..) | Phase::Failed => return Err(invalid()),
        };
        let result = (|| {
            let mut bytes = serde_json::to_vec(&request).map_err(|_| invalid())?;
            bytes.push(b'\n');
            self.sent = self.sent.checked_add(bytes.len()).ok_or_else(invalid)?;
            if bytes.len() > 8192 || self.sent > 65_536 {
                return Err(invalid());
            }
            Ok(Some(bytes))
        })();
        if result.is_err() {
            self.phase = Phase::Failed;
        }
        result
    }

    pub(super) fn accept_frame(&mut self, frame: &[u8]) -> p::Result<()> {
        let phase = std::mem::replace(&mut self.phase, Phase::Failed);
        self.live()?;
        self.account(frame.len())?;
        let Phase::Awaiting(id, kind) = phase else {
            return Err(invalid());
        };
        if frame.last() != Some(&b'\n') || frame[..frame.len() - 1].contains(&b'\n') {
            return Err(invalid());
        }
        let reply: Reply = serde_json::from_slice(frame).map_err(|_| invalid())?;
        if reply.jsonrpc != "2.0" || reply.id != id {
            return Err(invalid());
        }
        match kind {
            ReplyKind::Initialize => {
                let result: Initialization =
                    serde_json::from_value(reply.result).map_err(|_| invalid())?;
                if result.protocol_version != VERSION || !result.server_info.is_object() {
                    return Err(invalid());
                }
                let capabilities = result.capabilities.as_object().ok_or_else(invalid)?;
                self.has_tools = capability(capabilities.get("tools"))?;
                self.has_resources = capability(capabilities.get("resources"))?;
                self.phase = Phase::Initialized;
            }
            ReplyKind::Tools => {
                let result: Tools = serde_json::from_value(reply.result).map_err(|_| invalid())?;
                if self.tools.len().saturating_add(result.tools.len()) > 128 {
                    return Err(invalid());
                }
                for tool in result.tools {
                    let tool = p::ManagedMcpDiscoveryToolV1 {
                        schema_version: p::SchemaVersion(1),
                        name: tool.name,
                        description: tool.description,
                        schema_digest: p::canonical_digest(&tool.input_schema)?,
                        input_schema: tool.input_schema,
                    };
                    tool.validate()?;
                    if !self.names.insert(tool.name.clone()) {
                        return Err(invalid());
                    }
                    self.tools.push(tool);
                }
                let cursor =
                    check_cursor(result.next_cursor, self.tool_pages, &mut self.tool_cursors)?;
                self.phase = match cursor {
                    Some(cursor) => Phase::Tools(Some(cursor)),
                    None => self.after_tools(),
                };
            }
            ReplyKind::Resources => {
                let result: Resources =
                    serde_json::from_value(reply.result).map_err(|_| invalid())?;
                if self.resources.len().saturating_add(result.resources.len()) > 128 {
                    return Err(invalid());
                }
                for resource in result.resources {
                    let resource = p::ManagedMcpDiscoveryResourceV1 {
                        schema_version: p::SchemaVersion(1),
                        uri: resource.uri,
                        name: resource.name,
                        description: resource.description,
                        mime_type: resource.mime_type,
                    };
                    resource.validate()?;
                    if !self.uris.insert(resource.uri.clone()) {
                        return Err(invalid());
                    }
                    self.resources.push(resource);
                }
                self.phase = match check_cursor(
                    result.next_cursor,
                    self.resource_pages,
                    &mut self.resource_cursors,
                )? {
                    Some(cursor) => Phase::Resources(Some(cursor)),
                    None => Phase::Complete,
                };
            }
        }
        Ok(())
    }

    pub(super) fn awaiting_reply(&self) -> bool {
        matches!(self.phase, Phase::Awaiting(..))
    }

    /// Diagnostic output consumes the same wire budget; its body is not kept.
    pub(super) fn account_stderr(&mut self, bytes: usize) -> p::Result<()> {
        if matches!(self.phase, Phase::Failed)
            || self.live().is_err()
            || self.account(bytes).is_err()
        {
            self.phase = Phase::Failed;
            return Err(invalid());
        }
        Ok(())
    }

    /// Only a complete exchange can produce metadata. Native tree/I/O termination
    /// and authenticated result sealing remain separate controller obligations.
    pub(super) fn finish(self) -> p::Result<p::ManagedMcpDiscoveryCatalogV1> {
        self.live()?;
        if !matches!(self.phase, Phase::Complete) {
            return Err(invalid());
        }
        let mut catalog = p::ManagedMcpDiscoveryCatalogV1 {
            schema_version: p::SchemaVersion(1),
            spec: self.spec,
            tools: self.tools,
            resources: self.resources,
            catalog_digest: p::sha256_content_digest(b""),
        };
        catalog.catalog_digest = catalog.computed_digest()?;
        catalog.validate_for(&catalog.spec)?;
        Ok(catalog)
    }
    fn request(&mut self, method: &str, params: Value, kind: ReplyKind) -> p::Result<Value> {
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or_else(invalid)?;
        self.phase = Phase::Awaiting(id, kind);
        Ok(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
    }
    fn after_tools(&self) -> Phase {
        if self.has_resources {
            Phase::Resources(None)
        } else {
            Phase::Complete
        }
    }
    fn live(&self) -> p::Result<()> {
        if Instant::now() >= self.deadline {
            return Err(invalid());
        }
        Ok(())
    }
    fn account(&mut self, bytes: usize) -> p::Result<()> {
        self.received = self
            .received
            .checked_add(bytes.try_into().map_err(|_| invalid())?)
            .ok_or_else(invalid)?;
        if self.received > self.spec.output_budget_bytes {
            return Err(invalid());
        }
        Ok(())
    }
}
fn cursor_params(cursor: Option<String>) -> Value {
    cursor.map_or_else(|| json!({}), |cursor| json!({"cursor":cursor}))
}
fn capability(value: Option<&Value>) -> p::Result<bool> {
    match value {
        None => Ok(false),
        Some(Value::Object(_)) => Ok(true),
        _ => Err(invalid()),
    }
}
fn check_cursor(
    cursor: Option<String>,
    pages: usize,
    seen: &mut BTreeSet<String>,
) -> p::Result<Option<String>> {
    if let Some(cursor) = &cursor {
        if pages >= MAX_PAGES
            || cursor.is_empty()
            || cursor.len() > 2048
            || cursor.chars().any(char::is_control)
            || !seen.insert(cursor.clone())
        {
            return Err(invalid());
        }
    }
    Ok(cursor)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    jsonrpc: String,
    id: u64,
    result: Value,
}
#[derive(Deserialize)]
struct Initialization {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
    capabilities: Value,
    #[serde(rename = "serverInfo")]
    server_info: Value,
}
#[derive(Deserialize)]
struct Tools {
    tools: Vec<Tool>,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}
#[derive(Deserialize)]
struct Tool {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}
#[derive(Deserialize)]
struct Resources {
    resources: Vec<Resource>,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}
#[derive(Deserialize)]
struct Resource {
    uri: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
}
fn invalid() -> p::Error {
    p::Error("managed MCP discovery exchange is invalid, expired or over budget".into())
}

#[cfg(test)]
mod tests;
