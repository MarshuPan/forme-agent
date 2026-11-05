//! Private, signed management repair records; never product API input.
use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::invalid;

pub(super) const SCHEMA: p::SchemaVersion = p::SchemaVersion(1);
pub(super) const IMAGE_LIMIT: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RepairImage {
    Host,
    Web,
    Brain,
    Cli,
    Hostctl,
    Executord,
}

impl RepairImage {
    pub const ALL: [Self; 6] = [
        Self::Host,
        Self::Web,
        Self::Brain,
        Self::Cli,
        Self::Hostctl,
        Self::Executord,
    ];

    pub fn binary(self) -> &'static str {
        match self {
            Self::Host => "forme-hostd.exe",
            Self::Web => "forme-webd.exe",
            Self::Brain => "forme-gatewayd.exe",
            Self::Cli => "forme-cli.exe",
            Self::Hostctl => "forme-hostctl.exe",
            Self::Executord => "forme-executord.exe",
        }
    }

    pub fn may_change(self) -> bool {
        matches!(self, Self::Host | Self::Web | Self::Hostctl)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RepairImageBinding {
    pub schema_version: p::SchemaVersion,
    pub image: RepairImage,
    pub source_size: u64,
    pub source_digest: p::SchemaDigest,
    pub target_size: u64,
    pub target_digest: p::SchemaDigest,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RepairPlan {
    pub schema_version: p::SchemaVersion,
    pub repair_id: String,
    pub distribution_digest: p::SchemaDigest,
    pub install_ref: String,
    pub installation_digest: p::SchemaDigest,
    pub owner: p::OwnerPrincipalRef,
    pub original_operation_id: String,
    pub original_command_digest: p::SchemaDigest,
    pub original_release_ref: String,
    pub original_release_digest: p::SchemaDigest,
    pub journal_seq: u64,
    pub journal_head: p::SchemaDigest,
    pub config_files_digest: p::SchemaDigest,
    pub service_configuration_digest: p::SchemaDigest,
    pub images: Vec<RepairImageBinding>,
    pub journal_read_max: u32,
    pub updater_version: u32,
    pub authorized_at: p::Timestamp,
    pub authorization_deadline: p::Timestamp,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RepairReceipt {
    pub schema_version: p::SchemaVersion,
    pub repair_id: String,
    pub plan_digest: p::SchemaDigest,
    pub distribution_digest: p::SchemaDigest,
    pub install_ref: String,
    pub installation_digest: p::SchemaDigest,
    pub owner: p::OwnerPrincipalRef,
    pub original_operation_id: String,
    pub original_command_digest: p::SchemaDigest,
    pub original_release_ref: String,
    pub original_release_digest: p::SchemaDigest,
    pub journal_seq: u64,
    pub journal_head: p::SchemaDigest,
    pub config_files_digest: p::SchemaDigest,
    pub service_configuration_digest: p::SchemaDigest,
    pub installed_images_digest: p::SchemaDigest,
    pub journal_read_max: u32,
    pub updater_version: u32,
    pub committed_at: p::Timestamp,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SignedPlan {
    pub schema_version: p::SchemaVersion,
    pub plan: RepairPlan,
    pub signature: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SignedReceipt {
    pub schema_version: p::SchemaVersion,
    pub receipt: RepairReceipt,
    pub signature: Vec<u8>,
}

impl RepairPlan {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != SCHEMA
            || self.images.len() != RepairImage::ALL.len()
            || self.journal_seq == 0
            || self.journal_read_max != 3
            || self.updater_version != 3
            || self.authorized_at < 0
            || self.authorization_deadline <= self.authorized_at
            || self
                .authorization_deadline
                .saturating_sub(self.authorized_at)
                > 900_000
        {
            return Err(invalid());
        }
        for reference in [
            &self.repair_id,
            &self.install_ref,
            &self.original_operation_id,
        ] {
            p::validate_product_id(reference)?;
        }
        self.owner.validate()?;
        p::validate_release_id(&self.original_release_ref)?;
        for digest in [
            &self.distribution_digest,
            &self.installation_digest,
            &self.original_command_digest,
            &self.original_release_digest,
            &self.journal_head,
            &self.config_files_digest,
            &self.service_configuration_digest,
        ] {
            p::validate_product_digest(digest)?;
        }
        for (entry, image) in self.images.iter().zip(RepairImage::ALL) {
            if entry.schema_version != SCHEMA
                || entry.image != image
                || entry.source_size == 0
                || entry.source_size > IMAGE_LIMIT
                || entry.target_size == 0
                || entry.target_size > IMAGE_LIMIT
                || (!image.may_change()
                    && (entry.source_size != entry.target_size
                        || entry.source_digest != entry.target_digest))
            {
                return Err(invalid());
            }
            p::validate_product_digest(&entry.source_digest)?;
            p::validate_product_digest(&entry.target_digest)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> p::Result<p::SchemaDigest> {
        self.validate()?;
        p::canonical_digest(&("forme-management-reader-repair-plan-v1", self))
    }

    pub fn images_digest(&self) -> p::Result<p::SchemaDigest> {
        self.validate()?;
        let images: Vec<_> = self
            .images
            .iter()
            .map(|entry| (entry.image, entry.target_size, &entry.target_digest))
            .collect();
        p::canonical_digest(&("forme-management-reader-repair-images-v1", images))
    }

    pub fn validate_for(
        &self,
        installation: &p::InstallationManifest,
        binding: &p::LegacyReleaseReviewBinding,
    ) -> p::Result<()> {
        self.validate()?;
        binding.validate()?;
        if installation.target != p::ProductTarget::WindowsX86_64
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
            || self.journal_seq != binding.journal_seq
            || self.journal_head != binding.journal_head
        {
            return Err(invalid());
        }
        Ok(())
    }
}

impl RepairReceipt {
    pub fn validate_for(&self, plan: &RepairPlan, now: p::Timestamp) -> p::Result<()> {
        if self.schema_version != SCHEMA
            || self.repair_id != plan.repair_id
            || self.plan_digest != plan.digest()?
            || self.distribution_digest != plan.distribution_digest
            || self.install_ref != plan.install_ref
            || self.installation_digest != plan.installation_digest
            || self.owner != plan.owner
            || self.original_operation_id != plan.original_operation_id
            || self.original_command_digest != plan.original_command_digest
            || self.original_release_ref != plan.original_release_ref
            || self.original_release_digest != plan.original_release_digest
            || self.journal_seq != plan.journal_seq
            || self.journal_head != plan.journal_head
            || self.config_files_digest != plan.config_files_digest
            || self.service_configuration_digest != plan.service_configuration_digest
            || self.installed_images_digest != plan.images_digest()?
            || self.journal_read_max != plan.journal_read_max
            || self.updater_version != plan.updater_version
            || self.committed_at < plan.authorized_at
            || self.committed_at > plan.authorization_deadline
            || self.committed_at > now
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn digest(&self) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&("forme-management-reader-repair-receipt-v1", self))
    }
}
