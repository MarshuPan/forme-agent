//! Managed core-to-worker execution adapter.
//!
//! The adapter is deliberately below Harness policy and above the typed
//! AgentExecution transport. It never executes a local command and it never
//! creates a replacement job after dispatch has been attempted.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use forme_protocol as p;

use crate::planner::{plan_with, validate_plan_for};
use crate::support::{
    cancelled, complete_external, emit_started, failed, outcome_unknown, BoundedOutput,
};
use crate::{
    ActionBackend, ActionResult, BackendKind, CancelToken, EventSink, ExecutionPlan, OutputBudget,
};

use super::{ManagedMcpDiscoveryConsumer, ManagedWorkerBinding, WorkerBrokerTransport};

mod binding;
use binding::*;
mod channel;
mod desktop;
mod discovery;
mod discovery_recovery;
mod output;

const POLL_INTERVAL: Duration = Duration::from_millis(25);
const RESULT_ASSEMBLY_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESULT_CHUNKS: usize = 4096;

#[derive(Clone)]
struct WorkerAttempt {
    job: p::WorkerJob,
    permit: p::WorkerPermit,
    dispatch_attempted: bool,
    desktop: Option<desktop::DesktopAttempt>,
}

/// `ActionBackend` implementation for an installed, isolated worker.
pub struct WorkerActionBackend {
    kind: BackendKind,
    transport: Arc<dyn WorkerBrokerTransport + Send + Sync>,
    owner: p::VerifiedPrincipal,
    authority_epoch: p::AuthorityEpoch,
    sandbox_profile_ref: String,
    budget: OutputBudget,
    timeout: p::DurationMs,
    attempts: Mutex<BTreeMap<p::ActionId, WorkerAttempt>>,
    artifacts: Option<Arc<dyn crate::ArtifactStore>>,
    channel: Option<channel::ChannelConsumer>,
    discovery_consumer: Option<Arc<dyn ManagedMcpDiscoveryConsumer>>,
    discovery_dispatch: Option<discovery_recovery::Recorder>,
}

impl WorkerActionBackend {
    pub fn new(
        kind: BackendKind,
        transport: Arc<dyn WorkerBrokerTransport + Send + Sync>,
        owner: p::VerifiedPrincipal,
        authority_epoch: p::AuthorityEpoch,
        sandbox_profile_ref: impl Into<String>,
        budget: OutputBudget,
        timeout: p::DurationMs,
    ) -> p::Result<Self> {
        p::validate_product_id(&owner.0).map_err(|_| p::Error("worker owner is invalid".into()))?;
        if authority_epoch.0 == 0
            || budget.schema_version != p::SchemaVersion(1)
            || budget.max_bytes == 0
            || budget.max_bytes > p::WORKER_RESULT_MAX_BYTES
            || timeout.0 == 0
            || timeout.0 > 600_000
        {
            return Err(p::Error(
                "worker action backend configuration is incomplete".into(),
            ));
        }
        let sandbox_profile_ref = sandbox_profile_ref.into();
        p::validate_product_id(&sandbox_profile_ref)
            .map_err(|_| p::Error("worker sandbox profile is invalid".into()))?;
        Ok(Self {
            kind,
            transport,
            owner,
            authority_epoch,
            sandbox_profile_ref,
            budget,
            timeout,
            attempts: Mutex::new(BTreeMap::new()),
            artifacts: None,
            channel: None,
            discovery_consumer: None,
            discovery_dispatch: None,
        })
    }

    pub fn from_binding(
        kind: BackendKind,
        binding: &ManagedWorkerBinding,
        owner: p::VerifiedPrincipal,
        budget: OutputBudget,
        timeout: p::DurationMs,
    ) -> p::Result<Self> {
        binding.validate()?;
        if !binding.supports(kind) {
            return Err(p::Error("worker backend kind is not installed".into()));
        }
        Self::new(
            kind,
            binding.connect()?,
            owner,
            binding.authority_epoch,
            if kind == BackendKind::Mcp {
                p::MANAGED_MCP_PROGRAM_PROFILE.into()
            } else {
                binding.sandbox_profile_ref.clone()
            },
            budget,
            timeout,
        )
    }

    fn settle(
        &self,
        attempt: &WorkerAttempt,
        initial: p::WorkerObservation,
        token: &CancelToken,
    ) -> Settlement {
        let started = Instant::now();
        let timeout = Duration::from_millis(
            attempt
                .job
                .deadline
                .saturating_sub(now_ms())
                .try_into()
                .unwrap_or(0),
        );
        let mut observation = initial;
        let mut cancel_sent = false;
        loop {
            if is_terminal(observation.state) {
                return Settlement::Terminal(observation);
            }
            if (token.is_cancelled() || started.elapsed() >= timeout) && !cancel_sent {
                cancel_sent = true;
                match self.transport.cancel(&attempt.job.job_id) {
                    Ok(receipt) => match receipt.validate_for(&attempt.job) {
                        Ok(()) if receipt.outcome == p::WorkerOutcome::Cancelled => {
                            return Settlement::Cancelled;
                        }
                        Ok(()) => return Settlement::Unknown,
                        Err(_) => return Settlement::Unknown,
                    },
                    Err(_) => {
                        // A cancel response can race with a real terminal
                        // receipt. One probe of the original job is allowed;
                        // a replacement cancel or dispatch is not.
                        match self.transport.probe(&attempt.job.job_id) {
                            Ok(next) if validate_observation(&next, &attempt.job).is_ok() => {
                                observation = next;
                                continue;
                            }
                            _ => return Settlement::Unknown,
                        }
                    }
                }
            }
            if started.elapsed() >= timeout {
                return Settlement::Unknown;
            }
            thread::sleep(POLL_INTERVAL);
            observation = match self.transport.probe(&attempt.job.job_id) {
                Ok(next) if validate_observation(&next, &attempt.job).is_ok() => next,
                _ => return Settlement::Unknown,
            };
        }
    }

    fn result_for(
        &self,
        plan: &ExecutionPlan,
        attempt: &WorkerAttempt,
        observation: &p::WorkerObservation,
        sink: &EventSink,
    ) -> p::Result<ActionResult> {
        if observation.outcome == Some(p::WorkerOutcome::Unknown) {
            return unknown_result(plan, sink, &self.budget, &attempt.job.job_id);
        }
        let result_started = Instant::now();
        let manifest = match self.transport.result_manifest(&attempt.job.job_id) {
            Ok(manifest) => manifest,
            Err(_) => return unknown_result(plan, sink, &self.budget, &attempt.job.job_id),
        };
        if validate_manifest(&manifest, attempt, plan, observation).is_err() {
            return unknown_result(plan, sink, &self.budget, &attempt.job.job_id);
        }
        if output::is_screenshot(plan) {
            return self.complete_image(
                plan,
                attempt,
                observation,
                &manifest,
                sink,
                result_started,
            );
        }
        if !matches!(
            manifest.status,
            p::WorkerResultStatus::Available | p::WorkerResultStatus::Empty
        ) || manifest.encoding != p::WorkerResultEncoding::Utf8
        {
            return unknown_result(plan, sink, &self.budget, &attempt.job.job_id);
        }
        let bytes = match self.read_result(&manifest, result_started) {
            Ok(bytes) => bytes,
            Err(_) => return unknown_result(plan, sink, &self.budget, &attempt.job.job_id),
        };
        if observation.outcome == Some(p::WorkerOutcome::Completed)
            && self.verify_channel_result(plan, attempt, &bytes).is_err()
        {
            return unknown_result(plan, sink, &self.budget, &attempt.job.job_id);
        }
        if matches!(plan.intent.parameters, p::ActionParameters::McpDiscovery(_))
            && observation.outcome == Some(p::WorkerOutcome::Completed)
        {
            return self.complete_discovery(plan, attempt, observation, &manifest, &bytes, sink);
        }
        let mut output = BoundedOutput::new(self.budget.clone());
        if output.push(&bytes, plan, sink).is_err() {
            return unknown_result(plan, sink, &self.budget, &attempt.job.job_id);
        }
        match observation.outcome {
            Some(p::WorkerOutcome::Completed) => {
                emit_managed_call(plan, sink, None)?;
                let receipt = external_receipt(plan, &manifest, p::EffectStatus::Committed);
                complete_external(plan, sink, &output, receipt)
            }
            Some(p::WorkerOutcome::Failed) => {
                emit_managed_call(plan, sink, Some("server_error"))?;
                Err(failed(plan, sink, "managed worker reported failure"))
            }
            Some(p::WorkerOutcome::Cancelled) => {
                emit_managed_call(plan, sink, None)?;
                cancelled(plan, sink, &output)
            }
            Some(p::WorkerOutcome::Unknown) | None => {
                unknown_result(plan, sink, &self.budget, &attempt.job.job_id)
            }
        }
    }

    fn read_result(
        &self,
        manifest: &p::WorkerResultManifest,
        started: Instant,
    ) -> p::Result<Vec<u8>> {
        let timeout = Duration::from_millis(self.timeout.0).min(RESULT_ASSEMBLY_TIMEOUT);
        let mut bytes = Vec::with_capacity(manifest.bytes.min(1024 * 1024) as usize);
        let mut offset = 0_u64;
        let mut chunks = 0;
        loop {
            if started.elapsed() >= timeout || chunks >= MAX_RESULT_CHUNKS {
                return Err(p::Error(
                    "worker result assembly budget is exhausted".into(),
                ));
            }
            let request = p::WorkerResultRead {
                schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
                binding: manifest.binding.clone(),
                manifest_digest: manifest.manifest_digest.clone(),
                offset,
                max_bytes: p::WORKER_RESULT_MAX_CHUNK_BYTES,
            };
            let chunk = self.transport.read_result(request)?;
            chunks += 1;
            if started.elapsed() >= timeout {
                return Err(p::Error(
                    "worker result arrived after its read budget".into(),
                ));
            }
            chunk.validate()?;
            if chunk.manifest_digest != manifest.manifest_digest
                || chunk.offset != offset
                || (chunk.next_offset <= offset && !chunk.finished)
                || chunk.next_offset > manifest.bytes
                || (chunk.next_offset == manifest.bytes && !chunk.finished)
            {
                return Err(p::Error("worker result chunk offset is invalid".into()));
            }
            bytes.extend_from_slice(&chunk.bytes);
            if bytes.len() as u64 > manifest.bytes {
                return Err(p::Error("worker result exceeds its manifest size".into()));
            }
            offset = chunk.next_offset;
            if chunk.finished {
                if offset != manifest.bytes {
                    return Err(p::Error("worker result ended at the wrong offset".into()));
                }
                break;
            }
        }
        let expected_digest = p::sha256_content_digest(&bytes);
        match manifest.content_digest.as_ref() {
            Some(digest) if digest == &expected_digest => {}
            None if manifest.bytes == 0 => {}
            _ => return Err(p::Error("worker result digest is invalid".into())),
        }
        if manifest.encoding == p::WorkerResultEncoding::Utf8
            && std::str::from_utf8(&bytes).is_err()
        {
            return Err(p::Error("worker UTF-8 result is invalid".into()));
        }
        Ok(bytes)
    }
}

impl ActionBackend for WorkerActionBackend {
    fn kind(&self) -> BackendKind {
        self.kind
    }

    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        if intent.backend_hint != self.kind {
            return Err(p::Error(
                "worker action was routed to the wrong backend".into(),
            ));
        }
        let mut plan = plan_with(intent, self.budget.clone(), self.timeout)?;
        self.require_output_consumer(&plan)?;
        self.check_channel_plan(&plan)?;
        if is_file_mutation(intent) {
            // A Host overlay is a commit boundary, not a post-completion
            // FileRollback snapshot that this adapter could restore.
            plan.rollback_boundary = p::RollbackBoundary("none".into());
            plan.refresh_digest()?;
        }
        Ok(plan)
    }

    fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        token: CancelToken,
    ) -> p::Result<ActionResult> {
        validate_plan_for(&plan, self.kind)?;
        if plan.timeout != self.timeout
            || plan.budget != self.budget
            || (is_file_mutation(&plan.intent) && plan.rollback_boundary.0 != "none")
        {
            return Err(p::Error(
                "worker execution limits or rollback differ from the planned backend".into(),
            ));
        }
        self.require_output_consumer(&plan)?;
        let empty = BoundedOutput::new(self.budget.clone());
        let (attempt, fresh) = self.claim_dispatch(&plan, now_ms())?;
        if fresh && token.is_cancelled() {
            return cancelled(&plan, sink, &empty);
        }
        let observation = if fresh {
            let request = p::WorkerDispatchRequest {
                schema_version: p::SchemaVersion(1),
                job: attempt.job.clone(),
                permit: attempt.permit.clone(),
                execution: Some(execution_spec(&plan)),
            };
            self.record_discovery_dispatch(&request)?;
            emit_started(&plan, sink)?;
            if token.is_cancelled() {
                return cancelled(&plan, sink, &empty);
            }
            match self.dispatch_request(&plan, request) {
                Ok(observation) if validate_observation(&observation, &attempt.job).is_ok() => {
                    observation
                }
                _ => return unknown_result(&plan, sink, &self.budget, &attempt.job.job_id),
            }
        } else {
            match self.transport.probe(&attempt.job.job_id) {
                Ok(observation) if validate_observation(&observation, &attempt.job).is_ok() => {
                    observation
                }
                _ => return unknown_result(&plan, sink, &self.budget, &attempt.job.job_id),
            }
        };
        match self.settle(&attempt, observation, &token) {
            Settlement::Cancelled => {
                emit_managed_call(&plan, sink, None)?;
                cancelled(&plan, sink, &empty)
            }
            Settlement::Unknown => unknown_result(&plan, sink, &self.budget, &attempt.job.job_id),
            Settlement::Terminal(observation) => {
                self.result_for(&plan, &attempt, &observation, sink)
            }
        }
    }

    fn cancel(&self, action: p::ActionId) -> p::Result<()> {
        let attempt = self
            .attempts
            .lock()
            .map_err(|_| p::Error("worker action attempt state is unavailable".into()))?
            .get(&action)
            .cloned()
            .ok_or_else(|| p::Error("worker action is not currently known".into()))?;
        let receipt = self.transport.cancel(&attempt.job.job_id)?;
        receipt.validate_for(&attempt.job)?;
        if receipt.outcome == p::WorkerOutcome::Unknown {
            return Err(p::Error("worker cancellation outcome is unknown".into()));
        }
        Ok(())
    }
}

enum Settlement {
    Terminal(p::WorkerObservation),
    Cancelled,
    Unknown,
}

fn is_file_mutation(intent: &p::ActionIntent) -> bool {
    matches!(
        intent.parameters,
        p::ActionParameters::File {
            operation: p::FileOperation::Write | p::FileOperation::Delete,
            ..
        }
    )
}
