//! One numeric loopback connection; no DNS, redirects, proxies or retries.
use super::*;
use ed25519_dalek::{Signature, VerifyingKey};
use std::io::{Read, Write};
use std::net::TcpStream;

pub(super) fn post(
    grant: &p::ManagedChannelDeliveryGrantV1,
    source: &p::ProductChannelSource,
    content: &[u8],
    secret: &str,
    deadline: Instant,
    cancel: &crate::CancelToken,
) -> p::Result<Vec<u8>> {
    let (address, path) = p::channel_delivery_endpoint(&grant.endpoint)?;
    live(deadline, cancel)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    let mut stream = TcpStream::connect_timeout(&address, remaining).map_err(|_| invalid())?;
    // Only opaque bindings accompany the exact original body. Authentication
    // material is never sent to the worker or retained in the intent companion.
    let header = Zeroizing::new(format!(
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {secret}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\nX-ForMe-Grant: {}\r\nX-ForMe-Job: {}\r\nX-ForMe-Nonce: {}\r\nX-ForMe-Plan: {}\r\nX-ForMe-Content: {}\r\nX-ForMe-Participant: {}\r\n\r\n",
        content.len(), grant.grant_ref, grant.job_id, grant.nonce.0, grant.plan_digest.0,
        grant.content_digest.0, grant.participant.0));
    write(&mut stream, header.as_bytes(), deadline, cancel)?;
    write(&mut stream, content, deadline, cancel)?;
    let mut bytes = Vec::new();
    let mut block = [0; 4096];
    let mut header_end = None;
    let mut length = None;
    loop {
        live(deadline, cancel)?;
        stream
            .set_read_timeout(Some(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50)),
            ))
            .map_err(|_| invalid())?;
        match stream.read(&mut block) {
            Ok(0) => break,
            Ok(size) => bytes.extend_from_slice(&block[..size]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                continue
            }
            Err(_) => return Err(invalid()),
        }
        if bytes.len() as u64 > 8192 + grant.max_output_bytes {
            return Err(invalid());
        }
        if header_end.is_none() {
            if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                if end > 8192 {
                    return Err(invalid());
                }
                length = Some(response_length(&bytes[..end], grant.max_output_bytes)?);
                header_end = Some(end + 4);
            } else if bytes.len() > 8192 {
                return Err(invalid());
            }
        }
        if let (Some(end), Some(length)) = (header_end, length) {
            if bytes.len() >= end + length {
                break;
            }
        }
    }
    live(deadline, cancel)?;
    let end = header_end.ok_or_else(invalid)?;
    if bytes.len() != end + length.ok_or_else(invalid)? {
        return Err(invalid());
    }
    let body = bytes.split_off(end);
    verify_receipt(grant, source, &body)?;
    Ok(body)
}
fn write(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    deadline: Instant,
    cancel: &crate::CancelToken,
) -> p::Result<()> {
    while !bytes.is_empty() {
        live(deadline, cancel)?;
        stream
            .set_write_timeout(Some(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50)),
            ))
            .map_err(|_| invalid())?;
        match stream.write(bytes) {
            Ok(0) => return Err(invalid()),
            Ok(size) => bytes = &bytes[size..],
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(_) => return Err(invalid()),
        }
    }
    Ok(())
}
fn response_length(bytes: &[u8], limit: u64) -> p::Result<usize> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut lines = text.split("\r\n");
    if !matches!(
        lines.next(),
        Some("HTTP/1.1 200 OK") | Some("HTTP/1.0 200 OK")
    ) {
        return Err(invalid());
    }
    let mut length = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        if name.eq_ignore_ascii_case("transfer-encoding") || name.eq_ignore_ascii_case("location") {
            return Err(invalid());
        }
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(invalid());
            }
            let value: usize = value.trim().parse().map_err(|_| invalid())?;
            if value == 0 || value as u64 > limit {
                return Err(invalid());
            }
            length = Some(value);
        }
    }
    length.ok_or_else(invalid)
}
pub(crate) fn verify_receipt(
    grant: &p::ManagedChannelDeliveryGrantV1,
    source: &p::ProductChannelSource,
    bytes: &[u8],
) -> p::Result<()> {
    if bytes.len() as u64 > grant.max_output_bytes {
        return Err(invalid());
    }
    let receipt: p::ManagedChannelDeliveryReceiptV1 =
        serde_json::from_slice(bytes).map_err(|_| invalid())?;
    receipt.validate_binding(grant)?;
    let key = VerifyingKey::from_bytes(
        &source
            .delivery
            .as_ref()
            .ok_or_else(invalid)?
            .receipt_public_key,
    )
    .map_err(|_| invalid())?;
    let signature = Signature::from_slice(&receipt.signature).map_err(|_| invalid())?;
    key.verify_strict(&receipt.signing_bytes()?, &signature)
        .map_err(|_| invalid())
}
