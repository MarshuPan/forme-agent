//! Finite managed tools/call exchange; encoding is never proof of dispatch.
use forme_protocol as p;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

pub(super) struct CallExchange {
    tool: String,
    arguments: Value,
    deadline: Instant,
    budget: u64,
    received: u64,
    phase: Phase,
    result: Option<CallResult>,
}
enum Phase {
    Initialize,
    AwaitInitialize,
    Initialized,
    Call,
    AwaitCall,
    Complete,
    Failed,
}
pub(super) struct CallFrame {
    pub(super) bytes: Vec<u8>,
    pub(super) tool_call: bool,
    pub(super) reply: bool,
}
pub(super) use super::controller::ExchangeResult as CallResult;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    jsonrpc: String,
    id: u64,
    #[serde(default, deserialize_with = "present")]
    result: Option<Value>,
    #[serde(default, deserialize_with = "present")]
    error: Option<RpcError>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcError {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl CallExchange {
    pub(super) fn new(
        tool: String,
        arguments: Value,
        deadline: Instant,
        budget: u64,
    ) -> p::Result<Self> {
        if tool.is_empty()
            || tool.len() > 2048
            || tool.chars().any(char::is_control)
            || !arguments.is_object()
            || Instant::now() >= deadline
            || !(1..=1_048_576).contains(&budget)
        {
            return Err(invalid());
        }
        Ok(Self {
            tool,
            arguments,
            deadline,
            budget,
            received: 0,
            phase: Phase::Initialize,
            result: None,
        })
    }
    pub(super) fn next_frame(&mut self) -> p::Result<Option<CallFrame>> {
        let phase = std::mem::replace(&mut self.phase, Phase::Failed);
        self.live()?;
        let (request, tool_call, reply, next) = match phase {
            Phase::Initialize => (
                json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                    "protocolVersion":"2025-06-18","capabilities":{},
                    "clientInfo":{"name":"forme","version":"0.0.1"}}}),
                false,
                true,
                Phase::AwaitInitialize,
            ),
            Phase::Initialized => (
                json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
                false,
                false,
                Phase::Call,
            ),
            Phase::Call => (
                json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
                    "params":{"name":self.tool,"arguments":self.arguments}}),
                true,
                true,
                Phase::AwaitCall,
            ),
            Phase::Complete => {
                self.phase = Phase::Complete;
                return Ok(None);
            }
            _ => return Err(invalid()),
        };
        let mut bytes = serde_json::to_vec(&request).map_err(|_| invalid())?;
        bytes.push(b'\n');
        if bytes.len() > 1_048_576 {
            return Err(invalid());
        }
        self.phase = next;
        Ok(Some(CallFrame {
            bytes,
            tool_call,
            reply,
        }))
    }
    pub(super) fn accept(&mut self, bytes: &[u8]) -> p::Result<()> {
        let phase = std::mem::replace(&mut self.phase, Phase::Failed);
        self.live()?;
        self.account(bytes.len())?;
        if bytes.last() != Some(&b'\n') || bytes[..bytes.len() - 1].contains(&b'\n') {
            return Err(invalid());
        }
        let reply: Reply = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        if reply.jsonrpc != "2.0" || reply.result.is_some() == reply.error.is_some() {
            return Err(invalid());
        }
        match phase {
            Phase::AwaitInitialize if reply.id == 1 => {
                let result = reply.result.ok_or_else(invalid)?;
                if result.get("protocolVersion").and_then(Value::as_str) != Some("2025-06-18")
                    || !result.get("serverInfo").is_some_and(Value::is_object)
                    || !result.get("capabilities").is_some_and(Value::is_object)
                    || !result["capabilities"]
                        .get("tools")
                        .is_some_and(Value::is_object)
                {
                    return Err(invalid());
                }
                self.phase = Phase::Initialized;
            }
            Phase::AwaitCall if reply.id == 2 => {
                let (result, outcome) = match (reply.result, reply.error) {
                    (Some(result), None) => {
                        if !result.is_object()
                            || !result.get("content").is_some_and(Value::is_array)
                            || result
                                .get("structuredContent")
                                .is_some_and(|value| !value.is_object())
                            || result.get("isError").is_some_and(|flag| !flag.is_boolean())
                        {
                            return Err(invalid());
                        }
                        let outcome =
                            if result.get("isError").and_then(Value::as_bool) == Some(true) {
                                p::WorkerOutcome::Failed
                            } else {
                                p::WorkerOutcome::Completed
                            };
                        (result, outcome)
                    }
                    (None, Some(error)) => {
                        if error.message.len() > 16_384 {
                            return Err(invalid());
                        }
                        // Preserve a bounded protocol error only as untrusted
                        // protected output, never as a Host diagnostic message.
                        (
                            json!({"error":{"code":error.code,"message":error.message,"data":error.data}}),
                            p::WorkerOutcome::Failed,
                        )
                    }
                    _ => return Err(invalid()),
                };
                let bytes = serde_json::to_vec(&result).map_err(|_| invalid())?;
                if bytes.len() as u64 > self.budget {
                    return Err(invalid());
                }
                self.result = Some(CallResult { bytes, outcome });
                self.phase = Phase::Complete;
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }
    pub(super) fn finish(mut self, stderr_bytes: usize) -> p::Result<CallResult> {
        self.live()?;
        self.account(stderr_bytes)?;
        if !matches!(self.phase, Phase::Complete) {
            return Err(invalid());
        }
        self.result.take().ok_or_else(invalid)
    }
    fn account(&mut self, bytes: usize) -> p::Result<()> {
        self.received = self
            .received
            .checked_add(bytes as u64)
            .ok_or_else(invalid)?;
        if self.received > self.budget {
            return Err(invalid());
        }
        Ok(())
    }
    fn live(&self) -> p::Result<()> {
        if Instant::now() >= self.deadline {
            return Err(invalid());
        }
        Ok(())
    }
}
fn invalid() -> p::Error {
    p::Error("managed MCP call exchange is unavailable or unconfirmed".into())
}

#[cfg(test)]
mod tests;
