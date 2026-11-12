//! The isolated worker can request only the exact Host-issued delivery grant.
use forme_protocol as p;
use std::io::{Read, Write};
pub fn run() -> p::Result<()> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(1025)
        .read_to_end(&mut input)
        .map_err(|_| invalid())?;
    if input.len() > 1024 {
        return Err(invalid());
    }
    let request: p::ManagedChannelWorkerRequestV1 =
        serde_json::from_slice(&input).map_err(|_| invalid())?;
    request.validate()?;
    let mut output = serde_json::to_vec(&request).map_err(|_| invalid())?;
    output.push(b'\n');
    std::io::stdout().write_all(&output).map_err(|_| invalid())
}
fn invalid() -> p::Error {
    p::Error("isolated channel request is invalid".into())
}
