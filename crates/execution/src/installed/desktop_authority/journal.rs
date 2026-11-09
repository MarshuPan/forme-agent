//! OS-locked, bounded append-only Host journal. It never repairs partial writes.
use super::*;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};

const MAX_RECORD: usize = 128 * 1024;
const MAX_JOURNAL: u64 = 64 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum DesktopFact {
    Bound {
        identity: AuthorityIdentity,
    },
    Pending {
        request: p::DesktopPendingAction,
        at: p::Timestamp,
    },
    Consent {
        request: p::DesktopConsentRequest,
        selected: ObservedDesktopSession,
        consent_ref: String,
        at: p::Timestamp,
    },
    Revoke {
        request: p::DesktopRevokeRequest,
        at: p::Timestamp,
    },
    Grant {
        operation_id: String,
        request_digest: p::SchemaDigest,
        consent_ref: String,
        binding: p::DesktopSessionBinding,
    },
    Consume {
        operation_id: String,
        at: p::Timestamp,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema_version: p::SchemaVersion,
    sequence: u64,
    previous: p::SchemaDigest,
    fact: DesktopFact,
    digest: p::SchemaDigest,
}
impl Envelope {
    fn digest(&self) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&(
            "forme.desktop.authority.journal.v1",
            self.schema_version,
            self.sequence,
            &self.previous,
            &self.fact,
        ))
    }
}
#[derive(Clone)]
pub(super) struct ConsentRecord {
    pub(super) request: p::DesktopConsentRequest,
    pub(super) selected: ObservedDesktopSession,
    pub(super) revoked: bool,
}
#[derive(Clone)]
pub(super) struct OperationRecord {
    pub(super) digest: p::SchemaDigest,
    pub(super) consent_ref: String,
    pub(super) revision: u64,
}
#[derive(Clone)]
pub(super) struct GrantRecord {
    pub(super) operation_id: String,
    pub(super) request_digest: p::SchemaDigest,
    pub(super) consent_ref: String,
    pub(super) binding: p::DesktopSessionBinding,
    pub(super) consumed: bool,
    pub(super) revision: u64,
}
#[derive(Clone)]
pub(super) struct DesktopState {
    pub(super) identity: AuthorityIdentity,
    pub(super) revision: u64,
    pub(super) pending: BTreeMap<String, p::DesktopPendingAction>,
    pub(super) consents: BTreeMap<String, ConsentRecord>,
    pub(super) operations: BTreeMap<String, OperationRecord>,
    pub(super) grants: BTreeMap<String, GrantRecord>,
}
pub(super) struct DesktopWriter {
    file: File,
    head: p::SchemaDigest,
    poisoned: bool,
    pub(super) state: DesktopState,
    pub(super) catalog: Option<CachedCatalog>,
}
impl DesktopWriter {
    pub(super) fn open(path: &Path, identity: AuthorityIdentity) -> p::Result<Self> {
        super::super::reject_linked_ancestors(path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(|_| invalid())?;
        file.try_lock().map_err(|_| invalid())?;
        if !file.metadata().map_err(|_| invalid())?.is_file()
            || file.metadata().map_err(|_| invalid())?.len() > MAX_JOURNAL
        {
            return Err(invalid());
        }
        let mut state = DesktopState {
            identity: identity.clone(),
            revision: 0,
            pending: BTreeMap::new(),
            consents: BTreeMap::new(),
            operations: BTreeMap::new(),
            grants: BTreeMap::new(),
        };
        let mut head = p::canonical_digest(&"forme.desktop.authority.journal.v1")?;
        let mut reader = BufReader::new(&file);
        loop {
            let mut bytes = Vec::new();
            let length = reader
                .by_ref()
                .take((MAX_RECORD + 1) as u64)
                .read_until(b'\n', &mut bytes)
                .map_err(|_| invalid())?;
            if length == 0 {
                break;
            }
            if length > MAX_RECORD || bytes.last() != Some(&b'\n') {
                return Err(invalid());
            }
            let envelope: Envelope = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            if envelope.schema_version != p::SchemaVersion(1)
                || envelope.sequence != state.revision.checked_add(1).ok_or_else(invalid)?
                || envelope.previous != head
                || envelope.digest != envelope.digest()?
            {
                return Err(invalid());
            }
            state.apply(&envelope.fact, envelope.sequence)?;
            head = envelope.digest;
        }
        let mut writer = Self {
            file,
            head,
            poisoned: false,
            state,
            catalog: None,
        };
        if writer.state.revision == 0 {
            writer.append(DesktopFact::Bound { identity })?;
        }
        Ok(writer)
    }
    pub(super) fn healthy(&self) -> p::Result<()> {
        if self.poisoned {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn append(&mut self, fact: DesktopFact) -> p::Result<()> {
        self.healthy()?;
        let revision = self.state.revision.checked_add(1).ok_or_else(invalid)?;
        let mut next = self.state.clone();
        next.apply(&fact, revision)?;
        let mut envelope = Envelope {
            schema_version: p::SchemaVersion(1),
            sequence: revision,
            previous: self.head.clone(),
            fact,
            digest: self.head.clone(),
        };
        envelope.digest = envelope.digest()?;
        let mut bytes = serde_json::to_vec(&envelope).map_err(|_| invalid())?;
        bytes.push(b'\n');
        if bytes.len() > MAX_RECORD
            || self
                .file
                .metadata()
                .map_err(|_| invalid())?
                .len()
                .checked_add(bytes.len() as u64)
                .ok_or_else(invalid)?
                > MAX_JOURNAL
        {
            return Err(invalid());
        }
        self.poisoned = true;
        self.file.seek(SeekFrom::End(0)).map_err(|_| invalid())?;
        self.file.write_all(&bytes).map_err(|_| invalid())?;
        self.file.sync_all().map_err(|_| invalid())?;
        self.state = next;
        self.head = envelope.digest;
        self.poisoned = false;
        Ok(())
    }
}
