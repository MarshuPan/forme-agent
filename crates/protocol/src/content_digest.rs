//! Shared SHA-256 encoding; streaming keeps backup hashing memory bounded.
use crate::{Error, Result, SchemaDigest};
use sha2::{Digest, Sha256};
use std::io::Read;

pub fn sha256_content_digest(bytes: &[u8]) -> SchemaDigest {
    encoded(Sha256::digest(bytes).into())
}

/// Hashes a stream with a fixed buffer. Errors contain no source content/path.
pub fn sha256_reader_digest(mut reader: impl Read) -> Result<SchemaDigest> {
    let mut state = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(encoded(state.finalize().into())),
            Ok(size) => state.update(&buffer[..size]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(Error("content digest source is unavailable".into())),
        }
    }
}

fn encoded(bytes: [u8; 32]) -> SchemaDigest {
    let mut encoded = String::from("sha256:");
    for byte in bytes {
        use core::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    SchemaDigest(encoded)
}

pub fn sha256_digest_bytes(digest: &SchemaDigest) -> Result<[u8; 32]> {
    let Some(encoded) = digest.0.strip_prefix("sha256:") else {
        return Err(Error("digest is not sha256-prefixed".into()));
    };
    if encoded.len() != 64 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error("digest is not a canonical sha256 value".into()));
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let text = core::str::from_utf8(pair)
            .map_err(|_| Error("digest contains invalid bytes".into()))?;
        bytes[index] = u8::from_str_radix(text, 16)
            .map_err(|_| Error("digest contains invalid hexadecimal".into()))?;
    }
    Ok(bytes)
}
