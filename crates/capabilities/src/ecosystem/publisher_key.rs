//! Public verification material with explicit Host provisioning validation.
use super::sha256_prefixed;
use ed25519_dalek::VerifyingKey;
use forme_protocol as p;
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct PublisherPublicKey([u8; 32]);

impl PublisherPublicKey {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Validate Host-provisioned material before constructing a runtime keyring.
    pub fn try_from_bytes(bytes: [u8; 32]) -> p::Result<Self> {
        let point = VerifyingKey::from_bytes(&bytes)
            .map_err(|_| p::Error("publisher public key is invalid".into()))?;
        if point.is_weak() {
            return Err(p::Error("publisher public key is weak".into()));
        }
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn digest(&self) -> p::SchemaDigest {
        sha256_prefixed(&self.0)
    }
}

impl fmt::Debug for PublisherPublicKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PublisherPublicKey")
            .field(&self.digest())
            .finish()
    }
}
