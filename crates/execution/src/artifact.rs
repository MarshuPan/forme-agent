use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use forme_protocol as p;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactRecord {
    pub schema_version: p::SchemaVersion,
    pub content_ref: p::ContentRef,
    pub digest: p::SchemaDigest,
    pub bytes: u64,
}

pub trait ArtifactStore: Send + Sync {
    fn write(&self, scope: &p::Scope, extension: &str, bytes: &[u8]) -> p::Result<ArtifactRecord>;
}

#[derive(Debug, Clone)]
pub struct FileArtifactStore {
    root: PathBuf,
}

impl FileArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> p::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)
            .map_err(|_| p::Error("artifact root cannot be created".into()))?;
        let root = root
            .canonicalize()
            .map_err(|_| p::Error("artifact root cannot be canonicalized".into()))?;
        if !root.is_dir() {
            return Err(p::Error("artifact root is not a directory".into()));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl ArtifactStore for FileArtifactStore {
    fn write(&self, scope: &p::Scope, extension: &str, bytes: &[u8]) -> p::Result<ArtifactRecord> {
        if scope.0.trim().is_empty()
            || bytes.is_empty()
            || extension.is_empty()
            || extension.len() > 12
            || !extension
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
        {
            return Err(p::Error("artifact write boundary is invalid".into()));
        }
        let scope_key = sha256_hex(scope.0.as_bytes());
        let digest_hex = sha256_hex(bytes);
        let directory = self.root.join(&scope_key[..16]);
        fs::create_dir_all(&directory)
            .map_err(|_| p::Error("artifact scope cannot be created".into()))?;
        let directory = directory
            .canonicalize()
            .map_err(|_| p::Error("artifact scope cannot be canonicalized".into()))?;
        if !directory.starts_with(&self.root) {
            return Err(p::Error(
                "artifact scope escapes the configured root".into(),
            ));
        }
        let path = directory.join(format!("{digest_hex}.{extension}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|_| p::Error("artifact bytes cannot be persisted".into()))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = fs::read(&path)
                    .map_err(|_| p::Error("existing artifact cannot be verified".into()))?;
                if existing != bytes {
                    return Err(p::Error("artifact digest collision was detected".into()));
                }
            }
            Err(_) => return Err(p::Error("artifact file cannot be created".into())),
        }
        Ok(ArtifactRecord {
            schema_version: p::SchemaVersion(1),
            content_ref: p::ContentRef(format!("artifact:sha256:{digest_hex}")),
            digest: p::SchemaDigest(format!("sha256:{digest_hex}")),
            bytes: bytes.len() as u64,
        })
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut value = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_store_hashes_scope_and_content_without_path_injection() {
        let root = std::env::temp_dir().join(format!(
            "forme-artifact-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = FileArtifactStore::new(&root).unwrap();
        let record = store
            .write(
                &p::Scope("../../owner/private".into()),
                "txt",
                b"bounded artifact",
            )
            .unwrap();
        assert!(record.content_ref.0.starts_with("artifact:sha256:"));
        assert!(record.digest.0.starts_with("sha256:"));
        assert_eq!(record.bytes, 16);
        assert_eq!(std::fs::read_dir(store.root()).unwrap().count(), 1);
        assert!(store
            .write(&p::Scope("workspace:m2".into()), "../key", b"secret")
            .is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
