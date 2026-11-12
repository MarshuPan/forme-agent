//! Host-provisioned declarative plugin sources. Never executable modules.
use super::*;
use crate::Scope;
use std::collections::BTreeSet;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPluginSourceCatalog {
    pub schema_version: SchemaVersion,
    pub sources: Vec<ProductPluginSource>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPluginSource {
    pub schema_version: SchemaVersion,
    pub source_ref: String,
    pub plugin_ref: String,
    pub manifest_ref: String,
    pub scope: Scope,
    pub skills: Vec<ProductSkillConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_slots: Vec<ProductPluginToolSlot>,
}

impl ProductPluginSourceCatalog {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.sources.len() > 64 {
            return Err(invalid());
        }
        let mut sources = BTreeSet::new();
        let mut plugins = BTreeSet::new();
        let mut slots = BTreeSet::new();
        let mut hooks = BTreeSet::new();
        for source in &self.sources {
            source.validate()?;
            for slot in &source.tool_slots {
                if !slots.insert(&slot.id) {
                    return Err(invalid());
                }
                for hook in &slot.hooks {
                    if !hooks.insert(&hook.id) {
                        return Err(invalid());
                    }
                }
            }
            if !sources.insert(&source.source_ref) || !plugins.insert(&source.plugin_ref) {
                return Err(invalid());
            }
        }
        if serde_json::to_vec(self).map_err(|_| invalid())?.len() > 256 * 1024 {
            return Err(invalid());
        }
        Ok(())
    }
}

impl ProductPluginSource {
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.schema_version, SchemaVersion(1) | SchemaVersion(2))
            || (self.schema_version == SchemaVersion(1) && !self.tool_slots.is_empty())
            || (self.skills.is_empty() && self.tool_slots.is_empty())
            || self.skills.len() > 64
            || self.tool_slots.len() > 64
        {
            return Err(invalid());
        }
        for value in [
            &self.source_ref,
            &self.plugin_ref,
            &self.manifest_ref,
            &self.scope.0,
        ] {
            validate_product_id(value)?;
        }
        let mut ids = BTreeSet::new();
        for skill in &self.skills {
            skill.validate()?;
            if skill.enabled
                || skill.trust != ProductSkillTrust::Untrusted
                || skill.scope != self.scope
                || !ids.insert(&skill.id)
            {
                return Err(invalid());
            }
        }
        let mut slots = BTreeSet::new();
        let mut hooks = BTreeSet::new();
        for slot in &self.tool_slots {
            slot.validate()?;
            if !slots.insert(&slot.id) {
                return Err(invalid());
            }
            for hook in &slot.hooks {
                if !hooks.insert(&hook.id) {
                    return Err(invalid());
                }
            }
        }
        if serde_json::to_vec(self).map_err(|_| invalid())?.len() > 65_536 {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme.product.plugin-source.v1", self))
    }
    pub fn validate_settings(&self, settings: &ProductPluginConfig) -> Result<()> {
        settings.validate()?;
        if !settings.enabled
            || settings.trust != ProductSkillTrust::OwnerInput
            || settings.source_ref != self.source_ref
            || settings.plugin_ref != self.plugin_ref
            || settings.scope != self.scope
            || settings.expected_source_digest != self.digest()?
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn invalid() -> Error {
    Error("managed plugin source is invalid or outside its explicit configuration".into())
}
