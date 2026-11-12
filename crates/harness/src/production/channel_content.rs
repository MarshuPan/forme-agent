//! Read only the current owner's registered, lifecycle-protected message body.
use super::*;
impl ProductionRuntime {
    pub fn channel_content_digest(
        &self,
        owner: &p::VerifiedPrincipal,
        scope: &p::Scope,
        reference: &p::ContentRef,
    ) -> p::Result<p::SchemaDigest> {
        let _guard = self.data_read_guard()?;
        if owner.0 != self.config.owner.0 || scope.0 != self.config.workspace.0 {
            return Err(invalid());
        }
        let body = crate::production_artifact_store::read_material(
            &self.services.store,
            reference,
            scope,
        )?;
        let text = body.text.ok_or_else(invalid)?;
        if text.is_empty() || text.len() as u64 > p::MANAGED_CHANNEL_MAX_BYTES {
            return Err(invalid());
        }
        Ok(p::sha256_content_digest(text.as_bytes()))
    }
}
fn invalid() -> p::Error {
    p::Error("protected channel message is unavailable".into())
}
