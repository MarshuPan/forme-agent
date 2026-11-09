//! Host-authored publisher material is loaded before package registry recovery.
use super::*;
use forme_capabilities::{InMemoryPublisherKeyring, PublisherPublicKey};
use std::sync::Arc;

impl ManagedBrainConfig {
    pub(crate) fn publisher_keyring(&self) -> p::Result<Arc<InMemoryPublisherKeyring>> {
        let ring = Arc::new(InMemoryPublisherKeyring::default());
        let keys = match &self.managed_capabilities {
            Some(value) => {
                value.validate()?;
                value.publisher_keys.as_slice()
            }
            None => &[],
        };
        if keys.len() != self.publisher_key_files.len() {
            return Err(invalid());
        }
        if let Some(channels) = &self.channels {
            channels.validate()?;
        }
        for key in keys {
            if !key.configured {
                return Err(invalid());
            }
            let path = self
                .publisher_key_files
                .get(&key.publisher_ref)
                .ok_or_else(invalid)?;
            if path == &self.store_path || path.starts_with(&self.artifact_root) {
                return Err(invalid());
            }
            let bytes: [u8; 32] = read_managed_file(path, 32)?
                .try_into()
                .map_err(|_| invalid())?;
            if p::sha256_content_digest(&bytes) != key.public_key_digest {
                return Err(invalid());
            }
            ring.provision(
                p::CapabilityPublisherRef(key.publisher_ref.clone()),
                PublisherPublicKey::try_from_bytes(bytes)?,
            )?;
        }
        Ok(ring)
    }
}
