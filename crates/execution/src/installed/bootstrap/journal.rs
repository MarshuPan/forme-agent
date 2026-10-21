//! Durable setup facts. This file never contains raw proofs, passwords or keys.
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::{BootstrapError, BootstrapResult};
use p::BootstrapProblemCode as Code;

pub(super) const PROOF_TTL: i64 = 30 * 60 * 1000;
pub(super) const SETUP_TTL: i64 = 15 * 60 * 1000;
const LIMIT: u64 = 8 * 1024 * 1024;
const RECORD_LIMIT: u64 = 16 * 1024;

#[derive(Clone)]
pub(super) struct Reservation {
    pub operation: String,
    pub setup: p::SchemaDigest,
    pub expires: i64,
    pub selection: Option<(String, p::OwnerPrincipalRef)>,
}

#[derive(Clone, Default)]
pub(super) struct State {
    pub binding: Option<(String, p::SchemaDigest)>,
    pub proof: Option<(p::SchemaDigest, i64)>,
    pub rotations: BTreeSet<String>,
    pub reservation: Option<Reservation>,
    pub committed: Option<p::WebClaimCommit>,
    pub last_at: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum Fact {
    Bound {
        install: String,
        key: p::SchemaDigest,
    },
    ProofIssued {
        operation: String,
        digest: p::SchemaDigest,
        at: i64,
        expires: i64,
    },
    Reserved {
        operation: String,
        setup: p::SchemaDigest,
        at: i64,
        expires: i64,
    },
    Selected {
        candidate: String,
        owner: p::OwnerPrincipalRef,
        at: i64,
    },
    Committed {
        receipt: p::WebClaimCommit,
        at: i64,
    },
}

impl State {
    fn fold(&mut self, fact: &Fact) -> BootstrapResult<()> {
        let bad = || BootstrapError(Code::RecoveryRequired);
        if self.binding.is_none() && !matches!(fact, Fact::Bound { .. }) {
            return Err(bad());
        }
        let at = match fact {
            Fact::Bound { install, key } => {
                p::validate_product_id(install).map_err(|_| bad())?;
                p::validate_product_digest(key).map_err(|_| bad())?;
                if self.binding.is_some() {
                    return Err(bad());
                }
                self.binding = Some((install.clone(), key.clone()));
                return Ok(());
            }
            Fact::ProofIssued {
                operation,
                digest,
                at,
                expires,
            } => {
                p::validate_product_digest(digest).map_err(|_| bad())?;
                p::validate_product_id(operation).map_err(|_| bad())?;
                if self.rotations.contains(operation) {
                    return Err(bad());
                }
                if self.committed.is_some()
                    || *expires != at.checked_add(PROOF_TTL).ok_or_else(bad)?
                {
                    return Err(bad());
                }
                self.rotations.insert(operation.clone());
                self.proof = Some((digest.clone(), *expires));
                self.reservation = None;
                *at
            }
            Fact::Reserved {
                operation,
                setup,
                at,
                expires,
            } => {
                p::validate_product_id(operation).map_err(|_| bad())?;
                p::validate_product_digest(setup).map_err(|_| bad())?;
                if self.committed.is_some()
                    || self.reservation.is_some()
                    || self
                        .proof
                        .as_ref()
                        .is_none_or(|(_, deadline)| *at >= *deadline)
                    || *expires != at.checked_add(SETUP_TTL).ok_or_else(bad)?
                {
                    return Err(bad());
                }
                self.reservation = Some(Reservation {
                    operation: operation.clone(),
                    setup: setup.clone(),
                    expires: *expires,
                    selection: None,
                });
                *at
            }
            Fact::Selected {
                candidate,
                owner,
                at,
            } => {
                p::validate_product_id(candidate).map_err(|_| bad())?;
                owner.validate().map_err(|_| bad())?;
                let reservation = self.reservation.as_mut().ok_or_else(bad)?;
                if self.committed.is_some()
                    || reservation.selection.is_some()
                    || *at >= reservation.expires
                {
                    return Err(bad());
                }
                reservation.selection = Some((candidate.clone(), owner.clone()));
                *at
            }
            Fact::Committed { receipt, at } => {
                receipt.validate().map_err(|_| bad())?;
                let reservation = self.reservation.as_ref().ok_or_else(bad)?;
                let (candidate, owner) = reservation.selection.as_ref().ok_or_else(bad)?;
                if self.committed.is_some()
                    || *at >= reservation.expires
                    || receipt.install_ref != self.binding.as_ref().ok_or_else(bad)?.0
                    || receipt.operation_id != reservation.operation
                    || &receipt.candidate_ref != candidate
                    || &receipt.owner != owner
                    || receipt.expires_at != at.checked_add(5 * 60 * 1000).ok_or_else(bad)?
                {
                    return Err(bad());
                }
                self.committed = Some(receipt.clone());
                *at
            }
        };
        if at <= 0 || at < self.last_at {
            return Err(bad());
        }
        self.last_at = at;
        Ok(())
    }
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

fn digest(
    sequence: u64,
    previous: &p::SchemaDigest,
    fact: &Fact,
) -> BootstrapResult<p::SchemaDigest> {
    p::canonical_digest(&(
        "forme-install-claim-journal-v1",
        p::SchemaVersion(1),
        sequence,
        previous,
        fact,
    ))
    .map_err(|_| BootstrapError(Code::Unavailable))
}

pub(super) struct Journal {
    file: File,
    pub state: State,
    sequence: u64,
    head: p::SchemaDigest,
    poisoned: bool,
}

impl Journal {
    pub fn open(path: &Path, install: &str, key: p::SchemaDigest) -> BootstrapResult<Self> {
        super::super::reject_linked_ancestors(path)
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        let existed = path.exists();
        if existed && !path.is_file() {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        if !existed {
            options.create_new(true);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(path)
            .map_err(|_| BootstrapError(Code::Unavailable))?;
        file.try_lock()
            .map_err(|_| BootstrapError(Code::Conflict))?;
        if file
            .metadata()
            .map_err(|_| BootstrapError(Code::Unavailable))?
            .len()
            > LIMIT
        {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        let mut journal = Self {
            file,
            state: State::default(),
            sequence: 0,
            head: p::canonical_digest(&"forme-install-claim-journal-v1")
                .map_err(|_| BootstrapError(Code::Unavailable))?,
            poisoned: false,
        };
        let mut reader = BufReader::new(&journal.file);
        loop {
            let mut line = Vec::new();
            let count = reader
                .by_ref()
                .take(RECORD_LIMIT + 1)
                .read_until(b'\n', &mut line)
                .map_err(|_| BootstrapError(Code::RecoveryRequired))?;
            if count == 0 {
                break;
            }
            if count as u64 > RECORD_LIMIT || line.last() != Some(&b'\n') {
                return Err(BootstrapError(Code::RecoveryRequired));
            }
            let record: Envelope = serde_json::from_slice(&line)
                .map_err(|_| BootstrapError(Code::RecoveryRequired))?;
            if record.schema_version != p::SchemaVersion(1)
                || record.sequence
                    != journal
                        .sequence
                        .checked_add(1)
                        .ok_or(BootstrapError(Code::RecoveryRequired))?
                || record.previous != journal.head
                || record.digest != digest(record.sequence, &record.previous, &record.fact)?
            {
                return Err(BootstrapError(Code::RecoveryRequired));
            }
            journal.state.fold(&record.fact)?;
            journal.sequence = record.sequence;
            journal.head = record.digest;
        }
        if journal.sequence == 0 {
            if existed {
                return Err(BootstrapError(Code::RecoveryRequired));
            }
            journal.append(Fact::Bound {
                install: install.into(),
                key: key.clone(),
            })?;
            #[cfg(unix)]
            File::open(path.parent().ok_or(BootstrapError(Code::Unavailable))?)
                .and_then(|parent| parent.sync_all())
                .map_err(|_| BootstrapError(Code::Unavailable))?;
        }
        if journal.state.binding.as_ref() != Some(&(install.to_owned(), key)) {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        Ok(journal)
    }

    pub fn ensure_healthy(&self) -> BootstrapResult<()> {
        if self.poisoned {
            Err(BootstrapError(Code::RecoveryRequired))
        } else {
            Ok(())
        }
    }

    /// Once a write is uncertain this instance never attempts another append.
    pub fn append(&mut self, fact: Fact) -> BootstrapResult<()> {
        if self.poisoned {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        let mut next = self.state.clone();
        next.fold(&fact)?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(BootstrapError(Code::RecoveryRequired))?;
        let record = Envelope {
            schema_version: p::SchemaVersion(1),
            sequence,
            digest: digest(sequence, &self.head, &fact)?,
            previous: self.head.clone(),
            fact,
        };
        let mut bytes =
            serde_json::to_vec(&record).map_err(|_| BootstrapError(Code::Unavailable))?;
        bytes.push(b'\n');
        let length = self
            .file
            .metadata()
            .map_err(|_| BootstrapError(Code::Unavailable))?
            .len();
        if bytes.len() as u64 > RECORD_LIMIT || length.saturating_add(bytes.len() as u64) > LIMIT {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        self.poisoned = true;
        self.file
            .seek(SeekFrom::End(0))
            .and_then(|_| self.file.write_all(&bytes))
            .and_then(|_| self.file.sync_all())
            .map_err(|_| BootstrapError(Code::RecoveryRequired))?;
        self.state = next;
        self.sequence = sequence;
        self.head = record.digest;
        self.poisoned = false;
        Ok(())
    }
}
