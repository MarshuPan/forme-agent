//! Fixed Host sources and Windows handle custody. No process creation.
use super::{invalid, Identity};
use crate::installed::{
    file_digest, management_repair::storage::ReadCustody, reject_linked_ancestors,
    sandbox_lease::SandboxFileIdentity, windows_appcontainer_acl as acl, WorkerLaunchSpec,
};
use forme_protocol as p;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf, Prefix},
};

const MAX_PROGRAM_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PROGRAM_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

/// Provisioned only by the local installation/management domain. Never decode
/// this object from an owner Web command, model output, or execution intent.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedMcpProgramSource {
    pub schema_version: p::SchemaVersion,
    pub source_ref: String,
    pub scope: p::Scope,
    pub sandbox_profile_ref: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub expected_program_digest: p::SchemaDigest,
}
impl ManagedMcpProgramSource {
    pub(super) fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.sandbox_profile_ref != p::MANAGED_MCP_PROGRAM_PROFILE
            || self.args.len() > 64
            || self
                .args
                .iter()
                .any(|arg| arg.len() > 4096 || arg.chars().any(char::is_control))
            || self.args.iter().map(String::len).sum::<usize>() > 16_384
            || !local_disk_path(&self.program)
            || self
                .program
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
            || !self
                .program
                .extension()
                .is_some_and(|v| v.eq_ignore_ascii_case("exe"))
        {
            return Err(invalid());
        }
        p::validate_product_id(&self.source_ref)?;
        p::validate_product_id(&self.scope.0)?;
        p::sha256_digest_bytes(&self.expected_program_digest)?;
        let name = self
            .program
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(invalid)?;
        if [
            "cmd.exe",
            "powershell.exe",
            "pwsh.exe",
            "bash.exe",
            "sh.exe",
            "zsh.exe",
        ]
        .iter()
        .any(|shell| name.eq_ignore_ascii_case(shell))
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn launch(&self) -> p::Result<WorkerLaunchSpec> {
        WorkerLaunchSpec::new(
            self.sandbox_profile_ref.clone(),
            self.program.clone(),
            self.args.clone(),
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceConfig {
    schema_version: p::SchemaVersion,
    sources: Vec<ManagedMcpProgramSource>,
}

pub(super) struct Sources {
    root: PathBuf,
    entries: BTreeMap<String, ManagedMcpProgramSource>,
    // Retain both the fixed configuration and all ancestors for this lifetime.
    _configuration: ReadCustody,
}
impl Sources {
    pub(super) fn load(root: &Path, forbidden: &[PathBuf], scopes: &[p::Scope]) -> p::Result<Self> {
        if !local_disk_path(root) {
            return Err(invalid());
        }
        reject_linked_ancestors(root)?;
        if !root.is_absolute() || !root.is_dir() || scopes.is_empty() {
            return Err(invalid());
        }
        let root = std::fs::canonicalize(root).map_err(|_| invalid())?;
        for path in forbidden {
            reject_linked_ancestors(path)?;
            let path = if path.exists() {
                std::fs::canonicalize(path).map_err(|_| invalid())?
            } else {
                std::fs::canonicalize(path.parent().ok_or_else(invalid)?)
                    .map_err(|_| invalid())?
                    .join(path.file_name().ok_or_else(invalid)?)
            };
            if root.starts_with(&path) || path.starts_with(&root) {
                return Err(invalid());
            }
        }
        let mut configuration = ReadCustody::default();
        let config: SourceConfig = configuration.json(&root.join("sources.json"), 256 * 1024)?;
        if config.schema_version != p::SchemaVersion(1)
            || config.sources.len() > p::MANAGED_MCP_PROGRAM_MAX_ENTRIES
        {
            return Err(invalid());
        }
        let mut entries = BTreeMap::new();
        for source in config.sources {
            source.validate()?;
            if !scopes.contains(&source.scope)
                || entries.insert(source.source_ref.clone(), source).is_some()
            {
                return Err(invalid());
            }
        }
        Ok(Self {
            root,
            entries,
            _configuration: configuration,
        })
    }
    pub(super) fn observe(&self, id: &str) -> p::Result<PinnedSource> {
        let source = self.entries.get(id).ok_or_else(invalid)?;
        source.validate()?;
        let mut custody = ReadCustody::default();
        let bytes = custody.pin(&source.program, MAX_PROGRAM_BYTES)?;
        let actual = std::fs::canonicalize(&source.program).map_err(|_| invalid())?;
        if !actual.starts_with(&self.root) || actual == self.root || bytes == 0 {
            return Err(invalid());
        }
        let identity = acl::file_identity(&source.program)?;
        let digest = file_digest(&source.program)?;
        if digest != source.expected_program_digest {
            return Err(invalid());
        }
        let captured = CapturedSource {
            schema_version: p::SchemaVersion(1),
            source: source.clone(),
            identity,
            bytes,
            network: false,
            resource_refs: Vec::new(),
            credential_refs: Vec::new(),
        };
        captured.validate()?;
        Ok(PinnedSource {
            captured,
            _custody: custody,
        })
    }
    pub(super) fn choices(&self) -> p::Result<Vec<p::ManagedMcpProgramChoice>> {
        let mut identities = Vec::new();
        let mut bytes = 0_u64;
        let mut choices = Vec::new();
        for id in self.entries.keys() {
            let pinned = self.observe(id)?;
            bytes = bytes
                .checked_add(pinned.captured.bytes)
                .ok_or_else(invalid)?;
            if bytes > MAX_PROGRAM_TOTAL_BYTES || identities.contains(&pinned.captured.identity) {
                return Err(invalid());
            }
            identities.push(pinned.captured.identity.clone());
            choices.push(pinned.captured.choice()?);
        }
        Ok(choices)
    }
}

fn local_disk_path(path: &Path) -> bool {
    path.is_absolute()
        && matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CapturedSource {
    schema_version: p::SchemaVersion,
    pub(super) source: ManagedMcpProgramSource,
    identity: SandboxFileIdentity,
    bytes: u64,
    network: bool,
    resource_refs: Vec<String>,
    credential_refs: Vec<String>,
}
impl CapturedSource {
    pub(super) fn validate(&self) -> p::Result<()> {
        self.source.validate()?;
        SandboxFileIdentity::new(self.identity.volume_serial_number, self.identity.file_id)?;
        if self.schema_version != p::SchemaVersion(1)
            || self.bytes == 0
            || self.bytes > MAX_PROGRAM_BYTES
            || self.network
            || !self.resource_refs.is_empty()
            || !self.credential_refs.is_empty()
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn digest(&self) -> p::Result<p::SchemaDigest> {
        self.validate()?;
        p::canonical_digest(&("forme.managed-mcp.program.source.v1", self))
    }
    pub(super) fn choice(&self) -> p::Result<p::ManagedMcpProgramChoice> {
        Ok(p::ManagedMcpProgramChoice {
            schema_version: p::SchemaVersion(1),
            source_ref: self.source.source_ref.clone(),
            source_digest: self.digest()?,
            scope: self.source.scope.clone(),
            sandbox_profile_ref: self.source.sandbox_profile_ref.clone(),
        })
    }
    pub(super) fn registration(
        &self,
        identity: &Identity,
        operation: &str,
    ) -> p::Result<p::ManagedMcpProgramView> {
        self.validate()?;
        let digest = p::canonical_digest(&(
            "forme.managed-mcp.program.registration.v1",
            &identity.installation_ref,
            &identity.owner_ref,
            identity.authority_epoch,
            operation,
            self,
        ))?;
        Ok(p::ManagedMcpProgramView {
            schema_version: p::SchemaVersion(1),
            program_ref: format!("mcp-program:{}", &digest.0[7..]),
            registration_digest: digest,
            source_ref: self.source.source_ref.clone(),
            scope: self.source.scope.clone(),
            sandbox_profile_ref: self.source.sandbox_profile_ref.clone(),
            revoked: false,
        })
    }
}
pub(super) struct PinnedSource {
    pub(super) captured: CapturedSource,
    _custody: ReadCustody,
}
