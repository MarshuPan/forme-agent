//! Read-only installed backup verification, separate from brain commands.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductBackupVerificationRequest {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub owner: OwnerPrincipalRef,
    pub expected_config_revision: u64,
    pub source: String,
}

impl ProductBackupVerificationRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.expected_config_revision == 0 {
            return Err(Error("backup verification request is invalid".into()));
        }
        validate_product_id(&self.install_ref)?;
        self.owner.validate()?;
        validate_data_object_name(&self.source)
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme-offline-backup-verification-v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductBackupVerificationResponse {
    pub schema_version: SchemaVersion,
    pub request_digest: SchemaDigest,
    pub config_revision: u64,
    pub verification: DataBackupView,
}

impl ProductBackupVerificationResponse {
    pub fn validate_for(&self, request: &ProductBackupVerificationRequest) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.request_digest != request.digest()?
            || self.config_revision != request.expected_config_revision
            || self.verification.schema_version != SchemaVersion(1)
            || self.verification.object != request.source
            || !self.verification.verified
            || self.verification.created_at < 0
            || self.verification.restoration.is_some()
        {
            return Err(Error(
                "backup verification response does not match the request".into(),
            ));
        }
        validate_product_digest(&self.verification.digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_verification_binds_owner_revision_installation_and_opaque_object() {
        let request = ProductBackupVerificationRequest {
            schema_version: SchemaVersion(1),
            install_ref: "install:verify".into(),
            owner: OwnerPrincipalRef("owner:verify".into()),
            expected_config_revision: 2,
            source: "backup.sqlite".into(),
        };
        let response = ProductBackupVerificationResponse {
            schema_version: SchemaVersion(1),
            request_digest: request.digest().unwrap(),
            config_revision: 2,
            verification: DataBackupView {
                schema_version: SchemaVersion(1),
                object: request.source.clone(),
                digest: canonical_digest(&"verified-manifest").unwrap(),
                events: 1,
                created_at: 1,
                verified: true,
                restoration: None,
            },
        };
        response.validate_for(&request).unwrap();
        for field in ["owner", "revision", "install", "source"] {
            let mut changed = request.clone();
            match field {
                "owner" => changed.owner = OwnerPrincipalRef("owner:other".into()),
                "revision" => changed.expected_config_revision += 1,
                "install" => changed.install_ref = "install:other".into(),
                _ => changed.source = "other.sqlite".into(),
            }
            assert!(response.validate_for(&changed).is_err());
        }
        for source in [
            "../backup.sqlite",
            "C:\\backup.sqlite",
            "https://host/file",
            ".",
            "NUL",
        ] {
            let mut invalid = request.clone();
            invalid.source = source.into();
            assert!(invalid.validate().is_err());
        }
        let mut extra = serde_json::to_value(&request).unwrap();
        extra["path"] = "other.sqlite".into();
        assert!(serde_json::from_value::<ProductBackupVerificationRequest>(extra).is_err());
        let mut invalid = response;
        invalid.verification.verified = false;
        assert!(invalid.validate_for(&request).is_err());
    }
}
