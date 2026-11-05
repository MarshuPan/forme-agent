//! Durable setup facts. This file never contains raw proofs, passwords or keys.
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::{BootstrapError, BootstrapResult};
use p::BootstrapProblemCode as Code;

mod legacy;
use legacy::LegacyClaims;

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
    legacy: LegacyClaims,
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
    CandidateInventoryObserved {
        operation: String,
        catalog: p::BootstrapCandidateCatalogV2,
        at: i64,
    },
    CandidateSelectedV2 {
        selection: p::BootstrapCandidateSelectionV2,
        candidate: p::BootstrapCandidateViewV2,
        at: i64,
    },
    ClaimDataPrepared {
        prepared: p::BootstrapClaimDataPreparedV2,
        at: i64,
    },
    ClaimDataIntentV2 {
        intent: p::BootstrapDataIntentV2,
        at: i64,
    },
    ClaimDataBindingConfirmedV2 {
        confirmation: p::BootstrapDataBindingConfirmationV2,
        at: i64,
    },
    ClaimCommittedV2 {
        commit: p::BootstrapClaimCommitV2,
        at: i64,
    },
    ClaimActivatedV2 {
        activation: p::BootstrapClaimActivationV2,
        at: i64,
    },
    ClaimAbortedV2 {
        operation: String,
        candidate_ref: String,
        candidate_digest: p::SchemaDigest,
        issue_ref: String,
        at: i64,
    },
}

impl State {
    pub fn candidate_catalog(&self, operation: &str) -> Option<&p::BootstrapCandidateCatalogV2> {
        self.legacy.catalog(operation)
    }

    pub fn next_verification_epoch(&self) -> BootstrapResult<u64> {
        self.legacy.next_epoch()
    }

    pub fn legacy_intent(&self, operation: &str) -> Option<&p::BootstrapDataIntentV2> {
        self.legacy.intent(operation)
    }

    pub fn is_claimed(&self) -> bool {
        self.committed.is_some() || self.legacy.is_committed()
    }

    pub fn has_legacy_selection(&self) -> bool {
        self.legacy.has_selection()
    }

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
                if self.is_claimed()
                    || self.has_legacy_selection()
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
                if self.is_claimed()
                    || self.has_legacy_selection()
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
                if self.is_claimed() || self.has_legacy_selection() {
                    return Err(bad());
                }
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
            Fact::CandidateInventoryObserved { at, .. }
            | Fact::CandidateSelectedV2 { at, .. }
            | Fact::ClaimDataPrepared { at, .. }
            | Fact::ClaimDataIntentV2 { at, .. }
            | Fact::ClaimDataBindingConfirmedV2 { at, .. }
            | Fact::ClaimCommittedV2 { at, .. }
            | Fact::ClaimActivatedV2 { at, .. }
            | Fact::ClaimAbortedV2 { at, .. } => {
                if self.committed.is_some() {
                    return Err(bad());
                }
                let install = &self.binding.as_ref().ok_or_else(bad)?.0;
                let reservation = self.reservation.as_ref().ok_or_else(bad)?;
                self.legacy.fold(fact, install, reservation, *at)?;
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
    schema_version: p::SchemaVersion,
    sequence: u64,
    previous: &p::SchemaDigest,
    fact: &Fact,
) -> BootstrapResult<p::SchemaDigest> {
    p::canonical_digest(&(
        if schema_version == p::SchemaVersion(1) {
            "forme-install-claim-journal-v1"
        } else {
            "forme-install-claim-journal-v2"
        },
        schema_version,
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
    schema_version: p::SchemaVersion,
    recovered_claims: BTreeSet<String>,
}

impl Journal {
    pub fn require_recovery(&mut self, operation: &str) {
        self.recovered_claims.insert(operation.to_owned());
    }
    pub fn open(path: &Path, install: &str, key: p::SchemaDigest) -> BootstrapResult<Self> {
        Self::open_version(path, install, key, p::SchemaVersion(2))
    }

    #[cfg(test)]
    pub(super) fn open_v1(
        path: &Path,
        install: &str,
        key: p::SchemaDigest,
    ) -> BootstrapResult<Self> {
        Self::open_version(path, install, key, p::SchemaVersion(1))
    }

    fn open_version(
        path: &Path,
        install: &str,
        key: p::SchemaDigest,
        maximum_version: p::SchemaVersion,
    ) -> BootstrapResult<Self> {
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
            schema_version: p::SchemaVersion(1),
            recovered_claims: BTreeSet::new(),
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
            if !(1..=maximum_version.0).contains(&record.schema_version.0)
                || record.schema_version.0 < journal.schema_version.0
                || record.schema_version.0 < record.fact.minimum_version().0
                || record.sequence
                    != journal
                        .sequence
                        .checked_add(1)
                        .ok_or(BootstrapError(Code::RecoveryRequired))?
                || record.previous != journal.head
                || record.digest
                    != digest(
                        record.schema_version,
                        record.sequence,
                        &record.previous,
                        &record.fact,
                    )?
            {
                return Err(BootstrapError(Code::RecoveryRequired));
            }
            journal.state.fold(&record.fact)?;
            journal.sequence = record.sequence;
            journal.head = record.digest;
            journal.schema_version = record.schema_version;
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
        journal.recovered_claims = journal.state.legacy.pending_operations();
        Ok(journal)
    }

    pub fn claim_view_v2(&self, operation: &str) -> BootstrapResult<p::BootstrapClaimViewV2> {
        self.ensure_healthy()?;
        let install = &self
            .state
            .binding
            .as_ref()
            .ok_or(BootstrapError(Code::RecoveryRequired))?
            .0;
        self.state.legacy.view(
            install,
            operation,
            self.recovered_claims.contains(operation),
        )
    }

    #[cfg(test)]
    pub(super) fn snapshot_for_test(&mut self) -> Vec<u8> {
        let position = self.file.stream_position().unwrap();
        self.file.rewind().unwrap();
        let mut bytes = Vec::new();
        self.file.read_to_end(&mut bytes).unwrap();
        self.file.seek(SeekFrom::Start(position)).unwrap();
        bytes
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
        if fact
            .legacy_operation()
            .is_some_and(|operation| self.recovered_claims.contains(operation))
        {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        let mut next = self.state.clone();
        next.fold(&fact)?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(BootstrapError(Code::RecoveryRequired))?;
        let schema_version = p::SchemaVersion(self.schema_version.0.max(fact.minimum_version().0));
        let record = Envelope {
            schema_version,
            sequence,
            digest: digest(schema_version, sequence, &self.head, &fact)?,
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
        self.schema_version = schema_version;
        self.poisoned = false;
        Ok(())
    }
}

impl Fact {
    fn legacy_operation(&self) -> Option<&str> {
        match self {
            Self::CandidateInventoryObserved { operation, .. }
            | Self::ClaimAbortedV2 { operation, .. } => Some(operation),
            Self::CandidateSelectedV2 { selection, .. } => Some(&selection.session.operation_id),
            Self::ClaimDataPrepared { prepared, .. } => Some(&prepared.operation_id),
            Self::ClaimDataIntentV2 { intent, .. } => Some(&intent.request.session.operation_id),
            Self::ClaimDataBindingConfirmedV2 { confirmation, .. } => {
                Some(&confirmation.operation_id)
            }
            Self::ClaimCommittedV2 { commit, .. } => Some(&commit.receipt.operation_id),
            Self::ClaimActivatedV2 { activation, .. } => Some(&activation.operation_id),
            _ => None,
        }
    }

    fn minimum_version(&self) -> p::SchemaVersion {
        match self {
            Self::Bound { .. }
            | Self::ProofIssued { .. }
            | Self::Reserved { .. }
            | Self::Selected { .. }
            | Self::Committed { .. } => p::SchemaVersion(1),
            _ => p::SchemaVersion(2),
        }
    }
}
