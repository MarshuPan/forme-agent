//! Owner program registration is configuration, never permission to execute.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MANAGED_MCP_PROGRAM_PROFILE: &str = "windows-appcontainer-mcp-stdio-v1";
pub const MANAGED_MCP_PROGRAM_REQUEST_MAX_BYTES: usize = 8192;
pub const MANAGED_MCP_PROGRAM_MAX_ENTRIES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ManagedMcpProgramAction {
    Register {
        source_ref: String,
        source_digest: SchemaDigest,
    },
    Revoke {
        program_ref: String,
        registration_digest: SchemaDigest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedMcpProgramCommand {
    pub schema_version: SchemaVersion,
    pub installation_ref: String,
    pub owner_ref: OwnerPrincipalRef,
    pub authority_epoch: AuthorityEpoch,
    pub operation_id: String,
    pub expected_revision: u64,
    pub requested_deadline: Timestamp,
    pub action: ManagedMcpProgramAction,
}
impl ManagedMcpProgramCommand {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        for id in [
            &self.installation_ref,
            &self.owner_ref.0,
            &self.operation_id,
        ] {
            validate_product_id(id)?;
        }
        if self.authority_epoch.0 == 0 || self.requested_deadline <= 0 {
            return Err(invalid());
        }
        let (reference, digest) = match &self.action {
            ManagedMcpProgramAction::Register {
                source_ref,
                source_digest,
            } => (source_ref, source_digest),
            ManagedMcpProgramAction::Revoke {
                program_ref,
                registration_digest,
            } => (program_ref, registration_digest),
        };
        validate_product_id(reference)?;
        sha256_digest_bytes(digest)?;
        if serde_json::to_vec(self).map_err(|_| invalid())?.len()
            > MANAGED_MCP_PROGRAM_REQUEST_MAX_BYTES
        {
            return Err(invalid());
        }
        Ok(())
    }
    /// New mutations require a live five-minute window; historical same-key
    /// operation queries do not renew it or execute the command again.
    pub fn validate_at(&self, now: Timestamp) -> Result<()> {
        self.validate()?;
        if now <= 0
            || self.requested_deadline <= now
            || self.requested_deadline.saturating_sub(now) > 300_000
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme.managed-mcp.program.command.v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedMcpProgramChoice {
    pub schema_version: SchemaVersion,
    pub source_ref: String,
    pub source_digest: SchemaDigest,
    pub scope: Scope,
    pub sandbox_profile_ref: String,
}
impl ManagedMcpProgramChoice {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.source_ref)?;
        sha256_digest_bytes(&self.source_digest)?;
        binding(&self.scope, &self.sandbox_profile_ref)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedMcpProgramView {
    pub schema_version: SchemaVersion,
    pub program_ref: String,
    pub registration_digest: SchemaDigest,
    pub source_ref: String,
    pub scope: Scope,
    pub sandbox_profile_ref: String,
    pub revoked: bool,
}
impl ManagedMcpProgramView {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.source_ref)?;
        sha256_digest_bytes(&self.registration_digest)?;
        if self.program_ref != format!("mcp-program:{}", &self.registration_digest.0[7..]) {
            return Err(invalid());
        }
        binding(&self.scope, &self.sandbox_profile_ref)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedMcpProgramCatalog {
    pub schema_version: SchemaVersion,
    pub installation_ref: String,
    pub owner_ref: OwnerPrincipalRef,
    pub authority_epoch: AuthorityEpoch,
    pub revision: u64,
    pub choices: Vec<ManagedMcpProgramChoice>,
    pub programs: Vec<ManagedMcpProgramView>,
}
impl ManagedMcpProgramCatalog {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.installation_ref)?;
        validate_product_id(&self.owner_ref.0)?;
        if self.authority_epoch.0 == 0
            || self.revision == 0
            || self.choices.len() > MANAGED_MCP_PROGRAM_MAX_ENTRIES
            || self.programs.len() > MANAGED_MCP_PROGRAM_MAX_ENTRIES
        {
            return Err(invalid());
        }
        let mut sources = BTreeSet::new();
        for choice in &self.choices {
            choice.validate()?;
            if !sources.insert(&choice.source_ref) {
                return Err(invalid());
            }
        }
        let mut programs = BTreeSet::new();
        for program in &self.programs {
            program.validate()?;
            if !programs.insert(&program.program_ref) {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedMcpProgramOperation {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub revision: u64,
    pub program: ManagedMcpProgramView,
}
impl ManagedMcpProgramOperation {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.operation_id)?;
        sha256_digest_bytes(&self.command_digest)?;
        if self.revision < 2 {
            return Err(invalid());
        }
        self.program.validate()
    }
}

fn binding(scope: &Scope, profile: &str) -> Result<()> {
    validate_product_id(&scope.0)?;
    if profile != MANAGED_MCP_PROGRAM_PROFILE {
        return Err(invalid());
    }
    Ok(())
}
fn version(version: SchemaVersion) -> Result<()> {
    if version != SchemaVersion(1) {
        return Err(invalid());
    }
    Ok(())
}
fn invalid() -> Error {
    Error("managed MCP program request or view is invalid".into())
}
