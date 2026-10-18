//! Application-layer protected-content contracts.
//!
//! The envelope is safe to persist or place in an event payload.  It carries
//! no key material, secret reference, plaintext, or private endpoint.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

use crate::{
    canonical_digest, sha256_digest_bytes, ObjectRef, Result, SchemaDigest, SchemaVersion, Scope,
};

pub const CONTENT_PROTECTION_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
pub const CHACHA20_POLY1305_KEY_BYTES: usize = 32;
pub const CHACHA20_POLY1305_NONCE_BYTES: usize = 12;
pub const CHACHA20_POLY1305_TAG_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContentEncryptionAlgorithm {
    ChaCha20Poly1305,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentBinding {
    pub schema_version: SchemaVersion,
    pub owner_digest: SchemaDigest,
    pub scope: Scope,
    pub object: ObjectRef,
}

impl ContentBinding {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(crate::Error("content binding schema is invalid".into()));
        }
        validate_digest(&self.owner_digest, "content owner digest")?;
        validate_safe_reference(&self.scope.0, "content scope")?;
        validate_safe_reference(&self.object.0, "content object")
    }

    pub fn aad_digest(
        &self,
        algorithm: ContentEncryptionAlgorithm,
        key_digest: &SchemaDigest,
    ) -> Result<SchemaDigest> {
        self.validate()?;
        validate_digest(key_digest, "content key digest")?;
        canonical_digest(&ContentAad {
            binding: self,
            algorithm,
            key_digest,
        })
    }
}

#[derive(Serialize)]
struct ContentAad<'a> {
    binding: &'a ContentBinding,
    algorithm: ContentEncryptionAlgorithm,
    key_digest: &'a SchemaDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedContentEnvelope {
    pub schema_version: SchemaVersion,
    pub algorithm: ContentEncryptionAlgorithm,
    pub owner_digest: SchemaDigest,
    pub scope: Scope,
    pub object: ObjectRef,
    pub key_digest: SchemaDigest,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub aad_digest: SchemaDigest,
}

impl EncryptedContentEnvelope {
    pub fn binding(&self) -> ContentBinding {
        ContentBinding {
            schema_version: self.schema_version,
            owner_digest: self.owner_digest.clone(),
            scope: self.scope.clone(),
            object: self.object.clone(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != CONTENT_PROTECTION_SCHEMA_VERSION {
            return Err(crate::Error(
                "encrypted content schema is unsupported".into(),
            ));
        }
        self.binding().validate()?;
        validate_digest(&self.key_digest, "encrypted content key digest")?;
        validate_digest(&self.aad_digest, "encrypted content AAD digest")?;
        if self.nonce.len() != CHACHA20_POLY1305_NONCE_BYTES {
            return Err(crate::Error(
                "encrypted content nonce has invalid length".into(),
            ));
        }
        if self.ciphertext.len() < CHACHA20_POLY1305_TAG_BYTES {
            return Err(crate::Error(
                "encrypted content ciphertext is truncated".into(),
            ));
        }
        if self.algorithm != ContentEncryptionAlgorithm::ChaCha20Poly1305 {
            return Err(crate::Error(
                "encrypted content algorithm is unsupported".into(),
            ));
        }
        Ok(())
    }
}

fn validate_digest(value: &SchemaDigest, name: &str) -> Result<()> {
    sha256_digest_bytes(value)
        .map(|_| ())
        .map_err(|_| crate::Error(format!("{name} is not a canonical sha256 digest")))
}

fn validate_safe_reference(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(crate::Error(format!("{name} is incomplete")));
    }
    let lowered = value.to_ascii_lowercase();
    let forbidden = [
        "secret",
        "credential",
        "password",
        "api_key",
        "apikey",
        "private_key",
        "endpoint",
    ];
    if forbidden.iter().any(|marker| lowered.contains(marker))
        || lowered.contains("://")
        || lowered.starts_with("file:")
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(":\\")
    {
        return Err(crate::Error(format!(
            "{name} contains a secret, endpoint, or private-path marker"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> ContentBinding {
        ContentBinding {
            schema_version: CONTENT_PROTECTION_SCHEMA_VERSION,
            owner_digest: crate::sha256_content_digest(b"owner"),
            scope: Scope("workspace:alpha".into()),
            object: ObjectRef("content:alpha".into()),
        }
    }

    #[test]
    fn content_binding_digest_is_stable_and_scope_bound() {
        let first = binding()
            .aad_digest(
                ContentEncryptionAlgorithm::ChaCha20Poly1305,
                &crate::sha256_content_digest(b"key"),
            )
            .unwrap();
        let second = binding()
            .aad_digest(
                ContentEncryptionAlgorithm::ChaCha20Poly1305,
                &crate::sha256_content_digest(b"key"),
            )
            .unwrap();
        assert_eq!(first, second);

        let mut other = binding();
        other.scope = Scope("workspace:beta".into());
        assert_ne!(
            first,
            other
                .aad_digest(
                    ContentEncryptionAlgorithm::ChaCha20Poly1305,
                    &crate::sha256_content_digest(b"key"),
                )
                .unwrap()
        );
    }
}
