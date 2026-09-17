use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use forme_protocol as p;

use crate::support::{
    cancelled, complete, emit_started, failed, ActiveCancellations, BoundedOutput,
};
use crate::{
    planner::plan_with, planner::validate_plan_for, ActionBackend, ActionResult, BackendKind,
    CancelToken, EventSink, ExecutionPlan, OutputBudget,
};

pub struct McpBackend {
    budget: OutputBudget,
    timeout: p::DurationMs,
    active: ActiveCancellations,
}

impl McpBackend {
    pub fn new(budget: OutputBudget, timeout: p::DurationMs) -> p::Result<Self> {
        if budget.max_bytes == 0 || timeout.0 == 0 {
            return Err(p::Error("MCP backend configuration is incomplete".into()));
        }
        Ok(Self {
            budget,
            timeout,
            active: ActiveCancellations::default(),
        })
    }

    fn execute_inner(
        &self,
        plan: &ExecutionPlan,
        sink: &EventSink,
        token: &CancelToken,
    ) -> p::Result<ActionResult> {
        let p::ActionParameters::Mcp {
            server,
            tool,
            arguments,
            schema_digest,
            transport,
            stdio,
            timeout,
        } = &plan.intent.parameters
        else {
            return Err(p::Error("MCP backend received non-MCP parameters".into()));
        };
        if *transport != p::McpTransport::Stdio
            || schema_digest.is_none()
            || stdio.command.trim().is_empty()
            || timeout.0 == 0
        {
            return Err(p::Error("MCP stdio action is incomplete".into()));
        }

        emit_started(plan, sink)?;
        let effective_timeout = p::DurationMs(plan.timeout.0.min(timeout.0).min(self.timeout.0));
        let mut command = Command::new(&stdio.command);
        command
            .args(&stdio.args)
            .env_clear()
            .envs(stdio.env.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        validate_plan_for(plan, p::BackendKind::Mcp)?;
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                emit_call(sink, server, tool, false, Some(McpCallError::Server))?;
                return Err(failed(
                    plan,
                    sink,
                    format!("failed to start MCP action: {error}"),
                ));
            }
        };
        let mut rpc = match McpRpcSession::new(child, effective_timeout, token.clone()) {
            Ok(rpc) => rpc,
            Err(error) => return self.finish_error(plan, sink, server, tool, error),
        };

        let result = (|| {
            let initialize = rpc.request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "forme", "version": "0.0.1" }
                }),
            )?;
            if !initialize.is_object() {
                return Err(McpExecutionError::schema(
                    "MCP initialize result must be an object",
                ));
            }
            rpc.notify("notifications/initialized", serde_json::json!({}))?;
            let result = rpc.request(
                "tools/call",
                serde_json::json!({
                    "name": tool.0,
                    "arguments": arguments,
                }),
            )?;
            if !result.is_object() {
                return Err(McpExecutionError::schema(
                    "MCP tools/call result must be an object",
                ));
            }
            if result.get("isError").and_then(serde_json::Value::as_bool) == Some(true) {
                return Err(McpExecutionError::server(
                    "MCP tool reported an unsuccessful result",
                ));
            }
            Ok(result)
        })();

        match result {
            Ok(result) => {
                emit_call(sink, server, tool, false, None)?;
                let encoded = serde_json::to_vec(&result).map_err(|error| {
                    failed(
                        plan,
                        sink,
                        format!("failed to normalize MCP output: {error}"),
                    )
                })?;
                let mut output = BoundedOutput::new(plan.budget.clone());
                output
                    .push(&encoded, plan, sink)
                    .map_err(|error| failed(plan, sink, error.to_string()))?;
                complete(plan, sink, &output, None, None)
            }
            Err(error) => self.finish_error(plan, sink, server, tool, error),
        }
    }

    fn finish_error(
        &self,
        plan: &ExecutionPlan,
        sink: &EventSink,
        server: &p::McpServerRef,
        tool: &p::ToolRef,
        error: McpExecutionError,
    ) -> p::Result<ActionResult> {
        let output = BoundedOutput::new(plan.budget.clone());
        let call_error = match error.kind {
            McpExecutionErrorKind::Timeout => McpCallError::Timeout,
            McpExecutionErrorKind::Schema => McpCallError::Schema,
            McpExecutionErrorKind::Server => McpCallError::Server,
            McpExecutionErrorKind::Cancelled => {
                emit_call(sink, server, tool, false, None)?;
                return cancelled(plan, sink, &output);
            }
        };
        emit_call(
            sink,
            server,
            tool,
            call_error == McpCallError::Timeout,
            Some(call_error),
        )?;
        Err(failed(plan, sink, error.detail))
    }
}

impl ActionBackend for McpBackend {
    fn kind(&self) -> BackendKind {
        p::BackendKind::Mcp
    }

    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        plan_with(intent, self.budget.clone(), self.timeout)
    }

    fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        token: CancelToken,
    ) -> p::Result<ActionResult> {
        validate_plan_for(&plan, p::BackendKind::Mcp)?;
        let output = BoundedOutput::new(plan.budget.clone());
        if token.is_cancelled() {
            return cancelled(&plan, sink, &output);
        }
        self.active
            .register(plan.intent.intent_id.clone(), token.clone())?;
        let result = self.execute_inner(&plan, sink, &token);
        self.active.remove(&plan.intent.intent_id);
        result
    }

    fn cancel(&self, action: p::ActionId) -> p::Result<()> {
        self.active.cancel(&action)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpCallError {
    Timeout,
    Schema,
    Server,
}

impl McpCallError {
    fn protocol(self) -> p::McpErrorClass {
        let value = match self {
            Self::Timeout => "timeout",
            Self::Schema => "schema_mismatch",
            Self::Server => "server_error",
        };
        p::McpErrorClass(value.into())
    }
}

fn emit_call(
    sink: &EventSink,
    server: &p::McpServerRef,
    tool: &p::ToolRef,
    timeout: bool,
    error: Option<McpCallError>,
) -> p::Result<()> {
    sink.emit(p::EventPayload::McpCallEvent(p::McpCallEventPayload {
        server: server.clone(),
        tool: tool.clone(),
        timeout,
        error_class: error.map(McpCallError::protocol),
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpExecutionErrorKind {
    Timeout,
    Schema,
    Server,
    Cancelled,
}

struct McpExecutionError {
    kind: McpExecutionErrorKind,
    detail: String,
}

impl McpExecutionError {
    fn timeout(detail: impl Into<String>) -> Self {
        Self {
            kind: McpExecutionErrorKind::Timeout,
            detail: detail.into(),
        }
    }

    fn schema(detail: impl Into<String>) -> Self {
        Self {
            kind: McpExecutionErrorKind::Schema,
            detail: detail.into(),
        }
    }

    fn server(detail: impl Into<String>) -> Self {
        Self {
            kind: McpExecutionErrorKind::Server,
            detail: detail.into(),
        }
    }

    fn cancelled() -> Self {
        Self {
            kind: McpExecutionErrorKind::Cancelled,
            detail: "MCP action was cancelled".into(),
        }
    }
}

struct McpRpcSession {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    deadline: Instant,
    token: CancelToken,
    next_id: u64,
}

impl McpRpcSession {
    fn new(
        mut child: Child,
        timeout: p::DurationMs,
        token: CancelToken,
    ) -> Result<Self, McpExecutionError> {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpExecutionError::server("MCP process did not expose stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpExecutionError::server("MCP process did not expose stdout"))?;
        let (sender, lines) = mpsc::sync_channel(64);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            lines,
            deadline: Instant::now() + Duration::from_millis(timeout.0),
            token,
            next_id: 0,
        })
    }

    fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpExecutionError> {
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        self.receive(id)
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), McpExecutionError> {
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn send(&mut self, value: serde_json::Value) -> Result<(), McpExecutionError> {
        if self.token.is_cancelled() {
            return Err(McpExecutionError::cancelled());
        }
        serde_json::to_writer(&mut self.stdin, &value).map_err(|error| {
            McpExecutionError::server(format!("failed to encode MCP request: {error}"))
        })?;
        self.stdin.write_all(b"\n").map_err(|error| {
            McpExecutionError::server(format!("failed to write MCP request: {error}"))
        })?;
        self.stdin.flush().map_err(|error| {
            McpExecutionError::server(format!("failed to flush MCP request: {error}"))
        })
    }

    fn receive(&self, id: u64) -> Result<serde_json::Value, McpExecutionError> {
        loop {
            if self.token.is_cancelled() {
                return Err(McpExecutionError::cancelled());
            }
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(McpExecutionError::timeout("MCP call deadline elapsed"));
            }
            let wait = remaining.min(Duration::from_millis(5));
            let line = match self.lines.recv_timeout(wait) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(McpExecutionError::server(
                        "MCP process ended before responding",
                    ));
                }
            };
            let Ok(response) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if response.get("id").and_then(serde_json::Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(McpExecutionError::server(format!(
                    "MCP server returned an error: {error}"
                )));
            }
            return response
                .get("result")
                .cloned()
                .ok_or_else(|| McpExecutionError::schema("MCP response has no result"));
        }
    }
}

impl Drop for McpRpcSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
