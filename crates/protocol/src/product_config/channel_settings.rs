//! A bounded channel list edit that preserves write-only publisher material.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductChannelSettingsPatch {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub channels: Vec<ProductChannelConfig>,
}
impl ProductChannelSettingsPatch {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.expected_revision > 1_000_000_000 {
            return Err(Error(
                "channel settings version or revision is invalid".into(),
            ));
        }
        validate_product_id(&self.operation_id)?;
        self.owner.validate()?;
        ProductChannelSettings {
            schema_version: SchemaVersion(1),
            channels: self.channels.clone(),
        }
        .validate()
    }
    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme.product.channel-settings.v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductChannelSettingsView {
    pub schema_version: SchemaVersion,
    pub owner: Option<OwnerPrincipalRef>,
    pub active_revision: u64,
    pub pending_restart: bool,
    pub configured: bool,
    pub channels: Vec<ProductChannelConfig>,
    pub sources: Vec<ProductChannelConfig>,
    pub sources_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductChannelSettingsOperation {
    pub schema_version: SchemaVersion,
    pub owner: OwnerPrincipalRef,
    pub operation_id: String,
    pub request_digest: SchemaDigest,
    pub committed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductChannelSettingsResponse {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub applied: bool,
    pub view: ProductChannelSettingsView,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channel_settings_digest_matches_web_golden() {
        let patch: ProductChannelSettingsPatch=serde_json::from_str(r#"{"schema_version":1,"operation_id":"save:channel-golden","expected_revision":1,"owner":"owner:test","channels":[{"schema_version":1,"channel_ref":"channel:golden","adapter_ref":"text","connector_ref":"connector:golden","scope":"workspace:default","enabled":false,"participant_binding":"mapping:guest","credential_ref":"credential:guest"}]}"#).unwrap();
        assert_eq!(
            patch.digest().unwrap().0,
            "sha256:a26ab8e279015184808ce91750dfb1a90454ab24df97e1772a8ea352d33e7d75"
        );
    }
}
