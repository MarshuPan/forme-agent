//! Bounded, exact-address health probes for the managed Web listener.
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use forme_protocol as p;
use httparse::Status;
use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName};

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RESPONSE_BYTES: usize = 8 * 1024;

pub(super) fn probe(
    network: &p::ProductNetworkConfig,
    expected_listener_digest: &p::SchemaDigest,
    nonce: &str,
) -> p::Result<p::ProductNetworkHealthObservation> {
    network.validate()?;
    let stream = match TcpStream::connect_timeout(&network.bind, PROBE_TIMEOUT) {
        Ok(stream) => stream,
        Err(_) => return Ok(p::ProductNetworkHealthObservation::Unreachable),
    };
    stream
        .set_read_timeout(Some(PROBE_TIMEOUT))
        .and_then(|_| stream.set_write_timeout(Some(PROBE_TIMEOUT)))
        .map_err(|_| p::Error("Web health probe socket limits could not be applied".into()))?;
    let authority = network
        .allowed_hosts
        .first()
        .cloned()
        .unwrap_or_else(|| network.bind.to_string());
    let request = format!(
        "GET /api/v1/product/network/health?nonce={nonce} HTTP/1.1\r\nHost: {authority}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    if let Some(tls) = &network.tls {
        let certificates = read_certificates(&tls.certificate_path)?;
        let mut roots = rustls::RootCertStore::empty();
        for certificate in &certificates {
            roots
                .add(certificate.clone())
                .map_err(|_| p::Error("Web health certificate trust is unavailable".into()))?;
        }
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|_| p::Error("Web health TLS policy is unavailable".into()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let name = server_name(&authority)?;
        let client = rustls::ClientConnection::new(Arc::new(config), name)
            .map_err(|_| p::Error("Web health TLS identity is invalid".into()))?;
        let mut stream = rustls::StreamOwned::new(client, stream);
        stream
            .write_all(request.as_bytes())
            .and_then(|_| stream.flush())
            .map_err(|_| p::Error("Web health TLS request could not be sent".into()))?;
        let peer = stream
            .conn
            .peer_certificates()
            .and_then(|certificates| certificates.first());
        if peer != certificates.first() {
            return Ok(p::ProductNetworkHealthObservation::Untrusted);
        }
        observe_response(&mut stream, expected_listener_digest, nonce)
    } else {
        let mut stream = stream;
        stream
            .write_all(request.as_bytes())
            .and_then(|_| stream.flush())
            .map_err(|_| p::Error("Web health request could not be sent".into()))?;
        observe_response(&mut stream, expected_listener_digest, nonce)
    }
}

fn observe_response(
    stream: &mut impl Read,
    expected_listener_digest: &p::SchemaDigest,
    nonce: &str,
) -> p::Result<p::ProductNetworkHealthObservation> {
    let mut response = Vec::new();
    stream
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut response)
        .map_err(|_| p::Error("Web health response could not be read".into()))?;
    if response.len() > MAX_RESPONSE_BYTES {
        return Ok(p::ProductNetworkHealthObservation::Untrusted);
    }
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut parsed = httparse::Response::new(&mut headers);
    let body_offset = match parsed.parse(&response) {
        Ok(Status::Complete(offset)) if parsed.code == Some(200) => offset,
        _ => return Ok(p::ProductNetworkHealthObservation::Untrusted),
    };
    let proof: p::ProductNetworkHealthProof = match serde_json::from_slice(&response[body_offset..])
    {
        Ok(proof) => proof,
        Err(_) => return Ok(p::ProductNetworkHealthObservation::Untrusted),
    };
    if proof.validate().is_err()
        || proof.listener_digest != *expected_listener_digest
        || proof.nonce != nonce
    {
        return Ok(p::ProductNetworkHealthObservation::Untrusted);
    }
    Ok(p::ProductNetworkHealthObservation::Verified(proof))
}

fn read_certificates(path: &Path) -> p::Result<Vec<CertificateDer<'static>>> {
    let file = std::fs::File::open(path)
        .map_err(|_| p::Error("Web health certificate is unavailable".into()))?;
    if !file
        .metadata()
        .map_err(|_| p::Error("Web health certificate is unavailable".into()))?
        .is_file()
    {
        return Err(p::Error("Web health certificate is unavailable".into()));
    }
    let mut bytes = Vec::new();
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| p::Error("Web health certificate is unavailable".into()))?;
    if bytes.is_empty() || bytes.len() > 65_536 {
        return Err(p::Error("Web health certificate is invalid".into()));
    }
    let certificates = CertificateDer::pem_slice_iter(&bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| p::Error("Web health certificate is invalid".into()))?;
    if certificates.is_empty() || certificates.len() > 8 {
        return Err(p::Error("Web health certificate is invalid".into()));
    }
    Ok(certificates)
}

fn server_name(authority: &str) -> p::Result<ServerName<'static>> {
    let host = authority
        .strip_prefix('[')
        .and_then(|value| value.split_once(']').map(|(host, _)| host))
        .or_else(|| authority.rsplit_once(':').map(|(host, _)| host))
        .unwrap_or(authority);
    if let Ok(address) = host.parse::<IpAddr>() {
        return Ok(ServerName::IpAddress(address.into()));
    }
    ServerName::try_from(host.to_owned())
        .map_err(|_| p::Error("Web health certificate name is invalid".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_name_rejects_ambiguous_authorities() {
        assert!(server_name("[2001:db8::1]:8765").is_ok());
        assert!(server_name("127.0.0.1:8765").is_ok());
        assert!(server_name("bad host:8765").is_err());
    }
}
