//! HTTP model transport and its explicit proxy binding.
#![forbid(unsafe_code)]

use std::io::Write;
use std::process::{Command, Stdio};

use forme_protocol as p;
use serde_json::{json, Value};
use url::Url as ParsedUrl;

use super::{
    ModelOutput, ModelProfile, ModelProvider, ModelRequest, ModelResponse, ModelToolCall,
    ModelToolExchange, SecretString,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelHttpProxy(String);

impl ModelHttpProxy {
    pub fn parse(value: impl Into<String>) -> p::Result<Self> {
        let value = value.into();
        if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
            return Err(p::Error("model HTTP proxy endpoint is invalid".into()));
        }
        let url = ParsedUrl::parse(&value)
            .map_err(|_| p::Error("model HTTP proxy endpoint is invalid".into()))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || !matches!(url.path(), "" | "/")
            || url.query().is_some()
            || url.fragment().is_some()
            || url.cannot_be_a_base()
        {
            return Err(p::Error("model HTTP proxy endpoint is invalid".into()));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct ChatCompletionsProvider {
    profile: ModelProfile,
    credential: SecretString,
    proxy: Option<ModelHttpProxy>,
    pub(crate) executable: String,
    timeout: p::DurationMs,
}

impl ChatCompletionsProvider {
    pub fn new(
        profile: ModelProfile,
        credential: SecretString,
        timeout: p::DurationMs,
    ) -> p::Result<Self> {
        Self::new_with_proxy(profile, credential, timeout, None)
    }

    pub fn new_with_proxy(
        profile: ModelProfile,
        credential: SecretString,
        timeout: p::DurationMs,
        proxy: Option<ModelHttpProxy>,
    ) -> p::Result<Self> {
        profile.validate()?;
        if timeout.0 == 0 {
            return Err(p::Error("model timeout must be non-zero".into()));
        }
        Ok(Self {
            profile,
            credential,
            proxy,
            executable: "curl".into(),
            timeout,
        })
    }

    pub fn with_executable(mut self, executable: impl Into<String>) -> p::Result<Self> {
        let executable = executable.into();
        if executable.trim().is_empty() {
            return Err(p::Error("model HTTP executable cannot be empty".into()));
        }
        self.executable = executable;
        Ok(self)
    }

    pub(crate) fn request_json(&self, request: &ModelRequest) -> Value {
        let messages = request
            .messages
            .iter()
            .map(|message| match &message.tool_exchange {
                Some(ModelToolExchange::Call {
                    call_id,
                    name,
                    arguments,
                    ..
                }) => json!({
                    "role": "assistant", "content": null,
                    "tool_calls": [{"id": call_id, "type": "function", "function": {
                        "name": name, "arguments": arguments.to_string()
                    }}]
                }),
                Some(ModelToolExchange::Result { call_id, .. }) => json!({
                    "role": "tool", "tool_call_id": call_id,
                    "content": message.transport_content()
                }),
                None => json!({
                    "role": message.role.as_str(),
                    "content": message.transport_content(),
                }),
            })
            .collect::<Vec<_>>();
        let tools = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect::<Vec<_>>();
        let mut body = json!({
            "model": self.profile.model,
            "messages": messages,
            "stream": false,
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        if let Some(limit) = request.max_output_tokens {
            body["max_tokens"] = json!(limit);
        }
        body
    }

    fn call_http(&self, body: &Value) -> p::Result<Value> {
        let encoded = serde_json::to_string(body)
            .map_err(|error| p::Error(format!("failed to encode model request: {error}")))?;
        let config = self.curl_config(&encoded)?;
        let mut child = Command::new(&self.executable)
            .args(["--config", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| p::Error("retryable: model HTTP transport could not start".into()))?;
        child
            .stdin
            .take()
            .ok_or_else(|| p::Error("model HTTP transport has no input pipe".into()))?
            .write_all(config.as_bytes())
            .map_err(|_| {
                p::Error("unknown: model HTTP transport input outcome is unconfirmed".into())
            })?;
        let output = child
            .wait_with_output()
            .map_err(|_| p::Error("unknown: model HTTP transport outcome is unconfirmed".into()))?;
        let (body, status) = split_http_status(&output.stdout);
        if !output.status.success() {
            // curl stderr may echo private endpoints. Only fixed categories
            // and numeric codes may cross into event/trace failure evidence.
            let code = output.status.code().unwrap_or(-1);
            let retryable =
                matches!(code, 5..=7) || (code == 22 && matches!(status, Some(429 | 502 | 503)));
            return Err(p::Error(format!(
                "{}: model HTTP attempt failed (transport_code={code}, http_status={})",
                if retryable { "retryable" } else { "unknown" },
                status.unwrap_or(0)
            )));
        }
        serde_json::from_slice(body)
            .map_err(|error| p::Error(format!("model endpoint returned invalid JSON: {error}")))
    }

    pub(crate) fn curl_config(&self, body: &str) -> p::Result<String> {
        let endpoint = quote_curl_config(&self.profile.base_url.endpoint())?;
        let credential = quote_curl_config(self.credential.expose())?;
        let body = quote_curl_config(body)?;
        let timeout_seconds = self.timeout.0.div_ceil(1000).max(1);
        let proxy = self
            .proxy
            .as_ref()
            .map(|proxy| quote_curl_config(proxy.as_str()))
            .transpose()?;
        let proxy_directives = match proxy {
            Some(proxy) => format!("proxy = \"{proxy}\"\nnoproxy = \"\"\n"),
            None => "proxy = \"\"\nnoproxy = \"*\"\n".to_owned(),
        };
        Ok(format!(
            "silent\nshow-error\nfail-with-body\nrequest = \"POST\"\nmax-time = \"{timeout_seconds}\"\n{proxy_directives}url = \"{endpoint}\"\nheader = \"Content-Type: application/json\"\nheader = \"Authorization: Bearer {credential}\"\ndata = \"{body}\"\nwrite-out = \"\\nFORME_HTTP_STATUS:%{{http_code}}\"\n"
        ))
    }
}

pub(crate) fn split_http_status(bytes: &[u8]) -> (&[u8], Option<u16>) {
    const MARKER: &[u8] = b"\nFORME_HTTP_STATUS:";
    if let Some(at) = bytes.windows(MARKER.len()).rposition(|s| s == MARKER) {
        let status = std::str::from_utf8(&bytes[at + MARKER.len()..])
            .ok()
            .and_then(|s| s.trim().parse().ok());
        (&bytes[..at], status)
    } else {
        (bytes, None)
    }
}

impl p::ExternalProvider for ChatCompletionsProvider {
    fn kind(&self) -> p::ProviderKind {
        p::ProviderKind::Model
    }

    fn id(&self) -> p::ProviderId {
        self.profile.provider.clone()
    }

    fn declared_capabilities(&self) -> p::CapabilitySet {
        p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef(format!(
                "model:{}:{}",
                self.profile.provider.0, self.profile.model
            ))],
            permissions: Vec::new(),
        }
    }

    fn trust_default(&self) -> p::TrustTier {
        p::TrustTier::Untrusted
    }
}

impl p::ModelProvider for ChatCompletionsProvider {}

impl ModelProvider for ChatCompletionsProvider {
    fn call(&self, request: ModelRequest) -> p::Result<ModelResponse> {
        request.validate()?;
        parse_chat_response(self.call_http(&self.request_json(&request))?)
    }

    fn profile(&self) -> ModelProfile {
        self.profile.clone()
    }
}

fn quote_curl_config(value: &str) -> p::Result<String> {
    if value.contains('\0') {
        return Err(p::Error(
            "model transport value contains a null byte".into(),
        ));
    }
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\r' => escaped.push_str("\\r"),
            '\n' => escaped.push_str("\\n"),
            other => escaped.push(other),
        }
    }
    Ok(escaped)
}

pub(crate) fn parse_chat_response(body: Value) -> p::Result<ModelResponse> {
    if body.get("error").is_some_and(|error| !error.is_null()) {
        // Provider-controlled text can echo credentials or impersonate our
        // retry classification. It never becomes a runtime diagnostic.
        return Err(p::Error(
            "unknown: model endpoint rejected the request".into(),
        ));
    }
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| p::Error("model response has no choices".into()))?;
    let message = choice
        .get("message")
        .ok_or_else(|| p::Error("model response has no message".into()))?;
    let calls = message.get("tool_calls").and_then(Value::as_array);
    if calls.is_some_and(|calls| calls.len() > 1) {
        return Err(p::Error(
            "parallel model tool proposals are unsupported".into(),
        ));
    }
    let output = if let Some(tool_call) = calls.and_then(|calls| calls.first()) {
        let function = tool_call
            .get("function")
            .ok_or_else(|| p::Error("model tool call has no function".into()))?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| p::Error("model tool call has no name".into()))?;
        let arguments = function
            .get("arguments")
            .and_then(Value::as_str)
            .map(|value| serde_json::from_str(value).unwrap_or_else(|_| json!({ "raw": value })))
            .unwrap_or_else(|| json!({}));
        ModelOutput::Tool(Box::new(ModelToolCall {
            schema_version: p::SchemaVersion(1),
            call_id: p::ToolCallId(
                tool_call
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty())
                    .ok_or_else(|| p::Error("model tool call has no correlation id".into()))?
                    .to_owned(),
            ),
            tool: p::ToolRef(name.to_owned()),
            arguments,
            intent: None,
        }))
    } else {
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| p::Error("model response has no text content".into()))?;
        ModelOutput::Final(content.to_owned())
    };
    let usage = body.get("usage");
    Ok(ModelResponse {
        schema_version: p::SchemaVersion(1),
        output,
        usage: p::ModelUsage {
            input_tokens: usage
                .and_then(|value| value.get("prompt_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output_tokens: usage
                .and_then(|value| value.get("completion_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
        },
        finish_reason: p::FinishReason(
            choice
                .get("finish_reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
        ),
    })
}
