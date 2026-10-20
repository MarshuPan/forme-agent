use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::{reject_linked_ancestors, HostAuth, HostCredentialDomain};

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
        seq: u64,
        previous: &p::SchemaDigest,
        fact: &WorkerFact,
    ) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&(p::SchemaVersion(1), seq, previous, fact))
    }
}

#[derive(Clone)]
struct WorkerRecord {
    job: p::WorkerJob,
    permit: p::WorkerPermit,
    state: p::WorkerState,
    receipt: Option<p::WorkerReceipt>,
    reason_ref: Option<String>,
}

#[derive(Clone, Default)]
struct WorkerProjection {
    sequence: u64,
    head: Option<p::SchemaDigest>,
    jobs: BTreeMap<String, WorkerRecord>,
    nonces: BTreeSet<p::Nonce>,
    permits: BTreeSet<String>,
}

impl WorkerProjection {
    fn head(&self) -> p::Result<p::SchemaDigest> {
        self.head
            .clone()
            .map_or_else(|| p::canonical_digest(&"forme-worker-journal-v1"), Ok)
    }

    fn fold(&mut self, envelope: &WorkerEnvelope) -> p::Result<()> {
        if envelope.schema_version != p::SchemaVersion(1)
            || envelope.worker_seq != self.sequence.checked_add(1).ok_or_else(invalid_journal)?
            || envelope.previous != self.head()?
            || envelope.digest
                != WorkerEnvelope::calculate(
                    envelope.worker_seq,
                    &envelope.previous,
                    &envelope.fact,
                )?
        {
            return Err(invalid_journal());
        }

        match &envelope.fact {
            WorkerFact::Admitted { job, permit, at } => {
                job.validate()?;
                permit.validate_for(job, *at)?;
                if self.jobs.contains_key(&job.job_id)
                    || !self.nonces.insert(job.nonce.clone())
                    || !self.permits.insert(job.permit_ref.clone())
                {
                    return Err(invalid_journal());
                }
                self.jobs.insert(
                    job.job_id.clone(),
                    WorkerRecord {
                        job: *job.clone(),
                        permit: *permit.clone(),
                        state: p::WorkerState::Pending,
                        receipt: None,
                        reason_ref: None,
                    },
                );
            }
            WorkerFact::Started { job_id, at } => {
                validate_worker_id(job_id)?;
                let record = self.jobs.get_mut(job_id).ok_or_else(invalid_journal)?;
                if *at <= 0 || record.state != p::WorkerState::Pending {
                    return Err(invalid_journal());
                }
                record.state = p::WorkerState::Running;
            }
            WorkerFact::CancelRequested { job_id, at } => {
                validate_worker_id(job_id)?;
                let record = self.jobs.get_mut(job_id).ok_or_else(invalid_journal)?;
                if *at <= 0
                    || !matches!(
                        record.state,
                        p::WorkerState::Pending | p::WorkerState::Running
                    )
                {
                    return Err(invalid_journal());
                }
                record.state = p::WorkerState::CancelRequested;
            }
            WorkerFact::Receipt { receipt } => {
                let record = self
                    .jobs
                    .get_mut(&receipt.job_id)
                    .ok_or_else(invalid_journal)?;
                receipt.validate_for(&record.job)?;
                if !matches!(
                    record.state,
                    p::WorkerState::Running | p::WorkerState::CancelRequested
                ) || receipt.finished_at.is_none()
                {
                    return Err(invalid_journal());
                }
                record.state = match receipt.observed_outcome {
                    p::WorkerOutcome::Completed => p::WorkerState::Completed,
                    p::WorkerOutcome::Failed => p::WorkerState::Failed,
                    p::WorkerOutcome::Cancelled => p::WorkerState::Cancelled,
                    p::WorkerOutcome::Unknown => p::WorkerState::Unknown,
                };
                record.receipt = Some(receipt.clone());
                record.reason_ref = None;
            }
            WorkerFact::CancelReceipt { receipt } => {
                let record = self
                    .jobs
                    .get_mut(&receipt.job_id)
                    .ok_or_else(invalid_journal)?;
                receipt.validate_for(&record.job)?;
                if !matches!(
                    record.state,
                    p::WorkerState::Pending
                        | p::WorkerState::Running
                        | p::WorkerState::CancelRequested
                ) {
                    return Err(invalid_journal());
                }
                record.state = match receipt.outcome {
                    p::WorkerOutcome::Cancelled => p::WorkerState::Cancelled,
                    p::WorkerOutcome::Unknown => p::WorkerState::Unknown,
                    _ => return Err(invalid_journal()),
                };
                record.reason_ref = receipt.reason_ref.clone();
            }
            WorkerFact::Unknown {
                job_id,
                reason_ref,
                at,
            } => {
                validate_worker_id(job_id)?;
                validate_worker_ref(reason_ref, "worker unknown reason")?;
                let record = self.jobs.get_mut(job_id).ok_or_else(invalid_journal)?;
                if *at <= 0
                    || !matches!(
                        record.state,
                        p::WorkerState::Pending
                            | p::WorkerState::Running
                            | p::WorkerState::CancelRequested
                    )
                {
                    return Err(invalid_journal());
                }
                record.state = p::WorkerState::Unknown;
                record.reason_ref = Some(reason_ref.clone());
            }
        }
        self.sequence = envelope.worker_seq;
        self.head = Some(envelope.digest.clone());
        Ok(())
    }
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

    pub fn admit(
        &self,
        job: p::WorkerJob,
        permit: p::WorkerPermit,
        now: p::Timestamp,
    ) -> p::Result<(bool, p::WorkerObservation)> {
        job.validate()?;
        permit.validate_for(&job, now)?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        if let Some(record) = writer.projection.jobs.get(&job.job_id) {
            if record.job != job || record.permit != permit {
                return Err(p::Error(
                    "worker job id conflicts with its original admission".into(),
                ));
            }
            return Ok((false, observation(record, now)));
        }
        let job_id = job.job_id.clone();
        append_fact(
            &mut writer,
            WorkerFact::Admitted {
                job: Box::new(job),
                permit: Box::new(permit),
                at: now,
            },
        )?;
        let record = writer
            .projection
            .jobs
            .get(&job_id)
            .ok_or_else(invalid_journal)?;
        Ok((true, observation(record, now)))
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
}

fn observation(record: &WorkerRecord, now: p::Timestamp) -> p::WorkerObservation {
    let (outcome, receipt_ref, safe_result_ref, ground_truth_refs) = record
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

/// An isolation implementation must represent real OS-level admission. The
/// default is intentionally unavailable, so an unconfigured product cannot
/// silently execute a worker as the brain service account.
pub trait WorkerIsolation: Send + Sync {
    fn admit(&self, job: &p::WorkerJob, permit: &p::WorkerPermit) -> p::Result<()>;

    fn admit_at(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        _now: p::Timestamp,
    ) -> p::Result<()> {
        self.admit(job, permit)
    }

    fn cancel(&self, job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt>;

    /// Return a durable receipt only when the underlying worker has a known
    /// terminal result. The default keeps non-process fixtures observation
    /// only; it never invents a completion outcome.
    fn observe(
        &self,
        _job: &p::WorkerJob,
        _now: p::Timestamp,
    ) -> p::Result<Option<p::WorkerReceipt>> {
        Ok(None)
    }

    /// The timestamp-aware form lets a real process adapter bind a cancel
    /// receipt to the actual observation time without changing older test
    /// adapters that implement `cancel`.
    fn cancel_at(
        &self,
        job: &p::WorkerJob,
        _now: p::Timestamp,
    ) -> p::Result<p::WorkerCancelReceipt> {
        self.cancel(job)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableWorkerIsolation;

impl WorkerIsolation for UnavailableWorkerIsolation {
    fn admit(&self, _job: &p::WorkerJob, _permit: &p::WorkerPermit) -> p::Result<()> {
        Err(p::Error(
            "worker isolation capability is unavailable; execution is blocked".into(),
        ))
    }

    fn cancel(&self, _job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt> {
        Err(p::Error(
            "worker isolation capability is unavailable; cancellation is unverifiable".into(),
        ))
    }
}

pub trait IsolatedExecutorHost: Send + Sync {
    fn dispatch(
        &self,
        auth: &HostAuth,
        job: p::WorkerJob,
        permit: p::WorkerPermit,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation>;
    fn probe(
        &self,
        auth: &HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation>;
    fn cancel(
        &self,
        auth: &HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerCancelReceipt>;
    fn record_receipt(
        &self,
        auth: &HostAuth,
        receipt: p::WorkerReceipt,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation>;
}

pub struct WorkerExecutorHost<I = UnavailableWorkerIsolation> {
    journal: WorkerJournal,
    agent_credential_digest: p::SchemaDigest,
    authority_epoch: p::AuthorityEpoch,
    allowed_scopes: BTreeSet<p::Scope>,
    isolation: I,
}

impl WorkerExecutorHost<UnavailableWorkerIsolation> {
    pub fn open(
        journal_path: &Path,
        agent_credential_digest: p::SchemaDigest,
        authority_epoch: p::AuthorityEpoch,
        allowed_scopes: BTreeSet<p::Scope>,
    ) -> p::Result<Self> {
        Self::open_with_isolation(
            journal_path,
            agent_credential_digest,
            authority_epoch,
            allowed_scopes,
            UnavailableWorkerIsolation,
        )
    }
}

impl<I: WorkerIsolation> WorkerExecutorHost<I> {
    pub fn open_with_isolation(
        journal_path: &Path,
        agent_credential_digest: p::SchemaDigest,
        authority_epoch: p::AuthorityEpoch,
        allowed_scopes: BTreeSet<p::Scope>,
        isolation: I,
    ) -> p::Result<Self> {
        p::validate_product_digest(&agent_credential_digest)?;
        if authority_epoch.0 == 0 || allowed_scopes.is_empty() {
            return Err(p::Error(
                "worker host requires a non-zero authority epoch and scope allowlist".into(),
            ));
        }
        for scope in &allowed_scopes {
            validate_workspace_scope(scope)?;
        }
        Ok(Self {
            journal: WorkerJournal::open(journal_path)?,
            agent_credential_digest,
            authority_epoch,
            allowed_scopes,
            isolation,
        })
    }

    fn verify(&self, auth: &HostAuth) -> p::Result<()> {
        if auth.domain() != HostCredentialDomain::AgentExecution
            || auth.proof_ref() != &self.agent_credential_digest
        {
            return Err(p::Error("agent execution authorization is invalid".into()));
        }
        Ok(())
    }

    fn validate_admission(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        now: p::Timestamp,
    ) -> p::Result<()> {
        job.validate()?;
        permit.validate_for(job, now)?;
        if job.authority_epoch != self.authority_epoch {
            return Err(p::Error("worker authority epoch is stale".into()));
        }
        if !self.allowed_scopes.contains(&job.workspace_scope) {
            return Err(p::Error("worker workspace scope is not admitted".into()));
        }
        Ok(())
    }

    fn refresh_isolation(
        &self,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        let current = self.journal.observation(job_id, now)?;
        if matches!(
            current.state,
            p::WorkerState::Pending | p::WorkerState::Running
        ) {
            let job = self.journal.job(job_id)?;
            if let Some(receipt) = self.isolation.observe(&job, now)? {
                receipt.validate_for(&job)?;
                self.journal.record_receipt(receipt, now)?;
            }
        }
        self.journal.observation(job_id, now)
    }
}

impl<I: WorkerIsolation> IsolatedExecutorHost for WorkerExecutorHost<I> {
    fn dispatch(
        &self,
        auth: &HostAuth,
        job: p::WorkerJob,
        permit: p::WorkerPermit,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        self.verify(auth)?;
        self.validate_admission(&job, &permit, now)?;
        let (fresh, observation) = self.journal.admit(job.clone(), permit.clone(), now)?;
        if !fresh {
            return Ok(observation);
        }
        // Make the Running transition durable before asking the platform to
        // create a process. If process creation succeeds and the journal
        // write were attempted afterwards, a storage failure could leave a
        // side effect without a recoverable state boundary.
        let started = self.journal.start(&job.job_id, now)?;
        if let Err(error) = self.isolation.admit_at(&job, &permit, now) {
            let reason = p::canonical_digest(&("worker-admission-unknown", &job.job_id))?.0;
            self.journal.mark_unknown(&job.job_id, reason, now)?;
            return Err(error);
        }
        Ok(started)
    }

    fn probe(
        &self,
        auth: &HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        self.verify(auth)?;
        match self.refresh_isolation(job_id, now) {
            Ok(observation) => Ok(observation),
            Err(error) => {
                let reason = p::canonical_digest(&("worker-observation-unknown", job_id))?.0;
                self.journal.mark_unknown(job_id, reason, now)?;
                Err(error)
            }
        }
    }

    fn cancel(
        &self,
        auth: &HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerCancelReceipt> {
        self.verify(auth)?;
        let job = self.journal.job(job_id)?;
        let observation = match self.refresh_isolation(job_id, now) {
            Ok(observation) => observation,
            Err(error) => {
                let reason = p::canonical_digest(&("worker-observation-unknown", job_id))?.0;
                self.journal.mark_unknown(job_id, reason, now)?;
                return Err(error);
            }
        };
        match observation.state {
            p::WorkerState::Unknown => {
                return Ok(p::WorkerCancelReceipt {
                    schema_version: p::SchemaVersion(1),
                    job_id: job.job_id,
                    plan_digest: job.plan_digest,
                    nonce: job.nonce,
                    outcome: p::WorkerOutcome::Unknown,
                    observed_at: now,
                    receipt_ref: observation.receipt_ref,
                    reason_ref: observation.reason_ref,
                })
            }
            p::WorkerState::CancelRequested => {
                return Ok(p::WorkerCancelReceipt {
                    schema_version: p::SchemaVersion(1),
                    job_id: job.job_id,
                    plan_digest: job.plan_digest,
                    nonce: job.nonce,
                    outcome: p::WorkerOutcome::Unknown,
                    observed_at: now,
                    receipt_ref: observation.receipt_ref,
                    reason_ref: observation
                        .reason_ref
                        .or_else(|| Some("worker-cancel-outcome-unknown".into())),
                })
            }
            p::WorkerState::Completed | p::WorkerState::Failed | p::WorkerState::Cancelled => {
                return Err(p::Error("worker job is already terminal".into()))
            }
            p::WorkerState::Pending | p::WorkerState::Running => {}
        }
        self.journal.request_cancel(job_id, now)?;
        let receipt = match self.isolation.cancel_at(&job, now) {
            Ok(receipt) => receipt,
            Err(error) => {
                let reason = p::canonical_digest(&("worker-cancel-unknown", job_id))?.0;
                self.journal.mark_unknown(job_id, reason, now)?;
                return Err(error);
            }
        };
        receipt.validate_for(&job)?;
        self.journal.record_cancel_receipt(receipt.clone(), now)?;
        Ok(receipt)
    }

    fn record_receipt(
        &self,
        auth: &HostAuth,
        receipt: p::WorkerReceipt,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        self.verify(auth)?;
        if receipt.authority_epoch != self.authority_epoch {
            return Err(p::Error("worker receipt authority epoch is stale".into()));
        }
        self.journal.record_receipt(receipt, now)
    }
}

fn validate_worker_id(value: &str) -> p::Result<()> {
    validate_worker_ref(value, "worker job")
}

fn validate_worker_ref(value: &str, name: &str) -> p::Result<()> {
    p::validate_product_id(value).map_err(|_| p::Error(format!("{name} is invalid")))
}

fn validate_workspace_scope(value: &p::Scope) -> p::Result<()> {
    if value.0.len() > 512
        || !value.0.starts_with("workspace:")
        || value.0.contains(['\\', '\0'])
        || value.0.chars().any(char::is_control)
        || value
            .0
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(p::Error("worker workspace scope is invalid".into()));
    }
    Ok(())
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
    let envelope = WorkerEnvelope {
        schema_version: p::SchemaVersion(1),
        worker_seq,
        digest: WorkerEnvelope::calculate(worker_seq, &previous, &fact)?,
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
