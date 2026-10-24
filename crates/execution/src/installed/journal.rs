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
mod projection;
mod release_images;
#[cfg(test)]
mod tests;
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_release: Option<JournalReleaseRef>,
    },
    ActiveReleaseBound {
        release: JournalReleaseRef,
    },
    ActiveReleaseChanged {
        operation_id: String,
        from: JournalReleaseRef,
        to: JournalReleaseRef,
    },
    ReleaseImagesClaimed {
        operation_id: String,
        command_digest: p::SchemaDigest,
        prepared_digest: p::SchemaDigest,
        at: p::Timestamp,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalReleaseRef {
    release_id: String,
    release_digest: p::SchemaDigest,
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

#[derive(Deserialize)]
struct PersistedFact<'a> {
    #[serde(borrow)]
    fact: &'a serde_json::value::RawValue,
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
    release_images_claimed: bool,
}

#[derive(Clone, Default)]
struct Projection {
    install_id: Option<String>,
    installation_verification: Option<(p::SchemaDigest, p::SchemaDigest)>,
    active_release: Option<JournalReleaseRef>,
    sequence: u64,
    head: Option<p::SchemaDigest>,
    operations: BTreeMap<String, OperationState>,
    commands: BTreeMap<String, (p::SchemaDigest, String)>,
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
            // Hash the original JSON fact. Re-serializing today's typed DTO
            // would insert defaults absent from older, immutable records.
            // Deserialize the closed DTO separately so duplicate/unknown fields
            // still fail instead of being normalized away by Value.
            let raw: PersistedFact<'_> =
                serde_json::from_slice(&bytes).map_err(|_| invalid_journal())?;
            let calculated = p::canonical_digest(&(
                envelope.schema_version,
                envelope.product_seq,
                &envelope.previous,
                raw.fact,
            ))?;
            projection.fold_with_digest(&envelope, calculated)?;
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

    pub fn active_release(&self) -> p::Result<Option<(String, p::SchemaDigest)>> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        Ok(writer
            .projection
            .active_release
            .as_ref()
            .map(|release| (release.release_id.clone(), release.release_digest.clone())))
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
