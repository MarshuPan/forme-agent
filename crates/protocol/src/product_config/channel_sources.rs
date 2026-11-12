//! Private installation-owned connector identity mapping; never browser input.
use super::*;
use std::collections::BTreeSet;
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductChannelSourceCatalog {
    pub schema_version: SchemaVersion,
    pub sources: Vec<ProductChannelSource>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductChannelSource {
    pub schema_version: SchemaVersion,
    pub channel_ref: String,
    pub connector_ref: String,
    pub scope: crate::Scope,
    pub participant_binding: String,
    pub participant: crate::ParticipantId,
    pub credential_ref: String,
    pub credential_digest: SchemaDigest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<crate::ProductChannelDeliverySource>,
}
impl ProductChannelSource {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(if self.delivery.is_some() { 2 } else { 1 })
            || matches!(
                self.participant.0.as_str(),
                "owner" | "forme-representative"
            )
        {
            return Err(invalid());
        }
        for id in [
            &self.channel_ref,
            &self.connector_ref,
            &self.scope.0,
            &self.participant_binding,
            &self.participant.0,
            &self.credential_ref,
        ] {
            validate_product_id(id)?;
        }
        crate::sha256_digest_bytes(&self.credential_digest)?;
        if let Some(delivery) = &self.delivery {
            delivery.validate()?;
        }
        Ok(())
    }
    pub fn validate_settings(
        &self,
        settings: &ProductChannelConfig,
        owner: &OwnerPrincipalRef,
        workspace: &crate::WorkspaceRef,
    ) -> Result<()> {
        self.validate()?;
        settings.validate()?;
        if !settings.enabled
            || !(settings.adapter_ref == ProductChannelAdapter::Text
                || (settings.adapter_ref == ProductChannelAdapter::Loopback
                    && self.delivery.is_some()))
            || settings.channel_ref != self.channel_ref
            || settings.connector_ref != self.connector_ref
            || settings.scope != self.scope
            || self.scope.0 != workspace.0
            || settings.participant_binding != self.participant_binding
            || self.participant.0 == owner.0
            || settings.credential_ref.as_ref() != Some(&self.credential_ref)
        {
            return Err(invalid());
        }
        Ok(())
    }
}
impl ProductChannelSourceCatalog {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.sources.len() > 64 {
            return Err(invalid());
        }
        let mut ids = BTreeSet::new();
        let mut credentials = BTreeSet::new();
        for source in &self.sources {
            source.validate()?;
            if !ids.insert(&source.channel_ref) || !credentials.insert(&source.credential_digest) {
                return Err(invalid());
            }
        }
        if serde_json::to_vec(self).map_err(|_| invalid())?.len() > 128 * 1024 {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductChannelIngress {
    pub schema_version: SchemaVersion,
    pub configuration_path: PathBuf,
    pub sources: Vec<ProductChannelSource>,
}
fn invalid() -> Error {
    Error("managed channel source is invalid or outside its configured identity".into())
}
