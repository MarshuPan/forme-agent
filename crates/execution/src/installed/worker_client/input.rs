use super::{decode, Operation, WorkerBrokerClient};
use forme_protocol as p;

pub(super) fn read(
    client: &WorkerBrokerClient,
    request: p::WorkerInputReadRequest,
) -> p::Result<p::WorkerInputChunk> {
    request.validate()?;
    let job_id = request.grant.job_id.clone();
    let body = client.json(
        "POST",
        &format!("/v1/agent/workers/{job_id}/input"),
        Some(&request),
        Operation::Dispatch,
    )?;
    let chunk: p::WorkerInputChunk = decode(&body, Operation::Dispatch)?;
    chunk.validate()?;
    if chunk.grant_ref != request.grant.grant_ref
        || chunk.offset != request.offset
        || chunk.next_offset > request.grant.content_bytes.unwrap_or(0)
    {
        return Err(Operation::Dispatch.transport_error());
    }
    Ok(chunk)
}
