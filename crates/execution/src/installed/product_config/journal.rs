//! Single-writer configuration intent/receipt log. Values and secret refs stay out.
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::storage::{self, invalid};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SnapshotRef {
    pub id: String,
    pub revision: u64,
    pub state_digest: p::SchemaDigest,
    pub core_digest: Option<p::SchemaDigest>,
}

impl SnapshotRef {
    fn validate(&self) -> p::Result<()> {
        if self.id.len() != 32 || !self.id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        p::validate_product_digest(&self.state_digest)?;
        if let Some(digest) = &self.core_digest {
            p::validate_product_digest(digest)?;
        }
        if (self.revision == 0) != self.core_digest.is_none() {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Intent {
    pub operation: String,
    pub digest: p::SchemaDigest,
    pub snapshot: SnapshotRef,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Fact {
    Bound { install_ref: String },
    Imported { intent: Intent },
    Prepared { intent: Intent },
    Committed { operation: String },
    Acknowledged { revision: u64 },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema_version: p::SchemaVersion,
    sequence: u64,
    previous: p::SchemaDigest,
    fact: Fact,
    digest: p::SchemaDigest,
}

#[derive(Clone, Default)]
struct Projection {
    install: Option<String>,
    sequence: u64,
    head: Option<p::SchemaDigest>,
    current: Option<SnapshotRef>,
    pending: Option<Intent>,
    acknowledged: Option<u64>,
    operations: BTreeMap<String, p::SchemaDigest>,
}

impl Projection {
    fn head(&self) -> p::Result<p::SchemaDigest> {
        self.head.clone().map_or_else(
            || p::canonical_digest(&"forme-product-config-journal-v1"),
            Ok,
        )
    }

    fn fold(&mut self, entry: &Envelope) -> p::Result<()> {
        if entry.schema_version != p::SchemaVersion(1)
            || entry.sequence != self.sequence + 1
            || entry.previous != self.head()?
            || entry.digest
                != p::canonical_digest(&(
                    entry.schema_version,
                    entry.sequence,
                    &entry.previous,
                    &entry.fact,
                ))?
        {
            return Err(invalid());
        }
        match &entry.fact {
            Fact::Bound { install_ref } if self.sequence == 0 => {
                p::validate_product_id(install_ref)?;
                self.install = Some(install_ref.clone());
            }
            Fact::Prepared { intent } if self.install.is_some() && self.pending.is_none() => {
                p::validate_product_id(&intent.operation)?;
                p::validate_product_digest(&intent.digest)?;
                intent.snapshot.validate()?;
                let current = self.current.as_ref().map_or(0, |value| value.revision);
                if self.operations.contains_key(&intent.operation)
                    || !(intent.snapshot.revision == current + 1
                        || intent.snapshot.revision == current)
                {
                    return Err(invalid());
                }
                self.pending = Some(intent.clone());
            }
            Fact::Imported { intent }
                if self.sequence == 1 && self.current.is_none() && self.pending.is_none() =>
            {
                p::validate_product_id(&intent.operation)?;
                p::validate_product_digest(&intent.digest)?;
                intent.snapshot.validate()?;
                self.pending = Some(intent.clone());
            }
            Fact::Committed { operation } => {
                let intent = self.pending.take().ok_or_else(invalid)?;
                if intent.operation != *operation {
                    return Err(invalid());
                }
                self.operations.insert(operation.clone(), intent.digest);
                self.current = Some(intent.snapshot);
            }
            Fact::Acknowledged { revision }
                if self.pending.is_none()
                    && self
                        .current
                        .as_ref()
                        .is_some_and(|value| value.revision == *revision)
                    && *revision > 0 =>
            {
                self.acknowledged = Some(*revision)
            }
            _ => return Err(invalid()),
        }
        self.sequence = entry.sequence;
        self.head = Some(entry.digest.clone());
        Ok(())
    }
}

pub(super) struct ConfigJournal {
    file: File,
    projection: Projection,
    poisoned: bool,
}

impl ConfigJournal {
    pub fn open(path: &Path, install: &str) -> p::Result<Self> {
        storage::check(path)?;
        storage::directory(path.parent().ok_or_else(invalid)?)?;
        let mut options = OpenOptions::new();
        options.read(true).append(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let (file, created) = match options.open(path) {
            Ok(file) => (file, true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                options.create_new(false);
                (options.open(path).map_err(|_| invalid())?, false)
            }
            Err(_) => return Err(invalid()),
        };
        file.try_lock()
            .map_err(|_| p::Error("product configuration already has an active writer".into()))?;
        let length = file.metadata().map_err(|_| invalid())?.len();
        if (!created && length == 0) || length > 64 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut journal = Self {
            file,
            projection: Projection::default(),
            poisoned: false,
        };
        let mut reader = BufReader::new(journal.file.try_clone().map_err(|_| invalid())?);
        let mut line = Vec::new();
        loop {
            line.clear();
            let count = reader
                .by_ref()
                .take(8193)
                .read_until(b'\n', &mut line)
                .map_err(|_| invalid())?;
            if count == 0 {
                break;
            }
            if count > 8192 || line.last() != Some(&b'\n') {
                return Err(invalid());
            }
            let envelope = serde_json::from_slice(&line).map_err(|_| invalid())?;
            journal.projection.fold(&envelope)?;
        }
        if length == 0 {
            journal.append(Fact::Bound {
                install_ref: install.to_owned(),
            })?;
            storage::sync_directory(path.parent().ok_or_else(invalid)?)?;
        }
        if journal.projection.install.as_deref() != Some(install) {
            return Err(invalid());
        }
        Ok(journal)
    }

    pub fn current(&self) -> Option<&SnapshotRef> {
        self.projection.current.as_ref()
    }
    pub fn pending(&self) -> Option<&Intent> {
        self.projection.pending.as_ref()
    }
    pub fn acknowledged(&self) -> Option<u64> {
        self.projection.acknowledged
    }

    pub fn existing(&self, operation: &str, digest: &p::SchemaDigest) -> p::Result<bool> {
        if self.poisoned {
            return Err(invalid());
        }
        match self.projection.operations.get(operation) {
            Some(existing) if existing == digest => Ok(true),
            Some(_) => Err(p::Error(
                "configuration operation id was reused with different content".into(),
            )),
            None => Ok(false),
        }
    }

    pub fn prepare(&mut self, intent: Intent) -> p::Result<()> {
        self.append(Fact::Prepared { intent })
    }
    pub fn import(
        &mut self,
        operation: String,
        digest: p::SchemaDigest,
        snapshot: SnapshotRef,
    ) -> p::Result<()> {
        self.append(Fact::Imported {
            intent: Intent {
                operation,
                digest,
                snapshot,
            },
        })
    }
    pub fn commit(&mut self, operation: String) -> p::Result<()> {
        self.append(Fact::Committed { operation })
    }
    pub fn acknowledge(&mut self, revision: u64) -> p::Result<()> {
        self.append(Fact::Acknowledged { revision })
    }

    fn append(&mut self, fact: Fact) -> p::Result<()> {
        if self.poisoned {
            return Err(invalid());
        }
        let sequence = self
            .projection
            .sequence
            .checked_add(1)
            .ok_or_else(invalid)?;
        let previous = self.projection.head()?;
        let digest = p::canonical_digest(&(p::SchemaVersion(1), sequence, &previous, &fact))?;
        let envelope = Envelope {
            schema_version: p::SchemaVersion(1),
            sequence,
            previous,
            fact,
            digest,
        };
        let mut projection = self.projection.clone();
        projection.fold(&envelope)?;
        let mut bytes = serde_json::to_vec(&envelope).map_err(|_| invalid())?;
        bytes.push(b'\n');
        if bytes.len() > 8192
            || self.file.metadata().map_err(|_| invalid())?.len() + bytes.len() as u64
                > 64 * 1024 * 1024
        {
            return Err(invalid());
        }
        if self
            .file
            .seek(SeekFrom::End(0))
            .and_then(|_| self.file.write_all(&bytes))
            .and_then(|_| self.file.sync_all())
            .is_err()
        {
            self.poisoned = true;
            return Err(invalid());
        }
        self.projection = projection;
        Ok(())
    }
}
