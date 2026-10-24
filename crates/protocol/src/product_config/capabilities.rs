//! Installed skills are data; enabling them never grants executable authority.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductCapabilitySettings {
    pub schema_version: SchemaVersion,
    pub skills: Vec<ProductSkillConfig>,
}

impl Default for ProductCapabilitySettings {
    fn default() -> Self {
        Self {
            schema_version: PRODUCT_CONFIG_SCHEMA_VERSION,
            skills: Vec::new(),
        }
    }
}

impl ProductCapabilitySettings {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_CONFIG_SCHEMA_VERSION || self.skills.len() > 128 {
            return Err(Error(
                "product capability settings are invalid or unbounded".into(),
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for skill in &self.skills {
            skill.validate()?;
            if !ids.insert(&skill.id) {
                return Err(Error("product skill identities are duplicated".into()));
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| Error("product skills cannot be encoded".into()))?
            .len()
            > 65_536
        {
            return Err(Error(
                "product capability settings exceed their bound".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductSkillTrust {
    #[default]
    Untrusted,
    OwnerInput,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductSkillConfig {
    pub schema_version: SchemaVersion,
    pub id: crate::SkillRef,
    pub summary: String,
    pub scope: crate::Scope,
    pub version: crate::Version,
    pub body: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub trust: ProductSkillTrust,
}

impl std::fmt::Debug for ProductSkillConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductSkillConfig")
            .field("id", &self.id)
            .field("version", &self.version)
            .field("enabled", &self.enabled)
            .field("trust", &self.trust)
            .finish_non_exhaustive()
    }
}

impl ProductSkillConfig {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_CONFIG_SCHEMA_VERSION
            || self.version.0 == 0
            || !valid_text(&self.summary, 2_048)
            || !valid_text(&self.body, 16_384)
        {
            return Err(Error("product skill is invalid or unbounded".into()));
        }
        validate_product_id(&self.id.0)?;
        validate_product_id(&self.scope.0)
    }
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}
