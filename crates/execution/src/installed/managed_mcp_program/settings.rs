//! Configuration admission uses independent program/worker facts, never config re-entry.
use super::*;

impl InstalledManagedMcpPrograms {
    /// Caller already holds the configuration lock. Keep the same lock order as
    /// dispatch: configuration -> registry -> source; never call with_registry.
    pub(crate) fn validate_enabled_settings(
        &self,
        installation: &str,
        owner: &p::OwnerPrincipalRef,
        execution_authority: &str,
        settings: &p::ProductMcpServerConfig,
    ) -> p::Result<()> {
        settings.validate()?;
        if settings.trust != p::ProductSkillTrust::OwnerInput
            || !settings.enabled
            || !self.scopes.contains(&settings.scope)
            || settings.allowlist.tools.is_empty() && settings.allowlist.resources.is_empty()
            || !self.journal.is_file()
        {
            return Err(invalid());
        }
        self.require_live_consumer(execution_authority)?;
        let registry = self.registry_for_owner(installation, owner)?;
        let digest = p::SchemaDigest(format!(
            "sha256:{}",
            settings
                .program_ref
                .strip_prefix("mcp-program:")
                .ok_or_else(invalid)?
        ));
        p::sha256_digest_bytes(&digest)?;
        let lease = registry.acquire(
            owner,
            self.epoch,
            &settings.program_ref,
            &digest,
            &settings.scope,
        )?;
        registry.with_active(&lease, || self.require_live_consumer(execution_authority))
    }

    pub(crate) fn require_live_consumer(&self, installation: &str) -> p::Result<()> {
        let source = self.source.lock().map_err(|_| invalid())?;
        let source = source.as_ref().ok_or_else(invalid)?;
        if source.installation != installation
            || source.journal.upgrade().is_none()
            || source.results.upgrade().is_none()
        {
            return Err(invalid());
        }
        Ok(())
    }
}
