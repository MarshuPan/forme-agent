//! AEAD protection for sensitive content before it reaches durable objects.

#![forbid(unsafe_code)]

use std::sync::Arc;

use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use forme_protocol as p;

/// Local key boundary. Implementations must keep key material outside events,
/// logs, traces, manifests, and portable artifacts.
pub trait ContentKeyResolver: Send + Sync {
    fn resolve(&self, key_digest: &p::SchemaDigest) -> p::Result<Vec<u8>>;
    fn destroy(&self, key_digest: &p::SchemaDigest) -> p::Result<()>;
    /// Allocate or retrieve the external per-subject key. A resolver without
    /// allocation support remains usable for decrypting existing envelopes.
    fn subject_key(
        &self,
        _binding: &p::ContentBinding,
        _subject: &p::SchemaDigest,
    ) -> p::Result<p::SchemaDigest> {
        Err(p::Error(
            "content resolver does not support subject keys".into(),
        ))
    }
}

#[derive(Clone)]
pub struct ProtectedContentService {
    keys: Arc<dyn ContentKeyResolver>,
}

impl ProtectedContentService {
    pub fn new(keys: Arc<dyn ContentKeyResolver>) -> Self {
        Self { keys }
    }

    pub fn encrypt(
        &self,
        binding: &p::ContentBinding,
        key_digest: &p::SchemaDigest,
        plaintext: &[u8],
    ) -> p::Result<p::EncryptedContentEnvelope> {
        binding.validate()?;
        p::sha256_digest_bytes(key_digest)
            .map_err(|_| p::Error("content key digest is not canonical".into()))?;
        let key = self.keys.resolve(key_digest)?;
        validate_key(&key, key_digest)?;

        let mut nonce_bytes = vec![0_u8; p::CHACHA20_POLY1305_NONCE_BYTES];
        getrandom::fill(&mut nonce_bytes)
            .map_err(|error| p::Error(format!("content nonce generation failed: {error}")))?;
        let algorithm = p::ContentEncryptionAlgorithm::ChaCha20Poly1305;
        let aad_digest = binding.aad_digest(algorithm, key_digest)?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| p::Error("content encryption key length is invalid".into()))?;
        let nonce = Nonce::try_from(nonce_bytes.as_slice())
            .map_err(|_| p::Error("content encryption nonce has invalid length".into()))?;
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: aad_digest.0.as_bytes(),
                },
            )
            .map_err(|_| p::Error("content encryption failed".into()))?;
        Ok(p::EncryptedContentEnvelope {
            schema_version: p::CONTENT_PROTECTION_SCHEMA_VERSION,
            algorithm,
            owner_digest: binding.owner_digest.clone(),
            scope: binding.scope.clone(),
            object: binding.object.clone(),
            key_digest: key_digest.clone(),
            nonce: nonce_bytes,
            ciphertext,
            aad_digest,
        })
    }

    pub fn decrypt(
        &self,
        binding: &p::ContentBinding,
        envelope: &p::EncryptedContentEnvelope,
    ) -> p::Result<Vec<u8>> {
        envelope.validate()?;
        binding.validate()?;
        if envelope.binding() != *binding {
            return Err(p::Error("encrypted content binding mismatch".into()));
        }
        let expected_aad = binding.aad_digest(envelope.algorithm, &envelope.key_digest)?;
        if expected_aad != envelope.aad_digest {
            return Err(p::Error("encrypted content AAD binding mismatch".into()));
        }
        let key = self.keys.resolve(&envelope.key_digest)?;
        validate_key(&key, &envelope.key_digest)?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| p::Error("content decryption key length is invalid".into()))?;
        let nonce = Nonce::try_from(envelope.nonce.as_slice())
            .map_err(|_| p::Error("content decryption nonce has invalid length".into()))?;
        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: envelope.ciphertext.as_ref(),
                    aad: expected_aad.0.as_bytes(),
                },
            )
            .map_err(|_| p::Error("content decryption failed".into()))
    }

    pub fn destroy_key(&self, key_digest: &p::SchemaDigest) -> p::Result<()> {
        p::sha256_digest_bytes(key_digest)
            .map_err(|_| p::Error("content key digest is not canonical".into()))?;
        self.keys.destroy(key_digest)
    }

    pub(crate) fn ensure_key_available(&self, key_digest: &p::SchemaDigest) -> p::Result<()> {
        let key = self.keys.resolve(key_digest)?;
        validate_key(&key, key_digest)
    }
}

fn validate_key(key: &[u8], expected_digest: &p::SchemaDigest) -> p::Result<()> {
    if key.len() != p::CHACHA20_POLY1305_KEY_BYTES {
        return Err(p::Error("content key has invalid length".into()));
    }
    if p::sha256_content_digest(key) != *expected_digest {
        return Err(p::Error(
            "content key digest does not match resolver output".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct FixtureKeys {
        values: Mutex<BTreeMap<String, Vec<u8>>>,
    }

    impl ContentKeyResolver for FixtureKeys {
        fn resolve(&self, digest: &p::SchemaDigest) -> p::Result<Vec<u8>> {
            self.values
                .lock()
                .unwrap()
                .get(&digest.0)
                .cloned()
                .ok_or_else(|| p::Error("content key is unavailable".into()))
        }

        fn destroy(&self, digest: &p::SchemaDigest) -> p::Result<()> {
            self.values.lock().unwrap().remove(&digest.0);
            Ok(())
        }
    }

    fn binding() -> p::ContentBinding {
        p::ContentBinding {
            schema_version: p::CONTENT_PROTECTION_SCHEMA_VERSION,
            owner_digest: p::sha256_content_digest(b"owner"),
            scope: p::Scope("workspace:alpha".into()),
            object: p::ObjectRef("content:alpha".into()),
        }
    }

    fn service() -> (ProtectedContentService, Arc<FixtureKeys>, p::SchemaDigest) {
        let keys = Arc::new(FixtureKeys::default());
        let raw_key = vec![0x41; p::CHACHA20_POLY1305_KEY_BYTES];
        let digest = p::sha256_content_digest(&raw_key);
        keys.values
            .lock()
            .unwrap()
            .insert(digest.0.clone(), raw_key);
        (ProtectedContentService::new(keys.clone()), keys, digest)
    }

    #[test]
    fn encrypt_decrypt_binds_owner_scope_object_and_schema() {
        let (service, _keys, digest) = service();
        let binding = binding();
        let envelope = service
            .encrypt(&binding, &digest, b"private cognitive content")
            .unwrap();
        assert_eq!(
            service.decrypt(&binding, &envelope).unwrap(),
            b"private cognitive content"
        );

        let mut wrong_scope = binding.clone();
        wrong_scope.scope = p::Scope("workspace:other".into());
        assert!(service.decrypt(&wrong_scope, &envelope).is_err());
    }

    #[test]
    fn wrong_key_missing_key_and_tamper_fail_closed() {
        let (service, keys, digest) = service();
        let binding = binding();
        let mut envelope = service.encrypt(&binding, &digest, b"secret").unwrap();

        envelope.ciphertext[0] ^= 1;
        assert!(service.decrypt(&binding, &envelope).is_err());

        let envelope = service.encrypt(&binding, &digest, b"secret").unwrap();
        let wrong = p::SchemaDigest(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        );
        keys.values
            .lock()
            .unwrap()
            .insert(wrong.0.clone(), vec![0x42; 32]);
        let mut changed = envelope;
        changed.key_digest = wrong;
        assert!(service.decrypt(&binding, &changed).is_err());

        keys.values.lock().unwrap().clear();
        assert!(service.decrypt(&binding, &changed).is_err());
    }

    #[test]
    fn destroying_key_makes_existing_envelope_unreadable() {
        let (service, _keys, digest) = service();
        let binding = binding();
        let envelope = service
            .encrypt(&binding, &digest, b"to be shredded")
            .unwrap();
        service.destroy_key(&digest).unwrap();
        assert!(service.decrypt(&binding, &envelope).is_err());
    }

    #[test]
    fn rebinding_and_recomputing_public_digest_cannot_forge_authentication() {
        let (service, _keys, digest) = service();
        let envelope = service
            .encrypt(&binding(), &digest, b"private content")
            .unwrap();
        for field in 0..3 {
            let mut forged = envelope.clone();
            match field {
                0 => forged.owner_digest = p::sha256_content_digest(b"another owner"),
                1 => forged.scope = p::Scope("workspace:beta".into()),
                _ => forged.object = p::ObjectRef("content:another".into()),
            }
            let binding = forged.binding();
            forged.aad_digest = binding
                .aad_digest(forged.algorithm, &forged.key_digest)
                .unwrap();
            assert!(service.decrypt(&binding, &forged).is_err());
        }
    }
}
