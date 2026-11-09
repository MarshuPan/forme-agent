//! Only declarative, scoped skills may cross this managed configuration seam.
use super::*;
use forme_capabilities::{SkillBody, SkillDefinition, SkillMetadata};

impl ManagedBrainConfig {
    pub(super) fn runtime_capabilities(&self) -> p::Result<CapabilityRuntimeConfig> {
        let mut capabilities = CapabilityRuntimeConfig::default();
        self.configure_file_placement(&mut capabilities)?;
        self.configure_desktop_placement(&mut capabilities)?;
        if let Some(settings) = &self.capabilities {
            settings.validate()?;
            capabilities.skills = settings
                .skills
                .iter()
                .filter(|skill| skill.enabled)
                .map(|skill| SkillDefinition {
                    schema_version: p::SchemaVersion(1),
                    metadata: SkillMetadata {
                        schema_version: p::SchemaVersion(1),
                        id: skill.id.clone(),
                        summary: skill.summary.clone(),
                        scope: skill.scope.clone(),
                        version: skill.version,
                        trust: match skill.trust {
                            p::ProductSkillTrust::Untrusted => p::TrustTier::Untrusted,
                            p::ProductSkillTrust::OwnerInput => p::TrustTier::OwnerInput,
                        },
                    },
                    body: SkillBody(skill.body.clone()),
                })
                .collect();
        }
        Ok(capabilities)
    }
}
