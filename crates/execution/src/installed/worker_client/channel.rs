//! Dedicated channel dispatch stays separate from the generic worker routes.
use super::*;

pub(super) fn dispatch(
    client: &WorkerBrokerClient,
    input: p::ManagedChannelDeliveryInputV1,
) -> p::Result<p::WorkerObservation> {
    input.execution.validate_at(now_ms())?;
    if input.schema_version != p::SchemaVersion(1)
        || input.content_bytes.len() as u64 > p::MANAGED_CHANNEL_MAX_BYTES
    {
        return Err(Operation::Dispatch.transport_error());
    }
    let bytes = client.json(
        "POST",
        "/v1/agent/workers/channel/dispatch",
        Some(&input),
        Operation::Dispatch,
    )?;
    let observation: p::WorkerObservation = decode(&bytes, Operation::Dispatch)?;
    validate_observation_for_job(&observation, &input.execution.job)?;
    Ok(observation)
}
