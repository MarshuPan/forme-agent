use std::collections::VecDeque;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use forme_protocol as p;
use sha2::{Digest, Sha256};
use url::Url;

use crate::support::{
    cancelled, complete_external, emit_started, fail_unless_terminal, failed, outcome_unknown,
    ActiveCancellations, BoundedOutput,
};
use crate::{
    plan_with, resolve_external_input, validate_plan_for, ActionBackend, ActionResult, BackendKind,
    CancelToken, ContentResolver, DriverReceipt, EventSink, ExecutionPlan, OutputBudget,
    SecretResolver,
};

pub enum AppApiDriverOutcome {
    Completed(DriverReceipt),
    Unknown(p::ProbeHintRef),
}

pub trait AppApiDriver: Send + Sync {
    fn perform(
        &self,
        action: &p::AppApiActionSpec,
        secrets: &dyn SecretResolver,
        contents: &dyn ContentResolver,
    ) -> p::Result<AppApiDriverOutcome>;
}

pub struct HttpAppApiDriver {
    connector: p::ProviderId,
    base_url: Url,
    schema_digest: p::SchemaDigest,
    credential_ref: Option<p::SecretRef>,
    allowed_mutations: Vec<p::AppApiMutationMethod>,
    requests_per_minute: u32,
    max_timeout: p::DurationMs,
    max_response_bytes: u64,
    revoked: Arc<AtomicBool>,
    rate_window: Mutex<VecDeque<Instant>>,
}

impl HttpAppApiDriver {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        connector: p::ProviderId,
        base_url: String,
        schema_digest: p::SchemaDigest,
        credential_ref: Option<p::SecretRef>,
        allowed_mutations: Vec<p::AppApiMutationMethod>,
        requests_per_minute: u32,
        max_timeout: p::DurationMs,
        max_response_bytes: u64,
        revoked: Arc<AtomicBool>,
    ) -> p::Result<Self> {
        let base_url = Url::parse(&base_url)
            .map_err(|_| p::Error("App API base URL cannot be parsed".into()))?;
        if connector.0.trim().is_empty()
            || schema_digest.0.trim().is_empty()
            || requests_per_minute == 0
            || max_timeout.0 == 0
            || max_response_bytes == 0
            || max_response_bytes > 1_048_576
            || !valid_base_url(&base_url)
            || credential_ref
                .as_ref()
                .is_some_and(|reference| reference.0.trim().is_empty())
        {
            return Err(p::Error(
                "App API driver configuration is incomplete".into(),
            ));
        }
        Ok(Self {
            connector,
            base_url,
            schema_digest,
            credential_ref,
            allowed_mutations,
            requests_per_minute,
            max_timeout,
            max_response_bytes,
            revoked,
            rate_window: Mutex::new(VecDeque::new()),
        })
    }

    fn validate_action(&self, action: &p::AppApiActionSpec) -> p::Result<()> {
        if self.revoked.load(Ordering::SeqCst)
            || action.schema_version.0 == 0
            || action.connector != self.connector
            || action.schema_digest != self.schema_digest
            || action.credential != self.credential_ref
            || action.timeout.0 == 0
            || action.timeout.0 > self.max_timeout.0
            || action
                .participant
                .as_ref()
                .is_some_and(|participant| !safe_header_component(&participant.0))
        {
            return Err(p::Error(
                "App API connector was revoked or its plan binding changed".into(),
            ));
        }
        let endpoint = Url::parse(&action.endpoint)
            .map_err(|_| p::Error("App API endpoint cannot be parsed".into()))?;
        if !endpoint_within(&self.base_url, &endpoint) {
            return Err(p::Error(
                "App API endpoint is outside the configured connector".into(),
            ));
        }
        match &action.operation {
            p::AppApiOperation::Read => Ok(()),
            p::AppApiOperation::Mutation {
                method,
                body,
                idempotency_key,
            } => {
                if !self.allowed_mutations.contains(method)
                    || !safe_header_component(&idempotency_key.0)
                    || (*method == p::AppApiMutationMethod::Delete && body.is_some())
                {
                    return Err(p::Error("App API mutation is outside its allowlist".into()));
                }
                Ok(())
            }
        }
    }

    fn claim_rate_slot(&self) -> p::Result<()> {
        let now = Instant::now();
        let mut window = self
            .rate_window
            .lock()
            .map_err(|_| p::Error("App API rate state is unavailable".into()))?;
        while window
            .front()
            .is_some_and(|started| now.duration_since(*started) >= Duration::from_secs(60))
        {
            window.pop_front();
        }
        if window.len() >= self.requests_per_minute as usize {
            return Err(p::Error("App API connector rate limit is exhausted".into()));
        }
        window.push_back(now);
        Ok(())
    }
}

impl AppApiDriver for HttpAppApiDriver {
    fn perform(
        &self,
        action: &p::AppApiActionSpec,
        secrets: &dyn SecretResolver,
        contents: &dyn ContentResolver,
    ) -> p::Result<AppApiDriverOutcome> {
        self.validate_action(action)?;
        self.claim_rate_slot()?;
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_millis(action.timeout.0)))
            .max_redirects(0)
            .build();
        let agent: ureq::Agent = config.into();
        let credential = action
            .credential
            .as_ref()
            .map(|reference| secrets.resolve(reference))
            .transpose()?;
        if credential
            .as_ref()
            .is_some_and(|secret| !safe_bearer_value(secret.expose()))
        {
            return Err(p::Error(
                "resolved App API credential is not safe for an authorization header".into(),
            ));
        }
        let authorization = credential
            .as_ref()
            .map(|secret| format!("Bearer {}", secret.expose()));
        let representation = action.representation.map(|value| match value {
            p::Representation::Agent => "agent",
            p::Representation::AgentRepresentingOwner => "agent-representing-owner",
        });
        let participant = action.participant.as_ref().map(|value| value.0.as_str());

        let response = match &action.operation {
            p::AppApiOperation::Read => {
                let mut request = agent.get(&action.endpoint);
                if let Some(value) = authorization.as_deref() {
                    request = request.header("authorization", value);
                }
                request.call()
            }
            p::AppApiOperation::Mutation {
                method,
                body,
                idempotency_key,
            } => {
                let body = body
                    .as_ref()
                    .map(|input| resolve_external_input(input, secrets, contents))
                    .transpose()?
                    .unwrap_or_default();
                let send = |request: ureq::RequestBuilder<ureq::typestate::WithBody>| {
                    let mut request = request
                        .header("content-type", "application/octet-stream")
                        .header("idempotency-key", &idempotency_key.0);
                    if let Some(value) = authorization.as_deref() {
                        request = request.header("authorization", value);
                    }
                    if let Some(value) = representation {
                        request = request.header("x-forme-representation", value);
                    }
                    if let Some(value) = participant {
                        request = request.header("x-forme-participant", value);
                    }
                    request.send(body.as_bytes())
                };
                match method {
                    p::AppApiMutationMethod::Post => send(agent.post(&action.endpoint)),
                    p::AppApiMutationMethod::Put => send(agent.put(&action.endpoint)),
                    p::AppApiMutationMethod::Patch => send(agent.patch(&action.endpoint)),
                    p::AppApiMutationMethod::Delete => {
                        let mut request = agent
                            .delete(&action.endpoint)
                            .header("idempotency-key", &idempotency_key.0);
                        if let Some(value) = authorization.as_deref() {
                            request = request.header("authorization", value);
                        }
                        if let Some(value) = representation {
                            request = request.header("x-forme-representation", value);
                        }
                        if let Some(value) = participant {
                            request = request.header("x-forme-participant", value);
                        }
                        request.call()
                    }
                }
            }
        };
        let mutation = matches!(&action.operation, p::AppApiOperation::Mutation { .. });
        let mut response = match response {
            Ok(response) => response,
            Err(_) if mutation => {
                return Ok(AppApiDriverOutcome::Unknown(p::ProbeHintRef(
                    "query the connector by the bound idempotency key before any retry".into(),
                )));
            }
            Err(error) => {
                return Err(p::Error(format!(
                    "App API read transport failed ({})",
                    transport_error_category(&error)
                )))
            }
        };
        if !response.status().is_success() {
            return if mutation {
                Ok(AppApiDriverOutcome::Unknown(p::ProbeHintRef(
                    "query the connector by the bound idempotency key before any retry".into(),
                )))
            } else {
                Err(p::Error(
                    "App API read returned a non-success status".into(),
                ))
            };
        }
        let response_text = read_bounded_response(&mut response, self.max_response_bytes);
        let content_digest = response_text
            .as_ref()
            .ok()
            .filter(|body| !body.is_empty())
            .map(|body| sha256_digest(body.as_bytes()));
        let summary = match response_text {
            Ok(summary) if !summary.is_empty() => summary,
            Ok(_) if mutation => "App API mutation returned a successful empty response".into(),
            Ok(_) => return Err(p::Error("App API read returned an empty response".into())),
            Err(_) if mutation => {
                return Ok(AppApiDriverOutcome::Unknown(p::ProbeHintRef(
                    "query the connector by the bound idempotency key before any retry".into(),
                )))
            }
            Err(error) => return Err(error),
        };
        let effect = match action.operation {
            p::AppApiOperation::Read => p::EffectStatus::Observed,
            p::AppApiOperation::Mutation { .. } => p::EffectStatus::Committed,
        };
        Ok(AppApiDriverOutcome::Completed(DriverReceipt {
            schema_version: p::SchemaVersion(1),
            summary,
            content_ref: content_digest
                .as_ref()
                .map(|digest| p::ContentRef(format!("external:{}", digest.0))),
            digest: content_digest,
            effect,
        }))
    }
}

fn sha256_digest(bytes: &[u8]) -> p::SchemaDigest {
    let digest = Sha256::digest(bytes);
    p::SchemaDigest(format!("sha256:{digest:x}"))
}

fn transport_error_category(error: &ureq::Error) -> &'static str {
    match error {
        ureq::Error::Io(error) => match error.kind() {
            std::io::ErrorKind::ConnectionRefused => "connection_refused",
            std::io::ErrorKind::ConnectionReset => "connection_reset",
            std::io::ErrorKind::ConnectionAborted => "connection_aborted",
            std::io::ErrorKind::TimedOut => "io_timeout",
            std::io::ErrorKind::UnexpectedEof => "unexpected_eof",
            _ => "io",
        },
        ureq::Error::Timeout(_) => "timeout",
        ureq::Error::Protocol(_) => "protocol",
        ureq::Error::TooManyRedirects | ureq::Error::RedirectFailed => "redirect",
        ureq::Error::StatusCode(_) => "status",
        _ => "transport",
    }
}

fn read_bounded_response(
    response: &mut ureq::http::Response<ureq::Body>,
    max_bytes: u64,
) -> p::Result<String> {
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| p::Error("App API response body could not be read".into()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(p::Error("App API response exceeded its byte limit".into()));
    }
    String::from_utf8(bytes).map_err(|_| p::Error("App API response is not UTF-8 text".into()))
}

fn valid_base_url(url: &Url) -> bool {
    safe_app_api_url(url)
        && url.path().ends_with('/')
        && url.query().is_none()
        && url.fragment().is_none()
}

fn endpoint_within(base: &Url, endpoint: &Url) -> bool {
    safe_app_api_url(endpoint)
        && endpoint.origin() == base.origin()
        && endpoint.path().starts_with(base.path())
        && endpoint.query().is_none()
        && endpoint.fragment().is_none()
}

fn safe_app_api_url(url: &Url) -> bool {
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    (url.scheme() == "https" || (url.scheme() == "http" && loopback))
        && url.host_str().is_some()
        && url.port_or_known_default().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

fn safe_header_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
}

fn safe_bearer_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 8_192
        && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

pub struct AppApiBackend {
    connector: p::ProviderId,
    driver: Arc<dyn AppApiDriver>,
    secrets: Arc<dyn SecretResolver>,
    contents: Arc<dyn ContentResolver>,
    budget: OutputBudget,
    max_timeout: p::DurationMs,
    active: ActiveCancellations,
}

impl AppApiBackend {
    pub fn new(
        connector: p::ProviderId,
        driver: Arc<dyn AppApiDriver>,
        secrets: Arc<dyn SecretResolver>,
        contents: Arc<dyn ContentResolver>,
        budget: OutputBudget,
        max_timeout: p::DurationMs,
    ) -> p::Result<Self> {
        if connector.0.trim().is_empty() || budget.max_bytes == 0 || max_timeout.0 == 0 {
            return Err(p::Error(
                "App API backend configuration is incomplete".into(),
            ));
        }
        Ok(Self {
            connector,
            driver,
            secrets,
            contents,
            budget,
            max_timeout,
            active: ActiveCancellations::default(),
        })
    }
}

impl ActionBackend for AppApiBackend {
    fn kind(&self) -> BackendKind {
        p::BackendKind::AppApi
    }

    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        let p::ActionParameters::AppApi(spec) = &intent.parameters else {
            return Err(p::Error(
                "App API backend requires App API parameters".into(),
            ));
        };
        if spec.connector != self.connector
            || spec.schema_version.0 == 0
            || spec.timeout.0 == 0
            || spec.timeout.0 > self.max_timeout.0
        {
            return Err(p::Error(
                "App API connector profile is not configured".into(),
            ));
        }
        plan_with(intent, self.budget.clone(), spec.timeout)
    }

    fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        cancel: CancelToken,
    ) -> p::Result<ActionResult> {
        validate_plan_for(&plan, self.kind())?;
        let p::ActionParameters::AppApi(spec) = &plan.intent.parameters else {
            return Err(p::Error("App API plan has incompatible parameters".into()));
        };
        if spec.connector != self.connector || spec.timeout != plan.timeout {
            return Err(p::Error(
                "App API plan targets another connector profile".into(),
            ));
        }
        self.active
            .register(plan.intent.intent_id.clone(), cancel.clone())?;
        let result = (|| {
            emit_started(&plan, sink)?;
            let mut output = BoundedOutput::new(plan.budget.clone());
            if cancel.is_cancelled() {
                return cancelled(&plan, sink, &output);
            }
            match self
                .driver
                .perform(spec, self.secrets.as_ref(), self.contents.as_ref())
            {
                Ok(AppApiDriverOutcome::Completed(receipt)) => {
                    receipt.validate()?;
                    output.push(receipt.summary.as_bytes(), &plan, sink)?;
                    complete_external(
                        &plan,
                        sink,
                        &output,
                        p::ExternalActionReceipt {
                            schema_version: p::SchemaVersion(1),
                            action: plan.intent.intent_id.clone(),
                            content_ref: receipt.content_ref,
                            content_digest: receipt.digest,
                            trust: p::TrustTier::Untrusted,
                            effect: receipt.effect,
                            probe_hint: None,
                        },
                    )
                }
                Ok(AppApiDriverOutcome::Unknown(probe_hint)) => {
                    outcome_unknown(&plan, sink, &output, probe_hint)
                }
                Err(error) => Err(failed(&plan, sink, error.to_string())),
            }
        })()
        .map_err(|error| fail_unless_terminal(&plan, sink, error));
        self.active.remove(&plan.intent.intent_id);
        result
    }

    fn cancel(&self, action: p::ActionId) -> p::Result<()> {
        self.active.cancel(&action)
    }
}
