//! Local execution-data keys; never derive a data key from an API credential.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use forme_protocol as p;
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"FRMRES01";
const NONCE_BYTES: usize = 12;
pub(super) const ENVELOPE_OVERHEAD: u64 = 8 + NONCE_BYTES as u64 + 16;

pub(super) struct ResultKeys {
    root: PathBuf,
}

impl ResultKeys {
    pub(super) fn open(root: &Path) -> p::Result<Self> {
        super::reject_linked_ancestors(root)?;
        let mut directory = fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory.create(root).map_err(|_| unavailable())?;
        super::reject_linked_ancestors(root)?;
        super::set_private_directory(root)?;
        Ok(Self { root: root.into() })
    }

    pub(super) fn seal(
        &self,
        manifest: &p::WorkerResultManifest,
        bytes: &[u8],
    ) -> p::Result<Vec<u8>> {
        manifest.validate()?;
        let path = self.key_path(manifest)?;
        let mut key = Zeroizing::new([0_u8; 32]);
        getrandom::fill(key.as_mut()).map_err(|_| unavailable())?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => file
                .write_all(key.as_ref())
                .and_then(|()| file.sync_all())
                .map_err(|_| unavailable())?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                key = self.resolve(manifest)?;
            }
            Err(_) => return Err(unavailable()),
        }
        let mut nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|_| unavailable())?;
        let cipher = ChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| unavailable())?;
        let nonce_value = Nonce::try_from(nonce.as_slice()).map_err(|_| unavailable())?;
        let ciphertext = cipher
            .encrypt(
                &nonce_value,
                Payload {
                    msg: bytes,
                    aad: &aad(manifest)?,
                },
            )
            .map_err(|_| unavailable())?;
        let mut envelope = Vec::with_capacity(ENVELOPE_OVERHEAD as usize + bytes.len());
        envelope.extend_from_slice(MAGIC);
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&ciphertext);
        Ok(envelope)
    }

    pub(super) fn decrypt(
        &self,
        manifest: &p::WorkerResultManifest,
        envelope: &[u8],
    ) -> p::Result<Zeroizing<Vec<u8>>> {
        manifest.validate()?;
        if envelope.len() as u64 != manifest.bytes + ENVELOPE_OVERHEAD
            || !envelope.starts_with(MAGIC)
        {
            return Err(unavailable());
        }
        let key = self.resolve(manifest)?;
        let cipher = ChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| unavailable())?;
        let nonce = Nonce::try_from(&envelope[8..8 + NONCE_BYTES]).map_err(|_| unavailable())?;
        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &envelope[8 + NONCE_BYTES..],
                    aad: &aad(manifest)?,
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| unavailable())
    }

    fn key_path(&self, manifest: &p::WorkerResultManifest) -> p::Result<PathBuf> {
        p::validate_product_digest(&manifest.manifest_digest)?;
        let suffix = manifest.manifest_digest.0.trim_start_matches("sha256:");
        let path = self.root.join(format!("{suffix}.key"));
        super::reject_linked_ancestors(&path)?;
        Ok(path)
    }

    fn resolve(&self, manifest: &p::WorkerResultManifest) -> p::Result<Zeroizing<[u8; 32]>> {
        let path = self.key_path(manifest)?;
        let metadata = fs::symlink_metadata(&path).map_err(|_| unavailable())?;
        if !metadata.is_file() || metadata.len() != 32 {
            return Err(unavailable());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(unavailable());
            }
        }
        let mut key = Zeroizing::new([0_u8; 32]);
        File::open(path)
            .and_then(|mut file| file.read_exact(key.as_mut()))
            .map_err(|_| unavailable())?;
        Ok(key)
    }
}

fn aad(manifest: &p::WorkerResultManifest) -> p::Result<Vec<u8>> {
    let digest = p::canonical_digest(&(
        "forme-host-worker-result-envelope-v1",
        &manifest.manifest_digest,
    ))?;
    Ok(digest.0.into_bytes())
}

fn unavailable() -> p::Error {
    p::Error("worker result protection is unavailable or unverifiable".into())
}
