//! Bind protected bytes before the original delivery plan and approval exist.
use super::*;
impl LoopbackChannelAdapter {
    pub fn prepare_bound_delivery(
        &self,
        fabric: &InMemoryCommunicationFabric,
        message: OutboundMessage,
        digest: p::SchemaDigest,
    ) -> p::Result<PreparedDelivery> {
        p::sha256_digest_bytes(&digest)?;
        p::channel_delivery_endpoint(&self.endpoint)?;
        let mut prepared = self.prepare_delivery(fabric, message)?;
        let p::ActionParameters::AppApi(spec) = &mut prepared.intent.parameters else {
            return Err(invalid());
        };
        let p::AppApiOperation::Mutation {
            idempotency_key, ..
        } = &mut spec.operation
        else {
            return Err(invalid());
        };
        idempotency_key.0.push(':');
        idempotency_key.0.push_str(&digest.0);
        prepared.intent.risk_hint = p::Risk::High;
        Ok(prepared)
    }
}
fn invalid() -> p::Error {
    p::Error("fixed message delivery could not be prepared".into())
}
