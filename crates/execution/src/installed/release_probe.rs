//! The owner bearer is never sent to the core readiness listener.
use ed25519_dalek::{Signature, Signer, SigningKey};
use forme_protocol as p;
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub(super) fn probe(
    address: SocketAddr,
    credential: &[u8],
    binding: &p::ProductReleaseBinding,
    revision: u64,
    deadline: Instant,
) -> p::Result<p::ProductReleaseReadiness> {
    binding.validate()?;
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(invalid());
    }
    let seed = Zeroizing::new(p::product_release_probe_seed(credential)?);
    let key = SigningKey::from_bytes(&seed);
    let mut nonce = [0_u8; 32];
    getrandom::fill(&mut nonce).map_err(|_| invalid())?;
    let mut request = p::ProductReleaseReadinessProbe {
        schema_version: p::SchemaVersion(1),
        install_ref: binding.install_ref.clone(),
        nonce: nonce.iter().map(|b| format!("{b:02x}")).collect(),
        signature: Vec::new(),
    };
    request.signature = key
        .sign(request.signing_digest()?.0.as_bytes())
        .to_bytes()
        .to_vec();
    let body = serde_json::to_vec(&request).map_err(|_| invalid())?;
    let mut stream =
        TcpStream::connect_timeout(&address, remaining(deadline)?).map_err(|_| invalid())?;
    stream
        .set_write_timeout(Some(remaining(deadline)?))
        .map_err(|_| invalid())?;
    let head = format!("POST /v1/product/release/readiness HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(&body))
        .map_err(|_| invalid())?;
    let mut bytes = Vec::new();
    let mut block = [0; 2048];
    loop {
        stream
            .set_read_timeout(Some(remaining(deadline)?))
            .map_err(|_| invalid())?;
        let n = stream.read(&mut block).map_err(|_| invalid())?;
        if n == 0 {
            break;
        }
        if bytes.len() + n > 16384 {
            return Err(invalid());
        }
        bytes.extend_from_slice(&block[..n]);
    }
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut response = httparse::Response::new(&mut headers);
    let offset = match response.parse(&bytes) {
        Ok(httparse::Status::Complete(offset)) if response.code == Some(200) => offset,
        _ => return Err(invalid()),
    };
    let proof: p::ProductReleaseReadinessProof =
        serde_json::from_slice(&bytes[offset..]).map_err(|_| invalid())?;
    proof.validate_for(&request)?;
    key.verifying_key()
        .verify_strict(
            proof.signing_digest()?.0.as_bytes(),
            &Signature::from_slice(&proof.signature).map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?;
    if proof.readiness.binding != *binding || proof.readiness.config_revision != revision {
        return Err(invalid());
    }
    remaining(deadline)?;
    Ok(proof.readiness)
}
fn remaining(deadline: Instant) -> p::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .map(|d| d.min(Duration::from_secs(3)))
        .ok_or_else(invalid)
}
fn invalid() -> p::Error {
    p::Error("current installed core release readiness could not be verified".into())
}
#[cfg(test)]
mod tests;
