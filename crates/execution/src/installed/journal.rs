use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::reject_linked_ancestors;
#[path = "network_journal.rs"]
mod network_journal;
mod verification;

const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 64 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
enum JournalFact {
    InstallationBound {
        install_id: String,
    },
    InstallationVerified {
        manifest_digest: p::SchemaDigest,
        release_digest: p::SchemaDigest,
    },
    Planned {
        command: Box<p::MaintenanceCommand>,
        references: p::MaintenanceReferences,
        actor: p::MaintenanceActorClass,
        at: p::Timestamp,
        #[serde(default)]
        network_contract: Option<u32>,
    },
    StageIntent {
        operation_id: String,
        stage: p::MaintenanceStage,
        actor: p::MaintenanceActorClass,
        at: p::Timestamp,
    },
    StageReceipt {
        operation_id: String,
        stage: p::MaintenanceStage,
        receipts: Vec<p::SchemaDigest>,
        at: p::Timestamp,
        #[serde(default)]
        network_result: Option<p::ProductNetworkResult>,
    },
    NetworkProgress {
        operation_id: String,
        progress: p::ProductNetworkProgress,
        at: p::Timestamp,
    },
    RecoveryRequired {
        operation_id: String,
        issue: p::SchemaDigest,
        at: p::Timestamp,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalEnvelope {
    schema_version: p::SchemaVersion,
    product_seq: u64,
    previous: p::SchemaDigest,
    fact: JournalFact,
    digest: p::SchemaDigest,
}

impl JournalEnvelope {
    fn calculate(
        seq: u64,
        previous: &p::SchemaDigest,
        fact: &JournalFact,
    ) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&(p::SchemaVersion(1), seq, previous, fact))
    }
}

#[derive(Clone)]
struct OperationState {
    command: p::MaintenanceCommand,
    view: p::MaintenanceOperation,
    network_contract: bool,
    network_progress: Option<p::ProductNetworkProgress>,
}

#[derive(Clone, Default)]
struct Projection {
    install_id: Option<String>,
    installation_verification: Option<(p::SchemaDigest, p::SchemaDigest)>,
    sequence: u64,
    head: Option<p::SchemaDigest>,
    operations: BTreeMap<String, OperationState>,
    commands: BTreeMap<String, (p::SchemaDigest, String)>,
}

impl Projection {
    fn head(&self) -> p::Result<p::SchemaDigest> {
        self.head
            .clone()
            .map_or_else(|| p::canonical_digest(&"forme-product-journal-v1"), Ok)
    }
    fn fold(&mut self, envelope: &JournalEnvelope) -> p::Result<()> {
        if envelope.schema_version != p::SchemaVersion(1)
            || envelope.product_seq != self.sequence.checked_add(1).ok_or_else(invalid_journal)?
            || envelope.previous != self.head()?
            || envelope.digest
                != JournalEnvelope::calculate(
                    envelope.product_seq,
                    &envelope.previous,
                    &envelope.fact,
                )?
        {
            return Err(invalid_journal());
        }
        match &envelope.fact {
            JournalFact::InstallationBound { install_id } => {
                p::validate_product_id(install_id)?;
                if self.install_id.is_some() || self.sequence != 0 {
                    return Err(invalid_journal());
                }
                self.install_id = Some(install_id.clone());
            }
            JournalFact::InstallationVerified {
                manifest_digest,
                release_digest,
            } => {
                p::validate_product_digest(manifest_digest)?;
                p::validate_product_digest(release_digest)?;
                if self.install_id.is_none()
                    || self.sequence != 1
                    || self.installation_verification.is_some()
                {
                    return Err(invalid_journal());
                }
                self.installation_verification =
                    Some((manifest_digest.clone(), release_digest.clone()));
            }
            JournalFact::Planned {
                command,
                references,
                actor,
                at,
                network_contract,
            } => {
                command.validate()?;
                references.validate(&command.kind)?;
                if self.install_id.is_none()
                    || command.expected_revision != self.sequence
                    || *at >= command.requested_deadline
                    || self.operations.contains_key(&command.operation_id)
                    || self.commands.contains_key(&command.idempotency_key)
                    || self.has_unfinished_operation()
                    || *actor == p::MaintenanceActorClass::Setup && !command.kind.setup_allowed()
                {
                    return Err(invalid_journal());
                }
                let digest = command.bound_digest(references)?;
                self.commands.insert(
                    command.idempotency_key.clone(),
                    (digest.clone(), command.operation_id.clone()),
                );
                self.operations.insert(
                    command.operation_id.clone(),
                    OperationState {
                        command: command.as_ref().clone(),
                        view: p::MaintenanceOperation {
                            schema_version: p::SchemaVersion(1),
                            operation_id: command.operation_id.clone(),
                            command_digest: digest,
                            revision: envelope.product_seq,
                            stage: p::MaintenanceStage::Planned,
                            pending_stage: None,
                            original_release_ref: references.original_release_ref.clone(),
                            target_release_ref: references.target_release_ref.clone(),
                            data_candidate_ref: references.data_candidate_ref.clone(),
                            backup_ref: references.backup_ref.clone(),
                            receipts: Vec::new(),
                            failure_ref: None,
                            network_result: None,
                        },
                        network_contract: *network_contract == Some(1),
                        network_progress: None,
                    },
                );
            }
            JournalFact::StageIntent {
                operation_id,
                stage,
                actor,
                at,
            } => {
                let state = self
                    .operations
                    .get_mut(operation_id)
                    .ok_or_else(invalid_journal)?;
                if state.view.pending_stage.is_some()
                    || !state
                        .view
                        .stage
                        .permits(*stage, state.command.kind.is_lifecycle_transaction())
                    || *stage == p::MaintenanceStage::RecoveryRequired
                    || *at >= state.command.requested_deadline
                    || *actor == p::MaintenanceActorClass::Setup
                        && !state.command.kind.setup_allowed()
                {
                    return Err(invalid_journal());
                }
                state.view.pending_stage = Some(*stage);
                state.view.revision = envelope.product_seq;
            }
            JournalFact::StageReceipt {
                operation_id,
                stage,
                receipts,
                network_result,
                ..
            } => {
                let state = self
                    .operations
                    .get_mut(operation_id)
                    .ok_or_else(invalid_journal)?;
                if matches!(
                    state.view.stage,
                    p::MaintenanceStage::RecoveryRequired | p::MaintenanceStage::Committed
                ) || state.view.pending_stage != Some(*stage)
                    || receipts.is_empty()
                    || receipts.len() > 32
                {
                    return Err(invalid_journal());
                }
                for receipt in receipts {
                    p::validate_product_digest(receipt)?;
                }
                network_journal::validate_stage_receipt(state, *stage, network_result.as_ref())?;
                state.view.stage = *stage;
                state.view.pending_stage = None;
                state
                    .view
                    .receipts
                    .extend(receipts.iter().map(|r| r.0.clone()));
                if network_result.is_some() {
                    state.view.network_result = network_result.clone();
                }
                state.view.revision = envelope.product_seq;
            }
            JournalFact::NetworkProgress {
                operation_id,
                progress,
                ..
            } => {
                let state = self
                    .operations
                    .get_mut(operation_id)
                    .ok_or_else(invalid_journal)?;
                network_journal::apply_progress(state, progress, envelope.product_seq)?;
            }
            JournalFact::RecoveryRequired {
                operation_id,
                issue,
                ..
            } => {
                p::validate_product_digest(issue)?;
                let state = self
                    .operations
                    .get_mut(operation_id)
                    .ok_or_else(invalid_journal)?;
                if matches!(
                    state.view.stage,
                    p::MaintenanceStage::Committed | p::MaintenanceStage::RecoveryRequired
                ) {
                    return Err(invalid_journal());
                }
                state.view.stage = p::MaintenanceStage::RecoveryRequired;
                // Retain pending stage: it may already have changed the host.
                state.view.failure_ref = Some(issue.0.clone());
                state.view.revision = envelope.product_seq;
            }
        }
        self.sequence = envelope.product_seq;
        self.head = Some(envelope.digest.clone());
        Ok(())
    }
    fn has_unfinished_operation(&self) -> bool {
        self.operations
            .values()
            .any(|op| op.view.stage != p::MaintenanceStage::Committed)
    }
}

struct Writer {
    file: File,
    projection: Projection,
    poisoned: bool,
}

/// One OS-locked writer. Operations only record host facts; no driver or brain access.
/// A torn record or ambiguous durable write blocks further mutations.
pub struct ProductJournal {
    writer: Mutex<Writer>,
}

impl ProductJournal {
    pub fn open(path: &Path, install_id: &str) -> p::Result<Self> {
        p::validate_product_id(install_id)?;
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
            .map_err(|_| p::Error("installation management already has an active writer".into()))?;
        if file.metadata().map_err(|_| storage_error())?.len() > MAX_JOURNAL_BYTES {
            return Err(p::Error("product journal requires offline archival".into()));
        }
        let mut projection = Projection::default();
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
            let envelope: JournalEnvelope =
                serde_json::from_slice(&bytes).map_err(|_| invalid_journal())?;
            projection.fold(&envelope)?;
        }
        if projection
            .install_id
            .as_deref()
            .is_some_and(|id| id != install_id)
        {
            return Err(p::Error(
                "product journal belongs to a different installation".into(),
            ));
        }
        let journal = Self {
            writer: Mutex::new(Writer {
                file,
                projection,
                poisoned: false,
            }),
        };
        if journal.revision()? == 0 {
            journal.append(
                0,
                JournalFact::InstallationBound {
                    install_id: install_id.into(),
                },
            )?;
            #[cfg(unix)]
            File::open(path.parent().ok_or_else(storage_error)?)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| storage_error())?;
        }
        Ok(journal)
    }

    pub fn revision(&self) -> p::Result<u64> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        Ok(writer.projection.sequence)
    }

    pub fn operation(&self, id: &str) -> p::Result<p::MaintenanceOperation> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        writer
            .projection
            .operations
            .get(id)
            .map(|op| op.view.clone())
            .ok_or_else(|| p::Error("maintenance operation does not exist".into()))
    }

    pub fn command_kind(&self, id: &str) -> p::Result<p::MaintenanceKind> {
        self.command(id).map(|command| command.kind)
    }

    pub fn command(&self, id: &str) -> p::Result<p::MaintenanceCommand> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        writer
            .projection
            .operations
            .get(id)
            .map(|op| op.command.clone())
            .ok_or_else(|| p::Error("maintenance operation does not exist".into()))
    }
    pub fn operations(&self, after: u64) -> p::Result<Vec<p::MaintenanceOperation>> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let mut views: Vec<_> = writer
            .projection
            .operations
            .values()
            .filter(|op| op.view.revision > after)
            .map(|op| op.view.clone())
            .collect();
        views.sort_by_key(|op| op.revision);
        Ok(views)
    }
    pub fn plan(
        &self,
        command: p::MaintenanceCommand,
        actor: p::MaintenanceActorClass,
        references: p::MaintenanceReferences,
        now: p::Timestamp,
    ) -> p::Result<p::MaintenanceOperation> {
        command.validate()?;
        references.validate(&command.kind)?;
        if actor == p::MaintenanceActorClass::Setup && !command.kind.setup_allowed() {
            return Err(p::Error(
                "setup scope cannot access this maintenance command".into(),
            ));
        }
        let digest = command.bound_digest(&references)?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        if let Some((previous, id)) = writer.projection.commands.get(&command.idempotency_key) {
            if *previous != digest {
                return Err(p::Error(
                    "maintenance idempotency key conflicts with its original command".into(),
                ));
            }
            let existing = writer
                .projection
                .operations
                .get(id)
                .ok_or_else(invalid_journal)?;
            if existing.view.original_release_ref != references.original_release_ref
                || existing.view.target_release_ref != references.target_release_ref
                || existing.view.backup_ref != references.backup_ref
                || existing.view.data_candidate_ref != references.data_candidate_ref
            {
                return Err(p::Error(
                    "maintenance idempotency references conflict".into(),
                ));
            }
            return Ok(existing.view.clone());
        }
        let expected = command.expected_revision;
        let id = command.operation_id.clone();
        let network_contract = matches!(command.kind, p::MaintenanceKind::ConfigureLan { .. });
        write_fact(
            &mut writer,
            expected,
            JournalFact::Planned {
                command: Box::new(command),
                references,
                actor,
                at: now,
                network_contract: network_contract.then_some(1),
            },
        )?;
        Ok(writer
            .projection
            .operations
            .get(&id)
            .ok_or_else(invalid_journal)?
            .view
            .clone())
    }
    /// Persist before invoking the bounded native operation. After a crash, probe
    /// this same operation; never execute another copy just to obtain a receipt.
    pub fn begin_stage(
        &self,
        id: &str,
        expected: u64,
        stage: p::MaintenanceStage,
        actor: p::MaintenanceActorClass,
        now: p::Timestamp,
    ) -> p::Result<()> {
        self.append(
            expected,
            JournalFact::StageIntent {
                operation_id: id.into(),
                stage,
                actor,
                at: now,
            },
        )
    }
    pub fn finish_stage(
        &self,
        id: &str,
        expected: u64,
        stage: p::MaintenanceStage,
        receipts: Vec<p::SchemaDigest>,
        now: p::Timestamp,
    ) -> p::Result<()> {
        let network_result = self
            .writer
            .lock()
            .map_err(|_| storage_error())?
            .projection
            .operations
            .get(id)
            .and_then(|operation| operation.view.network_result.clone());
        self.append(
            expected,
            JournalFact::StageReceipt {
                operation_id: id.into(),
                stage,
                receipts,
                at: now,
                network_result,
            },
        )
    }

    pub fn require_recovery(
        &self,
        id: &str,
        expected: u64,
        issue: p::SchemaDigest,
        now: p::Timestamp,
    ) -> p::Result<()> {
        self.append(
            expected,
            JournalFact::RecoveryRequired {
                operation_id: id.into(),
                issue,
                at: now,
            },
        )
    }
    fn append(&self, expected: u64, fact: JournalFact) -> p::Result<()> {
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        write_fact(&mut writer, expected, fact)
    }
}

fn write_fact(writer: &mut Writer, expected: u64, fact: JournalFact) -> p::Result<()> {
    if writer.poisoned {
        return Err(storage_error());
    }
    if writer.projection.sequence != expected {
        return Err(p::Error("stale product revision".into()));
    }
    let product_seq = expected.checked_add(1).ok_or_else(invalid_journal)?;
    let previous = writer.projection.head()?;
    let envelope = JournalEnvelope {
        schema_version: p::SchemaVersion(1),
        product_seq,
        digest: JournalEnvelope::calculate(product_seq, &previous, &fact)?,
        previous,
        fact,
    };
    // Validate on a copy before either disk or in-memory state changes.
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
    p::Error("product journal requires recovery; no operation was replayed".into())
}
fn storage_error() -> p::Error {
    p::Error("product journal storage is unavailable; durable outcome must be checked".into())
}
