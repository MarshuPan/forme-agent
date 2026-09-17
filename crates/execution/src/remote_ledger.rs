use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutorLedgerDecision {
    New,
    Pending,
    Completed(Box<p::RemoteDriverReceipt>),
}

pub trait ExecutorReplayLedger: Send + Sync {
    fn begin(
        &self,
        dispatch: &p::RemoteDispatchId,
        lease: &p::RemoteExecutionLeaseRef,
        semantics: &p::SchemaDigest,
    ) -> p::Result<ExecutorLedgerDecision>;
    fn complete(
        &self,
        dispatch: &p::RemoteDispatchId,
        semantics: &p::SchemaDigest,
        receipt: p::RemoteDriverReceipt,
    ) -> p::Result<()>;
    fn receipt_for_lease(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
    ) -> p::Result<Option<p::RemoteDriverReceipt>>;
    fn receipt(
        &self,
        receipt: &p::RemoteDriverReceiptRef,
    ) -> p::Result<Option<p::RemoteDriverReceipt>>;
    fn pending(&self, lease: &p::RemoteExecutionLeaseRef) -> p::Result<bool>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutorLedgerRecord {
    schema_version: p::SchemaVersion,
    dispatch: p::RemoteDispatchId,
    lease: p::RemoteExecutionLeaseRef,
    semantics: p::SchemaDigest,
    receipt: Option<p::RemoteDriverReceipt>,
}

impl ExecutorLedgerRecord {
    fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.dispatch.0.trim().is_empty()
            || self.lease.0.trim().is_empty()
            || self.semantics.0.trim().is_empty()
        {
            return Err(p::Error("executor replay record is incomplete".into()));
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate()?;
            if receipt.dispatch != self.dispatch || receipt.lease != self.lease {
                return Err(p::Error(
                    "executor replay receipt does not match its dispatch".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryExecutorLedger {
    records: Mutex<BTreeMap<p::RemoteDispatchId, ExecutorLedgerRecord>>,
}

impl ExecutorReplayLedger for InMemoryExecutorLedger {
    fn begin(
        &self,
        dispatch: &p::RemoteDispatchId,
        lease: &p::RemoteExecutionLeaseRef,
        semantics: &p::SchemaDigest,
    ) -> p::Result<ExecutorLedgerDecision> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| p::Error("executor replay ledger is unavailable".into()))?;
        match records.get(dispatch) {
            Some(record) if &record.semantics != semantics || &record.lease != lease => Err(
                p::Error("dispatch id was replayed with new semantics".into()),
            ),
            Some(record) => Ok(record
                .receipt
                .clone()
                .map(Box::new)
                .map(ExecutorLedgerDecision::Completed)
                .unwrap_or(ExecutorLedgerDecision::Pending)),
            None => {
                let record = ExecutorLedgerRecord {
                    schema_version: p::M4_SCHEMA_VERSION,
                    dispatch: dispatch.clone(),
                    lease: lease.clone(),
                    semantics: semantics.clone(),
                    receipt: None,
                };
                record.validate()?;
                records.insert(dispatch.clone(), record);
                Ok(ExecutorLedgerDecision::New)
            }
        }
    }

    fn complete(
        &self,
        dispatch: &p::RemoteDispatchId,
        semantics: &p::SchemaDigest,
        receipt: p::RemoteDriverReceipt,
    ) -> p::Result<()> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| p::Error("executor replay ledger is unavailable".into()))?;
        let record = records
            .get_mut(dispatch)
            .ok_or_else(|| p::Error("executor completion has no pending dispatch".into()))?;
        if &record.semantics != semantics {
            return Err(p::Error("executor completion semantics changed".into()));
        }
        if let Some(previous) = &record.receipt {
            return if previous == &receipt {
                Ok(())
            } else {
                Err(p::Error(
                    "executor receipt id has conflicting semantics".into(),
                ))
            };
        }
        receipt.validate()?;
        record.receipt = Some(receipt);
        record.validate()
    }

    fn receipt_for_lease(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
    ) -> p::Result<Option<p::RemoteDriverReceipt>> {
        Ok(self
            .records
            .lock()
            .map_err(|_| p::Error("executor replay ledger is unavailable".into()))?
            .values()
            .find(|record| &record.lease == lease)
            .and_then(|record| record.receipt.clone()))
    }

    fn receipt(
        &self,
        receipt: &p::RemoteDriverReceiptRef,
    ) -> p::Result<Option<p::RemoteDriverReceipt>> {
        Ok(self
            .records
            .lock()
            .map_err(|_| p::Error("executor replay ledger is unavailable".into()))?
            .values()
            .filter_map(|record| record.receipt.as_ref())
            .find(|stored| &stored.receipt == receipt)
            .cloned())
    }

    fn pending(&self, lease: &p::RemoteExecutionLeaseRef) -> p::Result<bool> {
        Ok(self
            .records
            .lock()
            .map_err(|_| p::Error("executor replay ledger is unavailable".into()))?
            .values()
            .any(|record| &record.lease == lease && record.receipt.is_none()))
    }
}

/// Append-only, executor-local replay ledger. Pending is synced before the
/// driver call; completion is a second immutable record. A restart that sees
/// only Pending can report Unknown but can never execute the dispatch again.
pub struct FileExecutorLedger {
    root: PathBuf,
    records: Mutex<BTreeMap<p::RemoteDispatchId, ExecutorLedgerRecord>>,
}

impl FileExecutorLedger {
    pub fn open(root: impl AsRef<Path>) -> p::Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)
            .map_err(|_| p::Error("executor replay ledger directory is unavailable".into()))?;
        let mut records = BTreeMap::<p::RemoteDispatchId, ExecutorLedgerRecord>::new();
        let entries = fs::read_dir(&root)
            .map_err(|_| p::Error("executor replay ledger cannot be listed".into()))?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|_| p::Error("executor replay ledger entry is unreadable".into()))?;
        for entry in entries {
            if !entry
                .file_type()
                .map_err(|_| p::Error("executor replay ledger type is unreadable".into()))?
                .is_file()
                || entry.path().extension().and_then(|value| value.to_str()) != Some("json")
            {
                return Err(p::Error(
                    "executor replay ledger contains an unexpected entry".into(),
                ));
            }
            let bytes = fs::read(entry.path())
                .map_err(|_| p::Error("executor replay record cannot be read".into()))?;
            let record: ExecutorLedgerRecord = serde_json::from_slice(&bytes)
                .map_err(|_| p::Error("executor replay record is malformed".into()))?;
            record.validate()?;
            match records.get(&record.dispatch) {
                Some(previous)
                    if previous.semantics != record.semantics
                        || previous.lease != record.lease
                        || (previous.receipt.is_some()
                            && record.receipt.is_some()
                            && previous.receipt != record.receipt) =>
                {
                    return Err(p::Error(
                        "executor replay ledger has conflicting immutable records".into(),
                    ));
                }
                Some(previous) if previous.receipt.is_some() => {}
                _ => {
                    records.insert(record.dispatch.clone(), record);
                }
            }
        }
        Ok(Self {
            root,
            records: Mutex::new(records),
        })
    }

    fn record_path(&self, record: &ExecutorLedgerRecord, completed: bool) -> p::Result<PathBuf> {
        let identity = p::canonical_digest(&record.dispatch)?;
        let digest = identity
            .0
            .strip_prefix("sha256:")
            .ok_or_else(|| p::Error("executor replay identity digest is invalid".into()))?;
        Ok(self.root.join(format!(
            "{digest}-{}.json",
            if completed { "complete" } else { "pending" }
        )))
    }

    fn persist(&self, record: &ExecutorLedgerRecord) -> p::Result<()> {
        record.validate()?;
        let path = self.record_path(record, record.receipt.is_some())?;
        let bytes = serde_json::to_vec(record)
            .map_err(|_| p::Error("executor replay record cannot be encoded".into()))?;
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(|_| p::Error("executor replay record cannot be persisted".into()))?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let stored = fs::read(&path)
                    .map_err(|_| p::Error("executor replay record cannot be reread".into()))?;
                if stored == bytes {
                    Ok(())
                } else {
                    Err(p::Error(
                        "executor replay record path has conflicting contents".into(),
                    ))
                }
            }
            Err(_) => Err(p::Error("executor replay record cannot be created".into())),
        }
    }
}

impl ExecutorReplayLedger for FileExecutorLedger {
    fn begin(
        &self,
        dispatch: &p::RemoteDispatchId,
        lease: &p::RemoteExecutionLeaseRef,
        semantics: &p::SchemaDigest,
    ) -> p::Result<ExecutorLedgerDecision> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| p::Error("executor replay ledger is unavailable".into()))?;
        match records.get(dispatch) {
            Some(record) if &record.semantics != semantics || &record.lease != lease => Err(
                p::Error("dispatch id was replayed with new semantics".into()),
            ),
            Some(record) => Ok(record
                .receipt
                .clone()
                .map(Box::new)
                .map(ExecutorLedgerDecision::Completed)
                .unwrap_or(ExecutorLedgerDecision::Pending)),
            None => {
                let record = ExecutorLedgerRecord {
                    schema_version: p::M4_SCHEMA_VERSION,
                    dispatch: dispatch.clone(),
                    lease: lease.clone(),
                    semantics: semantics.clone(),
                    receipt: None,
                };
                self.persist(&record)?;
                records.insert(dispatch.clone(), record);
                Ok(ExecutorLedgerDecision::New)
            }
        }
    }

    fn complete(
        &self,
        dispatch: &p::RemoteDispatchId,
        semantics: &p::SchemaDigest,
        receipt: p::RemoteDriverReceipt,
    ) -> p::Result<()> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| p::Error("executor replay ledger is unavailable".into()))?;
        let pending = records
            .get(dispatch)
            .cloned()
            .ok_or_else(|| p::Error("executor completion has no pending dispatch".into()))?;
        if &pending.semantics != semantics {
            return Err(p::Error("executor completion semantics changed".into()));
        }
        if let Some(previous) = pending.receipt {
            return if previous == receipt {
                Ok(())
            } else {
                Err(p::Error(
                    "executor receipt id has conflicting semantics".into(),
                ))
            };
        }
        let completed = ExecutorLedgerRecord {
            receipt: Some(receipt),
            ..pending
        };
        self.persist(&completed)?;
        records.insert(dispatch.clone(), completed);
        Ok(())
    }

    fn receipt_for_lease(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
    ) -> p::Result<Option<p::RemoteDriverReceipt>> {
        Ok(self
            .records
            .lock()
            .map_err(|_| p::Error("executor replay ledger is unavailable".into()))?
            .values()
            .find(|record| &record.lease == lease)
            .and_then(|record| record.receipt.clone()))
    }

    fn receipt(
        &self,
        receipt: &p::RemoteDriverReceiptRef,
    ) -> p::Result<Option<p::RemoteDriverReceipt>> {
        Ok(self
            .records
            .lock()
            .map_err(|_| p::Error("executor replay ledger is unavailable".into()))?
            .values()
            .filter_map(|record| record.receipt.as_ref())
            .find(|stored| &stored.receipt == receipt)
            .cloned())
    }

    fn pending(&self, lease: &p::RemoteExecutionLeaseRef) -> p::Result<bool> {
        Ok(self
            .records
            .lock()
            .map_err(|_| p::Error("executor replay ledger is unavailable".into()))?
            .values()
            .any(|record| &record.lease == lease && record.receipt.is_none()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "forme-m4-ledger-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn pending_survives_restart_and_never_becomes_new() {
        let root = unique_root();
        let dispatch = p::RemoteDispatchId("dispatch:pending".into());
        let lease = p::RemoteExecutionLeaseRef("lease:pending".into());
        let semantics = p::SchemaDigest("sha256:pending".into());
        let ledger = FileExecutorLedger::open(&root).unwrap();
        assert_eq!(
            ledger.begin(&dispatch, &lease, &semantics).unwrap(),
            ExecutorLedgerDecision::New
        );
        drop(ledger);
        let reopened = FileExecutorLedger::open(&root).unwrap();
        assert_eq!(
            reopened.begin(&dispatch, &lease, &semantics).unwrap(),
            ExecutorLedgerDecision::Pending
        );
        fs::remove_dir_all(root).unwrap();
    }
}
