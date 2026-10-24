//! Preferences consumed by the managed production brain, never by the Web model client.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductModelRouting {
    pub schema_version: SchemaVersion,
    pub foreground: Option<crate::ModelProfileRef>,
    pub cognition: Option<crate::ModelProfileRef>,
}

impl Default for ProductModelRouting {
    fn default() -> Self {
        Self {
            schema_version: PRODUCT_CONFIG_SCHEMA_VERSION,
            foreground: None,
            cognition: None,
        }
    }
}

impl ProductModelRouting {
    pub fn validate(&self, profiles: &[String]) -> Result<()> {
        if self.schema_version != PRODUCT_CONFIG_SCHEMA_VERSION {
            return Err(Error("product model routing version is invalid".into()));
        }
        for profile in self.foreground.iter().chain(self.cognition.iter()) {
            if !profiles.contains(&profile.0) {
                return Err(Error(
                    "product model routing references an absent profile".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductBrainPreferences {
    pub schema_version: SchemaVersion,
    pub default_workspace: WorkspaceRef,
    pub model_routing: ProductModelRouting,
}

impl ProductBrainPreferences {
    pub fn validate(&self, profiles: &[String]) -> Result<()> {
        if self.schema_version != PRODUCT_CONFIG_SCHEMA_VERSION {
            return Err(Error("product brain preferences version is invalid".into()));
        }
        validate_product_id(&self.default_workspace.0)?;
        self.model_routing.validate(profiles)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductPreferenceSource {
    InstallationTemplate,
    ManagedConfiguration,
}

impl ProductConfigPatch {
    /// Preserve pre-preference journal receipts byte-for-byte when the
    /// additive field is absent. Explicit preferences have a new digest domain.
    pub fn digest(&self) -> Result<SchemaDigest> {
        let legacy = (
            &self.schema_version,
            &self.operation_id,
            &self.expected_revision,
            &self.owner,
            &self.models,
        );
        match &self.brain {
            None => crate::canonical_digest(&legacy),
            Some(brain) => {
                crate::canonical_digest(&("forme-product-brain-preferences-v1", legacy, brain))
            }
        }
    }
}
