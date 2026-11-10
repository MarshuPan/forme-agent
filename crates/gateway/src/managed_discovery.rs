//! Closed transport bodies; authentication supplies the owner out of band.
use forme_protocol as p;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiscoveryRequest {
    pub(crate) schema_version: p::SchemaVersion,
    pub(crate) session: p::SessionId,
    pub(crate) command: p::IdempotencyKey,
    pub(crate) spec: p::ManagedMcpDiscoverySpec,
}
impl DiscoveryRequest {
    pub(crate) fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1) {
            return Err(invalid());
        }
        p::validate_product_id(&self.session.0)?;
        p::validate_product_id(&self.command.0)?;
        self.spec.validate()
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PublicationRequest {
    pub(crate) schema_version: p::SchemaVersion,
    pub(crate) command: p::IdempotencyKey,
}
impl PublicationRequest {
    pub(crate) fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1) {
            return Err(invalid());
        }
        p::validate_product_id(&self.command.0)
    }
}
fn invalid() -> p::Error {
    p::Error("managed discovery request is invalid".into())
}
