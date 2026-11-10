//! Durable Host registration. The caller must authenticate the owner separately.
use forme_protocol as p;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

mod journal;
mod journal_binding;
mod managed;

pub(crate) use managed::admission::DiscoveryAdmission;
pub use managed::InstalledManagedMcpPrograms;
mod sources;
use journal::{Fact, Writer};
pub use sources::ManagedMcpProgramSource;
use sources::{PinnedSource, Sources};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    schema_version: p::SchemaVersion,
    installation_ref: String,
    owner_ref: p::OwnerPrincipalRef,
    authority_epoch: p::AuthorityEpoch,
}
impl Identity {
    fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1) || self.authority_epoch.0 == 0 {
            return Err(invalid());
        }
        p::validate_product_id(&self.installation_ref)?;
        p::validate_product_id(&self.owner_ref.0)
    }
    fn matches(&self, command: &p::ManagedMcpProgramCommand) -> bool {
        self.installation_ref == command.installation_ref
            && self.owner_ref == command.owner_ref
            && self.authority_epoch == command.authority_epoch
    }
}

pub struct ManagedMcpProgramRegistry {
    writer: Mutex<Writer>,
    sources: Sources,
}
impl ManagedMcpProgramRegistry {
    pub fn identity_matches(
        &self,
        installation: &str,
        owner: &p::OwnerPrincipalRef,
        epoch: p::AuthorityEpoch,
    ) -> p::Result<()> {
        let writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        let identity = &writer.state.identity;
        if identity.installation_ref != installation
            || &identity.owner_ref != owner
            || identity.authority_epoch != epoch
        {
            return Err(invalid());
        }
        Ok(())
    }
    /// Open only after the installation has a committed owner. This neither
    /// installs programs nor starts discovery; protection of the source root
    /// must also be established by the installed platform composition.
    pub fn open(
        journal: &Path,
        installation_ref: String,
        owner_ref: p::OwnerPrincipalRef,
        authority_epoch: p::AuthorityEpoch,
        source_root: &Path,
        scopes: &[p::Scope],
        protected_paths: &[PathBuf],
    ) -> p::Result<Self> {
        let identity = Identity {
            schema_version: p::SchemaVersion(1),
            installation_ref,
            owner_ref,
            authority_epoch,
        };
        identity.validate()?;
        let mut forbidden = protected_paths.to_vec();
        forbidden.push(journal.to_path_buf());
        let sources = Sources::load(source_root, &forbidden, scopes)?;
        Ok(Self {
            writer: Mutex::new(Writer::open(journal, identity)?),
            sources,
        })
    }
    pub fn catalog(&self) -> p::Result<p::ManagedMcpProgramCatalog> {
        let writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        let state = &writer.state;
        let catalog = p::ManagedMcpProgramCatalog {
            schema_version: p::SchemaVersion(1),
            installation_ref: state.identity.installation_ref.clone(),
            owner_ref: state.identity.owner_ref.clone(),
            authority_epoch: state.identity.authority_epoch,
            revision: state.revision,
            choices: self.sources.choices()?,
            programs: state
                .programs
                .values()
                .map(|program| program.view.clone())
                .collect(),
        };
        catalog.validate()?;
        Ok(catalog)
    }
    /// A same-key replay returns only its original durable result, including
    /// after revocation or expiry. It cannot revive the registered program.
    pub fn apply(
        &self,
        command: p::ManagedMcpProgramCommand,
        now: p::Timestamp,
    ) -> p::Result<p::ManagedMcpProgramOperation> {
        self.apply_in_window(command, CommandWindow::new(now), true)
    }
    pub(crate) fn apply_in_window(
        &self,
        command: p::ManagedMcpProgramCommand,
        window: CommandWindow,
        allow_mutation: bool,
    ) -> p::Result<p::ManagedMcpProgramOperation> {
        command.validate()?;
        let mut writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        if !writer.state.identity.matches(&command) {
            return Err(invalid());
        }
        if let Some(operation) = writer.state.operations.get(&command.operation_id) {
            if operation.command_digest != command.digest()? {
                return Err(invalid());
            }
            return Ok(operation.clone());
        }
        if !allow_mutation {
            return Err(invalid());
        }
        window.validate(&command)?;
        if command.expected_revision != writer.state.revision {
            return Err(invalid());
        }
        let operation_id = command.operation_id.clone();
        match &command.action {
            p::ManagedMcpProgramAction::Register {
                source_ref,
                source_digest,
            } => {
                if writer.state.programs.len() >= p::MANAGED_MCP_PROGRAM_MAX_ENTRIES {
                    return Err(invalid());
                }
                self.sources.choices()?;
                let pinned = self.sources.observe(source_ref)?;
                if pinned.captured.digest()? != *source_digest {
                    return Err(invalid());
                }
                // Keep custody until the registration fsync has succeeded.
                let at = window.validate(&command)?;
                writer.append_checked(
                    Fact::Registered {
                        command: command.clone(),
                        source: Box::new(pinned.captured.clone()),
                        at,
                    },
                    || window.validate(&command).map(|_| ()),
                )?;
                drop(pinned);
            }
            p::ManagedMcpProgramAction::Revoke { .. } => {
                let at = window.validate(&command)?;
                writer.append_checked(
                    Fact::Revoked {
                        command: command.clone(),
                        at,
                    },
                    || window.validate(&command).map(|_| ()),
                )?
            }
        }
        writer
            .state
            .operations
            .get(&operation_id)
            .cloned()
            .ok_or_else(invalid)
    }
    pub fn operation(&self, operation_id: &str) -> p::Result<p::ManagedMcpProgramOperation> {
        p::validate_product_id(operation_id)?;
        let writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        writer
            .state
            .operations
            .get(operation_id)
            .cloned()
            .ok_or_else(invalid)
    }
    /// Resolve only the registered executable, never caller-supplied argv. A
    /// lease holds the real file and ancestors against replacement on Windows.
    pub fn acquire(
        &self,
        owner: &p::OwnerPrincipalRef,
        epoch: p::AuthorityEpoch,
        program_ref: &str,
        registration_digest: &p::SchemaDigest,
        scope: &p::Scope,
    ) -> p::Result<ManagedMcpProgramLease> {
        let writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        let record = writer.state.programs.get(program_ref).ok_or_else(invalid)?;
        if &writer.state.identity.owner_ref != owner
            || writer.state.identity.authority_epoch != epoch
            || record.view.revoked
            || &record.view.registration_digest != registration_digest
            || &record.view.scope != scope
        {
            return Err(invalid());
        }
        let source = self.sources.observe(&record.view.source_ref)?;
        if source.captured != record.source {
            return Err(invalid());
        }
        Ok(ManagedMcpProgramLease {
            view: record.view.clone(),
            source,
        })
    }
    /// Linearize each subsequent RPC egress with revocation. The callback may
    /// perform deadline-supervised I/O; it must not acquire configuration/worker
    /// locks or wait for a server response while this exclusion is held.
    pub fn with_active<T>(
        &self,
        lease: &ManagedMcpProgramLease,
        action: impl FnOnce() -> p::Result<T>,
    ) -> p::Result<T> {
        let writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        let record = writer
            .state
            .programs
            .get(&lease.view.program_ref)
            .ok_or_else(invalid)?;
        if record.view != lease.view
            || record.source != lease.source.captured
            || record.view.revoked
        {
            return Err(invalid());
        }
        action()
    }
}

pub(crate) struct CommandWindow {
    received_at: p::Timestamp,
    started: std::time::Instant,
}
impl CommandWindow {
    pub(crate) fn new(received_at: p::Timestamp) -> Self {
        Self {
            received_at,
            started: std::time::Instant::now(),
        }
    }
    fn validate(&self, command: &p::ManagedMcpProgramCommand) -> p::Result<p::Timestamp> {
        command.validate_at(self.received_at)?;
        let elapsed = i64::try_from(self.started.elapsed().as_millis()).map_err(|_| invalid())?;
        let now = self.received_at.checked_add(elapsed).ok_or_else(invalid)?;
        command.validate_at(now)?;
        Ok(now)
    }
}

pub struct ManagedMcpProgramLease {
    view: p::ManagedMcpProgramView,
    source: PinnedSource,
}
impl ManagedMcpProgramLease {
    pub fn view(&self) -> &p::ManagedMcpProgramView {
        &self.view
    }
    pub fn launch(&self) -> p::Result<super::WorkerLaunchSpec> {
        self.source.captured.source.launch()
    }
}

fn invalid() -> p::Error {
    p::Error("managed MCP program registration is unavailable, stale or requires recovery".into())
}

#[cfg(test)]
mod tests;
