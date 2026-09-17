use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use forme_protocol as p;

use crate::{
    ActionResult, EventSink, ExecutionPlan, FileDiff, FileRollback, OutputBudget, ResultNormalizer,
};

#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
pub(crate) struct ActiveCancellations {
    actions: Mutex<BTreeMap<p::ActionId, CancelToken>>,
}

impl ActiveCancellations {
    pub(crate) fn register(&self, action: p::ActionId, token: CancelToken) -> p::Result<()> {
        let mut actions = self
            .actions
            .lock()
            .map_err(|_| p::Error("execution cancellation state is unavailable".into()))?;
        if actions.contains_key(&action) {
            return Err(p::Error("action is already executing".into()));
        }
        actions.insert(action, token);
        Ok(())
    }

    pub(crate) fn remove(&self, action: &p::ActionId) {
        if let Ok(mut actions) = self.actions.lock() {
            actions.remove(action);
        }
    }

    pub(crate) fn cancel(&self, action: &p::ActionId) -> p::Result<()> {
        let actions = self
            .actions
            .lock()
            .map_err(|_| p::Error("execution cancellation state is unavailable".into()))?;
        let token = actions
            .get(action)
            .ok_or_else(|| p::Error("action is not currently executing".into()))?;
        token.cancel();
        Ok(())
    }
}

pub(crate) struct BoundedOutput {
    budget: OutputBudget,
    bytes: Vec<u8>,
    truncated: bool,
}

impl BoundedOutput {
    pub(crate) fn new(budget: OutputBudget) -> Self {
        Self {
            budget,
            bytes: Vec::new(),
            truncated: false,
        }
    }

    pub(crate) fn push(
        &mut self,
        bytes: &[u8],
        plan: &ExecutionPlan,
        sink: &EventSink,
    ) -> p::Result<()> {
        self.push_with_ref(bytes, plan, sink, None)
    }

    pub(crate) fn push_with_ref(
        &mut self,
        bytes: &[u8],
        plan: &ExecutionPlan,
        sink: &EventSink,
        content_ref: Option<p::ContentRef>,
    ) -> p::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let remaining = self
            .budget
            .max_bytes
            .saturating_sub(self.bytes.len() as u64) as usize;
        if bytes.len() > remaining && !self.budget.truncate {
            return Err(p::Error(
                "action output exceeded its strict byte budget".into(),
            ));
        }
        let accepted = bytes.len().min(remaining);
        self.bytes.extend_from_slice(&bytes[..accepted]);
        let truncated_now = accepted < bytes.len();
        self.truncated |= truncated_now;
        if accepted > 0 || truncated_now {
            sink.emit(p::EventPayload::ActionOutputDelta(
                p::ActionOutputDeltaPayload {
                    intent_id: plan.intent.intent_id.clone(),
                    backend: plan.backend,
                    scope: plan.scope.clone(),
                    delta: String::from_utf8_lossy(&bytes[..accepted]).into_owned(),
                    truncated: truncated_now,
                    trust: p::TrustTier::Untrusted,
                    content_ref,
                    remote_lease: None,
                },
            ))?;
        }
        Ok(())
    }

    pub(crate) fn output(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    pub(crate) fn truncated(&self) -> bool {
        self.truncated
    }
}

pub(crate) fn emit_started(plan: &ExecutionPlan, sink: &EventSink) -> p::Result<()> {
    sink.emit(p::EventPayload::ActionStarted(p::ActionStartedPayload {
        intent_id: plan.intent.intent_id.clone(),
        backend: plan.backend,
        scope: plan.scope.clone(),
        remote_lease: None,
    }))
}

pub(crate) fn complete(
    plan: &ExecutionPlan,
    sink: &EventSink,
    output: &BoundedOutput,
    diff: Option<FileDiff>,
    rollback: Option<FileRollback>,
) -> p::Result<ActionResult> {
    let result =
        ResultNormalizer::completed(plan, output.output(), output.truncated(), diff, rollback);
    sink.emit(p::EventPayload::ActionCompleted(
        p::ActionCompletedPayload {
            intent_id: plan.intent.intent_id.clone(),
            result_ref: result.result_ref.clone(),
            receipt: None,
            remote_receipt: None,
        },
    ))?;
    emit_evidence(&result, sink)?;
    Ok(result)
}

pub(crate) fn complete_external(
    plan: &ExecutionPlan,
    sink: &EventSink,
    output: &BoundedOutput,
    receipt: p::ExternalActionReceipt,
) -> p::Result<ActionResult> {
    let result = ResultNormalizer::completed_external(
        plan,
        output.output(),
        output.truncated(),
        receipt.clone(),
    );
    sink.emit(p::EventPayload::ActionCompleted(
        p::ActionCompletedPayload {
            intent_id: plan.intent.intent_id.clone(),
            result_ref: result.result_ref.clone(),
            receipt: Some(receipt),
            remote_receipt: None,
        },
    ))?;
    emit_evidence(&result, sink)?;
    Ok(result)
}

pub(crate) fn outcome_unknown(
    plan: &ExecutionPlan,
    sink: &EventSink,
    output: &BoundedOutput,
    probe_hint: p::ProbeHintRef,
) -> p::Result<ActionResult> {
    let receipt = p::ExternalActionReceipt {
        schema_version: p::SchemaVersion(1),
        action: plan.intent.intent_id.clone(),
        content_ref: None,
        content_digest: None,
        trust: p::TrustTier::Untrusted,
        effect: p::EffectStatus::Unknown,
        probe_hint: Some(probe_hint.clone()),
    };
    let result =
        ResultNormalizer::outcome_unknown(plan, output.output(), output.truncated(), receipt);
    sink.emit(p::EventPayload::ActionOutcomeUnknown(
        p::ActionOutcomeUnknownPayload {
            intent_id: plan.intent.intent_id.clone(),
            probe_hint,
            remote_lease: None,
        },
    ))?;
    emit_evidence(&result, sink)?;
    Ok(result)
}

pub(crate) fn cancelled(
    plan: &ExecutionPlan,
    sink: &EventSink,
    output: &BoundedOutput,
) -> p::Result<ActionResult> {
    let result = ResultNormalizer::cancelled(plan, output.output(), output.truncated());
    sink.emit(p::EventPayload::ActionCancelled(
        p::ActionCancelledPayload {
            intent_id: plan.intent.intent_id.clone(),
            reason: p::ReasonRef("cancelled by execution token".into()),
        },
    ))?;
    emit_evidence(&result, sink)?;
    Ok(result)
}

pub(crate) fn failed(
    plan: &ExecutionPlan,
    sink: &EventSink,
    detail: impl Into<String>,
) -> p::Error {
    let detail = detail.into();
    let _ = sink.emit(p::EventPayload::ActionFailed(p::ActionFailedPayload {
        intent_id: plan.intent.intent_id.clone(),
        failure_ref: p::FailureEvidenceRef(format!(
            "execution-failure:{}",
            plan.intent.intent_id.0
        )),
        remote_lease: None,
    }));
    let _ = sink.emit(p::EventPayload::CapabilityEvidenceRecorded(
        p::CapabilityEvidenceRecordedPayload {
            capability: plan.intent.capability_ref.clone(),
            outcome: p::CapabilityOutcome("failure".into()),
            reliability: p::Reliability("observed".into()),
        },
    ));
    p::Error(detail)
}

pub(crate) fn fail_unless_terminal(
    plan: &ExecutionPlan,
    sink: &EventSink,
    error: p::Error,
) -> p::Error {
    let has_terminal = sink.events().iter().any(|event| {
        matches!(
            event,
            p::EventPayload::ActionCompleted(payload)
                if payload.intent_id == plan.intent.intent_id
        ) || matches!(
            event,
            p::EventPayload::ActionFailed(payload)
                if payload.intent_id == plan.intent.intent_id
        ) || matches!(
            event,
            p::EventPayload::ActionCancelled(payload)
                if payload.intent_id == plan.intent.intent_id
        ) || matches!(
            event,
            p::EventPayload::ActionOutcomeUnknown(payload)
                if payload.intent_id == plan.intent.intent_id
        )
    });
    if has_terminal {
        error
    } else {
        failed(plan, sink, error.to_string())
    }
}

fn emit_evidence(result: &ActionResult, sink: &EventSink) -> p::Result<()> {
    sink.emit(p::EventPayload::CapabilityEvidenceRecorded(
        p::CapabilityEvidenceRecordedPayload {
            capability: result.evidence.capability.clone(),
            outcome: result.evidence.outcome.clone(),
            reliability: result.evidence.reliability.clone(),
        },
    ))
}

pub(crate) enum ProcessOutcome {
    Exited(ExitStatus),
    Cancelled,
    TimedOut,
}

pub(crate) fn monitor_child(
    child: &mut Child,
    timeout: p::DurationMs,
    token: &CancelToken,
    plan: &ExecutionPlan,
    sink: &EventSink,
    output: &mut BoundedOutput,
) -> p::Result<ProcessOutcome> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| p::Error("child process stdout is unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| p::Error("child process stderr is unavailable".into()))?;
    let (sender, receiver) = mpsc::sync_channel(16);
    let readers = vec![
        spawn_reader(stdout, sender.clone()),
        spawn_reader(stderr, sender.clone()),
    ];
    drop(sender);
    let started = Instant::now();
    let mut status = None;
    let mut output_closed = false;

    let outcome = loop {
        if token.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            break ProcessOutcome::Cancelled;
        }
        if started.elapsed() >= Duration::from_millis(timeout.0) {
            let _ = child.kill();
            let _ = child.wait();
            break ProcessOutcome::TimedOut;
        }

        match receiver.recv_timeout(Duration::from_millis(5)) {
            Ok(chunk) => {
                if let Err(error) = output.push(&chunk, plan, sink) {
                    let _ = child.kill();
                    let _ = child.wait();
                    drop(receiver);
                    for reader in readers {
                        let _ = reader.join();
                    }
                    return Err(error);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => output_closed = true,
        }
        if status.is_none() {
            status = child
                .try_wait()
                .map_err(|error| p::Error(format!("failed to inspect child process: {error}")))?;
        }
        if let Some(status) = status {
            if output_closed {
                break ProcessOutcome::Exited(status);
            }
        }
    };

    while let Ok(chunk) = receiver.recv() {
        output.push(&chunk, plan, sink)?;
    }
    for reader in readers {
        let _ = reader.join();
    }
    Ok(outcome)
}

fn spawn_reader<R>(mut reader: R, sender: mpsc::SyncSender<Vec<u8>>) -> std::thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 4_096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if sender.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    })
}
