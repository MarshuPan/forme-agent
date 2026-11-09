use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::{reject_linked_ancestors, worker_binding::WorkerExecutionBinding};

#[path = "worker_admission.rs"]
mod admission;
#[path = "worker_output_journal.rs"]
mod output;
#[path = "worker_projection.rs"]
mod projection;
#[path = "worker_recovery.rs"]
mod recovery;

const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 128 * 1024;

/// The worker journal is a product fact domain. It is intentionally separate
/// from the brain EventStore and records only admission and execution facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
enum WorkerFact {
    Admitted {
        job: Box<p::WorkerJob>,
        permit: Box<p::WorkerPermit>,
        at: p::Timestamp,
        #[serde(default)]
        execution_binding: Option<WorkerExecutionBinding>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_transfer: Option<Box<p::DesktopOutputTransfer>>,
    },
    DesktopObservation {
        job_id: String,
        binding: Box<p::DesktopSessionBinding>,
        observation: Box<p::DesktopSessionObservation>,
    },
    Started {
        job_id: String,
        at: p::Timestamp,
    },
    CancelRequested {
        job_id: String,
        at: p::Timestamp,
    },
    Receipt {
        receipt: p::WorkerReceipt,
    },
    DesktopOutput {
        descriptor: Box<p::DesktopOutputDescriptor>,
    },
    ResultManifest {
        manifest: p::WorkerResultManifest,
    },
    CancelReceipt {
        receipt: p::WorkerCancelReceipt,
    },
    Unknown {
        job_id: String,
        reason_ref: String,
        at: p::Timestamp,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerEnvelope {
    schema_version: p::SchemaVersion,
    worker_seq: u64,
    previous: p::SchemaDigest,
    fact: WorkerFact,
    digest: p::SchemaDigest,
}

impl WorkerEnvelope {
    fn calculate(
        version: p::SchemaVersion,
        seq: u64,
        previous: &p::SchemaDigest,
        fact: &WorkerFact,
    ) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&(version, seq, previous, fact))
    }
}

#[derive(Clone)]
struct WorkerRecord {
    job: p::WorkerJob,
    permit: p::WorkerPermit,
    execution_binding: Option<WorkerExecutionBinding>,
    desktop_binding: Option<p::DesktopSessionBinding>,
    desktop_observation: Option<p::DesktopSessionObservation>,
    state: p::WorkerState,
    receipt: Option<p::WorkerReceipt>,
    result_manifest: Option<p::WorkerResultManifest>,
    output_transfer: Option<p::DesktopOutputTransfer>,
    desktop_output: Option<p::DesktopOutputDescriptor>,
    cancel_receipt: Option<p::WorkerCancelReceipt>,
    reason_ref: Option<String>,
}

#[derive(Clone, Default)]
struct WorkerProjection {
    uses_output: bool,
    sequence: u64,
    head: Option<p::SchemaDigest>,
    jobs: BTreeMap<String, WorkerRecord>,
    nonces: BTreeSet<p::Nonce>,
    permits: BTreeSet<String>,
}

struct WorkerWriter {
    file: File,
    projection: WorkerProjection,
    poisoned: bool,
}

/// One OS-locked writer for worker admission and recovery facts.
pub struct WorkerJournal {
    writer: Mutex<WorkerWriter>,
}

impl WorkerJournal {
    pub fn open(path: &Path) -> p::Result<Self> {
        reject_linked_ancestors(path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(|_| storage_error())?;
        file.try_lock()
            .map_err(|_| p::Error("worker journal already has an active writer".into()))?;
        if file.metadata().map_err(|_| storage_error())?.len() > MAX_JOURNAL_BYTES {
            return Err(p::Error("worker journal requires offline archival".into()));
        }
        let mut projection = WorkerProjection::default();
        let mut reader = BufReader::new(&file);
        loop {
            let mut bytes = Vec::new();
            let count = reader
                .by_ref()
                .take((MAX_RECORD_BYTES + 1) as u64)
                .read_until(b'\n', &mut bytes)
                .map_err(|_| storage_error())?;
            if count == 0 {
                break;
            }
            if count > MAX_RECORD_BYTES || bytes.last() != Some(&b'\n') {
                return Err(invalid_journal());
            }
            let envelope: WorkerEnvelope =
                serde_json::from_slice(&bytes).map_err(|_| invalid_journal())?;
            projection.fold(&envelope)?;
        }
        let journal = Self {
            writer: Mutex::new(WorkerWriter {
                file,
                projection,
                poisoned: false,
            }),
        };

        // A process can die after worker admission and before a terminal
        // receipt. Those jobs are not reclaimable: mark the exact old job
        // Unknown so a caller must probe or ask the owner before retrying.
        let pending: Vec<_> = {
            let writer = journal.writer.lock().map_err(|_| storage_error())?;
            writer
                .projection
                .jobs
                .values()
                .filter(|record| {
                    matches!(
                        record.state,
                        p::WorkerState::Pending
                            | p::WorkerState::Running
                            | p::WorkerState::CancelRequested
                    )
                })
                .map(|record| record.job.job_id.clone())
                .collect()
        };
        for job_id in pending {
            let reason = p::canonical_digest(&("worker-restart-unknown", &job_id))?.0;
            journal.mark_unknown(&job_id, reason, 1)?;
        }
        Ok(journal)
    }

    pub fn start(&self, job_id: &str, now: p::Timestamp) -> p::Result<p::WorkerObservation> {
        validate_worker_id(job_id)?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        let record = writer
            .projection
            .jobs
            .get(job_id)
            .ok_or_else(|| p::Error("worker job does not exist".into()))?;
        if record.state == p::WorkerState::Running {
            return Ok(observation(record, now));
        }
        append_fact(
            &mut writer,
            WorkerFact::Started {
                job_id: job_id.into(),
                at: now,
            },
        )?;
        let record = writer
            .projection
            .jobs
            .get(job_id)
            .ok_or_else(invalid_journal)?;
        Ok(observation(record, now))
    }

    pub fn request_cancel(
        &self,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        validate_worker_id(job_id)?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        let record = writer
            .projection
            .jobs
            .get(job_id)
            .ok_or_else(|| p::Error("worker job does not exist".into()))?;
        if record.state == p::WorkerState::CancelRequested {
            return Ok(observation(record, now));
        }
        append_fact(
            &mut writer,
            WorkerFact::CancelRequested {
                job_id: job_id.into(),
                at: now,
            },
        )?;
        let record = writer
            .projection
            .jobs
            .get(job_id)
            .ok_or_else(invalid_journal)?;
        Ok(observation(record, now))
    }

    pub fn record_receipt(
        &self,
        receipt: p::WorkerReceipt,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        let job_id = receipt.job_id.clone();
        let job = writer
            .projection
            .jobs
            .get(&job_id)
            .ok_or_else(|| p::Error("worker job does not exist".into()))?;
        receipt.validate_for(&job.job)?;
        append_fact(&mut writer, WorkerFact::Receipt { receipt })?;
        let record = writer
            .projection
            .jobs
            .get(&job_id)
            .ok_or_else(invalid_journal)?;
        Ok(observation(record, now))
    }

    pub fn record_result_manifest(&self, manifest: p::WorkerResultManifest) -> p::Result<()> {
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        let job = writer
            .projection
            .jobs
            .get(&manifest.binding.job_id)
            .ok_or_else(|| p::Error("worker job does not exist".into()))?;
        manifest.validate_for(&job.job, &job.permit)?;
        if job.result_manifest.as_ref() == Some(&manifest) {
            return Ok(());
        }
        append_fact(&mut writer, WorkerFact::ResultManifest { manifest })
    }

    pub fn record_desktop_observation(
        &self,
        job_id: &str,
        binding: p::DesktopSessionBinding,
        observation: p::DesktopSessionObservation,
        now: p::Timestamp,
    ) -> p::Result<()> {
        validate_worker_id(job_id)?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        let record = writer
            .projection
            .jobs
            .get(job_id)
            .ok_or_else(|| p::Error("worker job does not exist".into()))?;
        if !matches!(
            record.state,
            p::WorkerState::Pending | p::WorkerState::Running
        ) {
            return Err(p::Error(
                "desktop session observation is not current".into(),
            ));
        }
        binding.validate_for(&record.job, &record.permit, now)?;
        observation.validate()?;
        if observation.observed_at > now {
            return Err(p::Error(
                "desktop session observation is from the future".into(),
            ));
        }
        if observation.state == p::DesktopSessionState::Ready {
            observation.validate_for(&binding)?;
        } else if observation.helper_ref != binding.helper_ref
            || observation.platform != binding.platform
        {
            return Err(p::Error(
                "desktop session observation is not bound to the grant".into(),
            ));
        }
        append_fact(
            &mut writer,
            WorkerFact::DesktopObservation {
                job_id: job_id.into(),
                binding: Box::new(binding),
                observation: Box::new(observation),
            },
        )
    }

    pub fn record_cancel_receipt(
        &self,
        receipt: p::WorkerCancelReceipt,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        let job_id = receipt.job_id.clone();
        let job = writer
            .projection
            .jobs
            .get(&job_id)
            .ok_or_else(|| p::Error("worker job does not exist".into()))?;
        receipt.validate_for(&job.job)?;
        append_fact(&mut writer, WorkerFact::CancelReceipt { receipt })?;
        let record = writer
            .projection
            .jobs
            .get(&job_id)
            .ok_or_else(invalid_journal)?;
        Ok(observation(record, now))
    }

    pub fn mark_unknown(
        &self,
        job_id: &str,
        reason_ref: String,
        now: p::Timestamp,
    ) -> p::Result<bool> {
        validate_worker_id(job_id)?;
        validate_worker_ref(&reason_ref, "worker unknown reason")?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        let Some(record) = writer.projection.jobs.get(job_id) else {
            return Err(p::Error("worker job does not exist".into()));
        };
        if record.state == p::WorkerState::Unknown {
            return Ok(false);
        }
        if matches!(
            record.state,
            p::WorkerState::Completed | p::WorkerState::Failed | p::WorkerState::Cancelled
        ) {
            return Ok(false);
        }
        append_fact(
            &mut writer,
            WorkerFact::Unknown {
                job_id: job_id.into(),
                reason_ref,
                at: now,
            },
        )?;
        Ok(true)
    }

    pub fn observation(&self, job_id: &str, now: p::Timestamp) -> p::Result<p::WorkerObservation> {
        validate_worker_id(job_id)?;
        let should_expire = {
            let writer = self.writer.lock().map_err(|_| storage_error())?;
            let record = writer
                .projection
                .jobs
                .get(job_id)
                .ok_or_else(|| p::Error("worker job does not exist".into()))?;
            now >= record.job.deadline
                && matches!(
                    record.state,
                    p::WorkerState::Pending
                        | p::WorkerState::Running
                        | p::WorkerState::CancelRequested
                )
        };
        if should_expire {
            let reason = p::canonical_digest(&("worker-deadline-unknown", job_id))?.0;
            self.mark_unknown(job_id, reason, now)?;
        }
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        let record = writer
            .projection
            .jobs
            .get(job_id)
            .ok_or_else(|| p::Error("worker job does not exist".into()))?;
        Ok(observation(record, now))
    }

    pub fn existing_observation(
        &self,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<Option<p::WorkerObservation>> {
        validate_worker_id(job_id)?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        Ok(writer
            .projection
            .jobs
            .get(job_id)
            .map(|record| observation(record, now)))
    }

    pub fn job(&self, job_id: &str) -> p::Result<p::WorkerJob> {
        validate_worker_id(job_id)?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        writer
            .projection
            .jobs
            .get(job_id)
            .map(|record| record.job.clone())
            .ok_or_else(|| p::Error("worker job does not exist".into()))
    }

    pub fn result_manifest(&self, job_id: &str) -> p::Result<Option<p::WorkerResultManifest>> {
        validate_worker_id(job_id)?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        Ok(writer
            .projection
            .jobs
            .get(job_id)
            .and_then(|record| record.result_manifest.clone()))
    }

    pub fn result_binding(
        &self,
        job_id: &str,
        installation_ref: String,
        receipt_ref: Option<String>,
    ) -> p::Result<p::WorkerResultBinding> {
        validate_worker_id(job_id)?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        let record = writer
            .projection
            .jobs
            .get(job_id)
            .ok_or_else(|| p::Error("worker job does not exist".into()))?;
        let (normalized_action_digest, output_budget_bytes) = record
            .execution_binding
            .as_ref()
            .map(|binding| {
                (
                    Some(binding.normalized_action_digest.clone()),
                    binding.output_budget_bytes,
                )
            })
            .unwrap_or((None, p::WORKER_RESULT_MAX_BYTES));
        let binding = p::WorkerResultBinding {
            schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
            installation_ref,
            job_id: record.job.job_id.clone(),
            owner_ref: record.job.owner_ref.clone(),
            action_ref: record.job.action_ref.clone(),
            workspace_scope: record.job.workspace_scope.clone(),
            plan_digest: record.job.plan_digest.clone(),
            normalized_action_digest,
            permit_ref: record.permit.permit_ref.clone(),
            permit_digest: p::canonical_digest(&record.permit)?,
            authority_epoch: record.job.authority_epoch,
            nonce: record.job.nonce.clone(),
            receipt_ref,
            output_budget_bytes,
        };
        binding.validate()?;
        Ok(binding)
    }
}

fn observation(record: &WorkerRecord, now: p::Timestamp) -> p::WorkerObservation {
    let (outcome, receipt_ref, receipt_result_ref, ground_truth_refs) = record
        .receipt
        .as_ref()
        .map(|receipt| {
            (
                Some(receipt.observed_outcome),
                Some(receipt.receipt_ref.clone()),
                receipt.safe_result_ref.clone(),
                receipt.ground_truth_refs.clone(),
            )
        })
        .or_else(|| {
            record.cancel_receipt.as_ref().map(|receipt| {
                (
                    Some(receipt.outcome),
                    receipt.receipt_ref.clone(),
                    None,
                    Vec::new(),
                )
            })
        })
        .unwrap_or_else(|| {
            (
                match record.state {
                    p::WorkerState::Unknown => Some(p::WorkerOutcome::Unknown),
                    _ => None,
                },
                None,
                None,
                Vec::new(),
            )
        });
    let safe_result_ref = receipt_result_ref.or_else(|| {
        record
            .result_manifest
            .as_ref()
            .and_then(|manifest| manifest.result_ref.clone())
    });
    p::WorkerObservation {
        schema_version: p::SchemaVersion(1),
        job_id: record.job.job_id.clone(),
        plan_digest: record.job.plan_digest.clone(),
        authority_epoch: record.job.authority_epoch,
        nonce: record.job.nonce.clone(),
        state: record.state,
        outcome,
        receipt_ref,
        safe_result_ref,
        ground_truth_refs,
        observed_at: now,
        reason_ref: record.reason_ref.clone(),
    }
}

fn validate_worker_id(value: &str) -> p::Result<()> {
    validate_worker_ref(value, "worker job")
}

fn validate_worker_ref(value: &str, name: &str) -> p::Result<()> {
    p::validate_product_id(value).map_err(|_| p::Error(format!("{name} is invalid")))
}

fn append_fact(writer: &mut WorkerWriter, fact: WorkerFact) -> p::Result<()> {
    if writer.poisoned {
        return Err(storage_error());
    }
    let worker_seq = writer
        .projection
        .sequence
        .checked_add(1)
        .ok_or_else(invalid_journal)?;
    let previous = writer.projection.head()?;
    let output_fact = matches!(
        &fact,
        WorkerFact::DesktopOutput { .. }
            | WorkerFact::Admitted {
                output_transfer: Some(_),
                ..
            }
    );
    let version = p::SchemaVersion(if writer.projection.uses_output || output_fact {
        2
    } else {
        1
    });
    let envelope = WorkerEnvelope {
        schema_version: version,
        worker_seq,
        digest: WorkerEnvelope::calculate(version, worker_seq, &previous, &fact)?,
        previous,
        fact,
    };
    let mut next = writer.projection.clone();
    next.fold(&envelope)?;
    let mut bytes = serde_json::to_vec(&envelope).map_err(|_| invalid_journal())?;
    bytes.push(b'\n');
    if bytes.len() > MAX_RECORD_BYTES
        || writer.file.metadata().map_err(|_| storage_error())?.len() + bytes.len() as u64
            > MAX_JOURNAL_BYTES
    {
        return Err(storage_error());
    }
    writer.poisoned = true;
    writer
        .file
        .seek(SeekFrom::End(0))
        .map_err(|_| storage_error())?;
    writer.file.write_all(&bytes).map_err(|_| storage_error())?;
    writer.file.sync_all().map_err(|_| storage_error())?;
    writer.projection = next;
    writer.poisoned = false;
    Ok(())
}

fn invalid_journal() -> p::Error {
    p::Error("worker journal requires recovery; no job was replayed".into())
}

fn storage_error() -> p::Error {
    p::Error("worker journal storage is unavailable; outcome must be checked".into())
}
