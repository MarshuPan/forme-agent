//! Closed private archive format; no web DTO or successful-recovery assertion.
use forme_protocol as p;
use serde::{Deserialize, Deserializer, Serialize};

use super::invalid;

pub(super) const SCHEMA: p::SchemaVersion = p::SchemaVersion(1);

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArchiveOrigin {
    pub schema_version: p::SchemaVersion,
    pub source_ref: String,
    pub install_ref: String,
    pub installation_digest: p::SchemaDigest,
    pub owner: p::OwnerPrincipalRef,
    pub original_operation_id: String,
    pub original_command_digest: p::SchemaDigest,
    pub original_release_ref: String,
    pub original_release_digest: p::SchemaDigest,
    pub target_release_ref: String,
    pub target_release_digest: p::SchemaDigest,
    pub config_revision: u64,
    pub config_digest: p::SchemaDigest,
    pub archive_digest: p::SchemaDigest,
    pub archive_revision: u64,
    pub recorded_at: p::Timestamp,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SignedArchiveOrigin {
    pub schema_version: p::SchemaVersion,
    pub origin: ArchiveOrigin,
    pub signature: Vec<u8>,
}

impl ArchiveOrigin {
    pub fn digest(&self) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&("forme-legacy-data-archive-origin-v1", self))
    }

    pub fn validate_for(
        &self,
        installation: &p::InstallationManifest,
        binding: &p::LegacyReleaseReviewBinding,
        original: &p::ProductReleaseRequest,
        original_started_at: p::Timestamp,
    ) -> p::Result<()> {
        p::validate_product_id(&self.source_ref)?;
        p::validate_product_digest(&self.archive_digest)?;
        if self.schema_version != SCHEMA
            || installation.target != p::ProductTarget::WindowsX86_64
            || self.install_ref != installation.install_id
            || self.installation_digest != installation.digest()?
            || self.install_ref != binding.install_ref
            || self.owner != binding.owner
            || self.original_operation_id != binding.original_operation_id
            || self.original_command_digest != binding.original_command_digest
            || self.original_release_ref != binding.original_release_ref
            || self.original_release_digest != binding.original_release_digest
            || self.original_release_ref != installation.release_id
            || self.original_release_digest != installation.release_digest
            || self.config_revision != binding.config_revision
            || self.config_digest != binding.config_digest
            || self.archive_revision == 0
            || self.archive_revision >= binding.journal_seq
            || self.recorded_at < 0
            || self.recorded_at >= original_started_at
            || self.install_ref != original.install_ref
            || self.original_operation_id != original.operation_id
            || self.original_command_digest != original.command_digest
            || self.original_release_ref != original.original_release_ref
            || self.original_release_digest != original.original_release_digest
            || self.target_release_ref != original.target_release_ref
            || self.target_release_digest != original.target_release_digest
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArchivedIdentity {
    pub schema_version: p::SchemaVersion,
    pub owner_digest: p::SchemaDigest,
    pub store_identity: p::SchemaDigest,
    pub schema_digest: p::SchemaDigest,
    pub database_digest: p::SchemaDigest,
    pub event_count: u64,
    pub last_stream_seq: u64,
    pub authenticated_content_rows: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DataArchive {
    pub schema_version: p::SchemaVersion,
    pub operation: String,
    pub outcome: String,
    pub prepare_inbox: bool,
    pub release_id: String,
    pub release_digest: p::SchemaDigest,
    pub changes: Vec<String>,
    #[serde(deserialize_with = "required_option")]
    pub failure_stage: Option<String>,
    pub provider_called: bool,
    pub provider_keys_read: bool,
    pub service_actions: u64,
    pub maintenance_commands_submitted: u64,
    pub program_images_changed: bool,
    #[serde(deserialize_with = "required_option")]
    pub database_schema_version: Option<u32>,
    pub runtime_compatibility_verified: bool,
    pub offline_identity: ArchivedIdentity,
    pub product_revision: u64,
    pub config_schema_version: u32,
    pub actual_database_schema_required_during_upgrade: bool,
    pub services: Vec<ArchivedService>,
    pub copied_file_count: u64,
}

// Explicit JSON null is meaningful historical evidence. A missing property is
// not allowed to silently become None through serde's optional-field default.
fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

impl DataArchive {
    pub fn validate_for(&self, origin: &ArchiveOrigin) -> p::Result<()> {
        p::validate_product_id(&self.operation)?;
        let changes = [
            "created-private-inbox-parent",
            "created-private-preparation-directory",
            "published-exact-signed-inbox",
        ];
        if self.schema_version != SCHEMA
            || self.outcome != "inbox_prepared_only"
            || !self.prepare_inbox
            || self.release_id != origin.target_release_ref
            || self.release_digest != origin.target_release_digest
            || self.changes != changes
            || self.failure_stage.is_some()
            || self.provider_called
            || self.provider_keys_read
            || self.service_actions != 0
            || self.maintenance_commands_submitted != 0
            || self.program_images_changed
            || self.database_schema_version == Some(0)
            || self.runtime_compatibility_verified
            || self.offline_identity.schema_version != SCHEMA
            || self.offline_identity.owner_digest
                != p::sha256_content_digest(origin.owner.0.as_bytes())
            || self.product_revision != origin.archive_revision
            || self.config_schema_version == 0
            || !self.actual_database_schema_required_during_upgrade
            || self.copied_file_count == 0
            || self.services.len() != 4
        {
            return Err(invalid());
        }
        for digest in [
            &self.offline_identity.owner_digest,
            &self.offline_identity.store_identity,
            &self.offline_identity.schema_digest,
            &self.offline_identity.database_digest,
        ] {
            p::validate_product_digest(digest)?;
        }
        for name in [
            "forme-host",
            "forme-web",
            "forme-brain",
            "forme-release-updater",
        ] {
            let mut services = self.services.iter().filter(|s| s.name == name);
            let service = services.next().ok_or_else(invalid)?;
            if services.next().is_some() {
                return Err(invalid());
            }
            service.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "PascalCase")]
pub(super) struct ArchivedService {
    pub name: String,
    pub state: String,
    pub start_name: String,
    pub start_mode: String,
    pub process_id: u32,
    pub path_name: String,
}

impl ArchivedService {
    fn validate(&self) -> p::Result<()> {
        let account = match self.name.as_str() {
            "forme-host" | "forme-release-updater" => "LocalSystem",
            "forme-web" => "NT SERVICE\\forme-web",
            "forme-brain" => "NT SERVICE\\forme-brain",
            _ => return Err(invalid()),
        };
        let state_valid = match self.state.as_str() {
            "Stopped" => self.process_id == 0,
            "Running" => self.process_id > 0,
            _ => false,
        };
        if self.start_name != account
            || !matches!(self.start_mode.as_str(), "Manual" | "Auto")
            || !state_valid
            || (matches!(self.name.as_str(), "forme-brain" | "forme-release-updater")
                && self.state != "Stopped")
            || self.path_name.is_empty()
            || self.path_name.len() > 4096
            || self.path_name.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        Ok(())
    }
}
