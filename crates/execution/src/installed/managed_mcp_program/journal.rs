//! Bounded, OS-locked Host facts with semantic replay and no tail repair.
use super::{invalid, sources::CapturedSource, Identity};
use forme_protocol as p;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

const MAX_RECORD: usize = 64 * 1024;
const MAX_JOURNAL: u64 = 64 * 1024 * 1024;
const MAX_OPERATIONS: usize = 4096;
const DOMAIN: &str = "forme.managed-mcp.program.journal.v1";

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Fact {
    Bound {
        identity: Identity,
    },
    Registered {
        command: p::ManagedMcpProgramCommand,
        source: Box<CapturedSource>,
        at: p::Timestamp,
    },
    Revoked {
        command: p::ManagedMcpProgramCommand,
        at: p::Timestamp,
    },
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
impl Envelope {
    fn digest(&self) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&(
            DOMAIN,
            self.schema_version,
            self.sequence,
            &self.previous,
            &self.fact,
        ))
    }
}
#[derive(Clone)]
pub(super) struct Program {
    pub(super) view: p::ManagedMcpProgramView,
    pub(super) source: CapturedSource,
}
#[derive(Clone)]
pub(super) struct State {
    pub(super) identity: Identity,
    pub(super) revision: u64,
    pub(super) programs: BTreeMap<String, Program>,
    pub(super) operations: BTreeMap<String, p::ManagedMcpProgramOperation>,
}
impl State {
    fn apply(&mut self, fact: &Fact, sequence: u64) -> p::Result<()> {
        if sequence != self.revision.checked_add(1).ok_or_else(invalid)? {
            return Err(invalid());
        }
        let (command, at) = match fact {
            Fact::Bound { identity } => {
                identity.validate()?;
                if self.revision != 0 || identity != &self.identity {
                    return Err(invalid());
                }
                self.revision = sequence;
                return Ok(());
            }
            Fact::Registered { command, at, .. } | Fact::Revoked { command, at } => (command, *at),
        };
        command.validate_at(at)?;
        if self.revision == 0
            || !self.identity.matches(command)
            || command.expected_revision != self.revision
            || self.operations.contains_key(&command.operation_id)
            || self.operations.len() >= MAX_OPERATIONS
        {
            return Err(invalid());
        }
        let view = match (fact, &command.action) {
            (
                Fact::Registered { source, .. },
                p::ManagedMcpProgramAction::Register {
                    source_ref,
                    source_digest,
                },
            ) => {
                source.validate()?;
                if &source.source.source_ref != source_ref
                    || source.digest()? != *source_digest
                    || self.programs.len() >= p::MANAGED_MCP_PROGRAM_MAX_ENTRIES
                {
                    return Err(invalid());
                }
                let view = source.registration(&self.identity, &command.operation_id)?;
                view.validate()?;
                if self.programs.contains_key(&view.program_ref) {
                    return Err(invalid());
                }
                self.programs.insert(
                    view.program_ref.clone(),
                    Program {
                        view: view.clone(),
                        source: source.as_ref().clone(),
                    },
                );
                view
            }
            (
                Fact::Revoked { .. },
                p::ManagedMcpProgramAction::Revoke {
                    program_ref,
                    registration_digest,
                },
            ) => {
                let record = self.programs.get_mut(program_ref).ok_or_else(invalid)?;
                if record.view.revoked || &record.view.registration_digest != registration_digest {
                    return Err(invalid());
                }
                record.view.revoked = true;
                record.view.clone()
            }
            _ => return Err(invalid()),
        };
        let operation = p::ManagedMcpProgramOperation {
            schema_version: p::SchemaVersion(1),
            operation_id: command.operation_id.clone(),
            command_digest: command.digest()?,
            revision: sequence,
            program: view,
        };
        operation.validate()?;
        self.operations
            .insert(command.operation_id.clone(), operation);
        self.revision = sequence;
        Ok(())
    }
}
pub(super) struct Writer {
    file: File,
    _binding: crate::installed::management_repair::storage::ReadCustody,
    head: p::SchemaDigest,
    poisoned: bool,
    pub(super) state: State,
}
impl Writer {
    pub(super) fn open(path: &Path, identity: Identity) -> p::Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        crate::installed::reject_linked_ancestors(path)?;
        super::journal_binding::preflight(path)?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        let (file, fresh) = match options.create_new(true).open(path) {
            Ok(file) => (file, true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (
                options
                    .create_new(false)
                    .open(path)
                    .map_err(|_| invalid())?,
                false,
            ),
            Err(_) => return Err(invalid()),
        };
        file.try_lock().map_err(|_| invalid())?;
        if !file.metadata().map_err(|_| invalid())?.is_file()
            || file.metadata().map_err(|_| invalid())?.len() > MAX_JOURNAL
            || (!fresh && file.metadata().map_err(|_| invalid())?.len() == 0)
            || crate::installed::windows_appcontainer_acl::inspect_file(&file)?.1 != 1
        {
            return Err(invalid());
        }
        let mut state = State {
            identity: identity.clone(),
            revision: 0,
            programs: BTreeMap::new(),
            operations: BTreeMap::new(),
        };
        let binding = if fresh {
            Default::default()
        } else {
            super::journal_binding::pin(path, &file, &identity, false)?
        };
        let mut head = p::canonical_digest(&DOMAIN)?;
        let mut reader = BufReader::new(&file);
        loop {
            let mut bytes = Vec::new();
            (&mut reader)
                .take((MAX_RECORD + 1) as u64)
                .read_until(b'\n', &mut bytes)
                .map_err(|_| invalid())?;
            if bytes.is_empty() {
                break;
            }
            if bytes.len() > MAX_RECORD || bytes.last() != Some(&b'\n') {
                return Err(invalid());
            }
            let envelope: Envelope = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            if envelope.schema_version != p::SchemaVersion(1)
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
            _binding: binding,
            state,
            head,
            poisoned: false,
        };
        if writer.state.revision == 0 {
            writer.append(Fact::Bound {
                identity: identity.clone(),
            })?;
        }
        if fresh {
            writer._binding = super::journal_binding::pin(path, &writer.file, &identity, true)?;
        }
        Ok(writer)
    }
    pub(super) fn healthy(&self) -> p::Result<()> {
        if self.poisoned {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn append(&mut self, fact: Fact) -> p::Result<()> {
        self.append_checked(fact, || Ok(()))
    }
    pub(super) fn append_checked(
        &mut self,
        fact: Fact,
        before_write: impl FnOnce() -> p::Result<()>,
    ) -> p::Result<()> {
        self.healthy()?;
        let sequence = self.state.revision.checked_add(1).ok_or_else(invalid)?;
        let mut next = self.state.clone();
        next.apply(&fact, sequence)?;
        let mut envelope = Envelope {
            schema_version: p::SchemaVersion(1),
            sequence,
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
        before_write()?;
        self.poisoned = true;
        self.file
            .seek(SeekFrom::End(0))
            .and_then(|_| self.file.write_all(&bytes))
            .and_then(|_| self.file.sync_all())
            .map_err(|_| invalid())?;
        self.head = envelope.digest;
        self.state = next;
        self.poisoned = false;
        Ok(())
    }
}
