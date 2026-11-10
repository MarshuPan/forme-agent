mod io;

use std::process::{Child, Command, Stdio};
use std::sync::mpsc::RecvTimeoutError;
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
        let admission =
            io::Admission::acquire().map_err(|error| failed(plan, sink, error.detail))?;
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
        let mut rpc = match McpRpcSession::new(child, admission, effective_timeout, token.clone()) {
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
            if !result
                .get("content")
                .is_some_and(serde_json::Value::is_array)
                || result
                    .get("structuredContent")
                    .is_some_and(|value| !value.is_object())
            {
                return Err(McpExecutionError::schema(
                    "MCP tools/call content is invalid",
                ));
            }
            if result
                .get("isError")
                .is_some_and(|value| !value.is_boolean())
            {
                return Err(McpExecutionError::schema(
                    "MCP tools/call error flag must be boolean",
                ));
            }
            rpc.tool_outcome_observed = true;
            if result.get("isError").and_then(serde_json::Value::as_bool) == Some(true) {
                return Err(McpExecutionError::server(
                    "MCP tool reported an unsuccessful result",
                ));
            }
            Ok(result)
        })();

        if !rpc.io.shutdown() {
            return self.finish_error(
                plan,
                sink,
                server,
                tool,
                McpExecutionError::cleanup_unknown(),
            );
        }

        let result = result.map_err(|mut error| {
            error.unverified_outcome = rpc.io.tool_dispatch_started() && !rpc.tool_outcome_observed;
            error
        });
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
            McpExecutionErrorKind::Timeout => Some(McpCallError::Timeout),
            McpExecutionErrorKind::Schema => Some(McpCallError::Schema),
            McpExecutionErrorKind::Server | McpExecutionErrorKind::CleanupUnknown => {
                Some(McpCallError::Server)
            }
            McpExecutionErrorKind::Cancelled => None,
        };
        emit_call(
            sink,
            server,
            tool,
            call_error == Some(McpCallError::Timeout),
            call_error,
        )?;
        if error.unverified_outcome {
            sink.emit(p::EventPayload::ActionOutcomeUnknown(
                p::ActionOutcomeUnknownPayload {
                    intent_id: plan.intent.intent_id.clone(),
                    probe_hint: p::ProbeHintRef("review-original-mcp-process-and-io".into()),
                    remote_lease: None,
                },
            ))?;
            return Err(p::Error(error.detail));
        }
        if error.kind == McpExecutionErrorKind::Cancelled {
            return cancelled(plan, sink, &output);
        }
        Err(failed(plan, sink, error.detail))
    }
}

impl ActionBackend for McpBackend {
    fn kind(&self) -> BackendKind {
        p::BackendKind::Mcp
    }

    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        reject_discovery(intent)?;
        plan_with(intent, self.budget.clone(), self.timeout)
    }

    fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        token: CancelToken,
    ) -> p::Result<ActionResult> {
        reject_discovery(&plan.intent)?;
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

fn reject_discovery(intent: &p::ActionIntent) -> p::Result<()> {
    if matches!(intent.parameters, p::ActionParameters::McpDiscovery(_)) {
        return Err(p::Error(
            "managed MCP discovery requires an isolated worker".into(),
        ));
    }
    Ok(())
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
    CleanupUnknown,
}

struct McpExecutionError {
    kind: McpExecutionErrorKind,
    detail: String,
    unverified_outcome: bool,
}

impl McpExecutionError {
    fn cleanup_unknown() -> Self {
        Self {
            kind: McpExecutionErrorKind::CleanupUnknown,
            detail: "MCP original process or I/O cleanup is unverified".into(),
            unverified_outcome: true,
        }
    }
    fn timeout(detail: impl Into<String>) -> Self {
        Self {
            kind: McpExecutionErrorKind::Timeout,
            detail: detail.into(),
            unverified_outcome: false,
        }
    }

    fn schema(detail: impl Into<String>) -> Self {
        Self {
            kind: McpExecutionErrorKind::Schema,
            detail: detail.into(),
            unverified_outcome: false,
        }
    }

    fn server(detail: impl Into<String>) -> Self {
        Self {
            kind: McpExecutionErrorKind::Server,
            detail: detail.into(),
            unverified_outcome: false,
        }
    }

    fn cancelled() -> Self {
        Self {
            kind: McpExecutionErrorKind::Cancelled,
            detail: "MCP action was cancelled".into(),
            unverified_outcome: false,
        }
    }
}

struct McpRpcSession {
    io: io::ProcessIo,
    deadline: Instant,
    token: CancelToken,
    next_id: u64,
    tool_outcome_observed: bool,
}

impl McpRpcSession {
    fn new(
        child: Child,
        admission: io::Admission,
        timeout: p::DurationMs,
        token: CancelToken,
    ) -> Result<Self, McpExecutionError> {
        let deadline = Instant::now() + Duration::from_millis(timeout.0);
        Ok(Self {
            io: io::ProcessIo::start(child, admission)?,
            deadline,
            token,
            next_id: 0,
            tool_outcome_observed: false,
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
        self.receive(id, method == "tools/call")
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), McpExecutionError> {
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn send(&mut self, value: serde_json::Value) -> Result<(), McpExecutionError> {
        self.check_live()?;
        let receipt = self.io.send(&value, self.deadline, &self.token)?;
        loop {
            self.check_live()?;
            let next = receipt.recv_timeout(io::POLL);
            self.check_live()?;
            match next {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => {
                    return Err(McpExecutionError::server(
                        "MCP request could not be written",
                    ));
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn check_live(&self) -> Result<(), McpExecutionError> {
        if self.token.is_cancelled() {
            Err(McpExecutionError::cancelled())
        } else if Instant::now() >= self.deadline {
            Err(McpExecutionError::timeout("MCP call deadline elapsed"))
        } else {
            Ok(())
        }
    }

    fn receive(
        &mut self,
        id: u64,
        tool_call: bool,
    ) -> Result<serde_json::Value, McpExecutionError> {
        loop {
            if self.token.is_cancelled() {
                return Err(McpExecutionError::cancelled());
            }
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(McpExecutionError::timeout("MCP call deadline elapsed"));
            }
            let wait = remaining.min(Duration::from_millis(5));
            let next = self.io.output().recv_timeout(wait);
            self.check_live()?;
            let line = match next {
                Ok(io::ReadEvent::Frame(line)) => line,
                Ok(io::ReadEvent::Oversized) => {
                    return Err(McpExecutionError::schema(
                        "MCP response exceeds its wire frame bound",
                    ));
                }
                Ok(io::ReadEvent::Failed) => {
                    return Err(McpExecutionError::server("MCP response could not be read"));
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(McpExecutionError::server(
                        "MCP process ended before responding",
                    ));
                }
            };
            let Ok(response) = serde_json::from_slice::<serde_json::Value>(&line) else {
                continue;
            };
            self.check_live()?;
            if response.get("id").and_then(serde_json::Value::as_u64) != Some(id) {
                continue;
            }
            if response.get("jsonrpc").and_then(serde_json::Value::as_str) != Some("2.0")
                || response.get("result").is_some() == response.get("error").is_some()
            {
                return Err(McpExecutionError::schema(
                    "MCP response envelope is invalid",
                ));
            }
            if let Some(error) = response.get("error") {
                if error
                    .get("code")
                    .and_then(serde_json::Value::as_i64)
                    .is_none()
                    || error
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .is_none()
                {
                    return Err(McpExecutionError::schema("MCP error response is invalid"));
                }
                self.tool_outcome_observed |= tool_call;
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
