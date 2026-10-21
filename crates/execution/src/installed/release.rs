use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use ed25519_dalek::{Signature, VerifyingKey};
use forme_protocol as p;
use sha2::{Digest, Sha256};

use super::reject_linked_ancestors;

/// Populated only from the administrator-owned distribution key configuration.
/// Package contents and download responses must never add trusted keys.
pub struct ReleaseKeyring {
    keys: BTreeMap<String, VerifyingKey>,
}

impl ReleaseKeyring {
    pub fn from_pinned_keys(keys: BTreeMap<String, [u8; 32]>) -> p::Result<Self> {
        if keys.is_empty() || keys.len() > 32 {
            return Err(p::Error(
                "release trust roots are missing or unbounded".into(),
            ));
        }
        let mut trusted = BTreeMap::new();
        for (id, bytes) in keys {
            p::validate_product_id(&id)?;
            let key = VerifyingKey::from_bytes(&bytes)
                .map_err(|_| p::Error("invalid release verification key".into()))?;
            trusted.insert(id, key);
        }
        Ok(Self { keys: trusted })
    }

    pub fn verify(
        &self,
        manifest: &p::ReleaseManifest,
        signature: &[u8],
        root: &Path,
        target: p::ProductTarget,
    ) -> p::Result<VerifiedRelease> {
        manifest.validate()?;
        if manifest.target != target {
            return Err(p::Error("release target is incompatible".into()));
        }
        let key = self
            .keys
            .get(&manifest.signature_ref)
            .ok_or_else(|| p::Error("release signing identity is not pinned".into()))?;
        let signature = Signature::from_slice(signature).map_err(|_| signature_error())?;
        let digest = manifest.digest()?;
        key.verify_strict(digest.0.as_bytes(), &signature)
            .map_err(|_| signature_error())?;
        verify_release_content(manifest, root)?;
        Ok(VerifiedRelease {
            manifest: manifest.clone(),
            digest,
        })
    }
}

pub struct VerifiedRelease {
    manifest: p::ReleaseManifest,
    digest: p::SchemaDigest,
}

impl VerifiedRelease {
    pub fn manifest(&self) -> &p::ReleaseManifest {
        &self.manifest
    }
    pub fn digest(&self) -> &p::SchemaDigest {
        &self.digest
    }
    /// Repeat immediately before a version switch under the installation lock.
    pub fn recheck(&self, root: &Path) -> p::Result<()> {
        verify_release_content(&self.manifest, root)
    }
}

pub fn file_digest(path: &Path) -> p::Result<p::SchemaDigest> {
    reject_linked_ancestors(path)?;
    let file = File::open(path).map_err(|_| content_error())?;
    let metadata = file.metadata().map_err(|_| content_error())?;
    if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 * 1024 {
        return Err(content_error());
    }
    let mut hash = Sha256::new();
    let mut reader = file.take(metadata.len().checked_add(1).ok_or_else(content_error)?);
    let mut buffer = [0_u8; 64 * 1024];
    let mut read = 0_u64;
    loop {
        let n = reader.read(&mut buffer).map_err(|_| content_error())?;
        if n == 0 {
            break;
        }
        read += n as u64;
        hash.update(&buffer[..n]);
    }
    if read != metadata.len() {
        return Err(content_error());
    }
    Ok(p::SchemaDigest(format!("sha256:{:x}", hash.finalize())))
}

fn verify_release_content(manifest: &p::ReleaseManifest, root: &Path) -> p::Result<()> {
    reject_linked_ancestors(root)?;
    if !root.is_dir() {
        return Err(content_error());
    }
    let mut actual = BTreeSet::new();
    collect_files(root, root, &mut actual, 0)?;
    let expected: BTreeSet<_> = manifest
        .files
        .iter()
        .map(|f| f.relative_path.clone())
        .collect();
    if actual != expected {
        return Err(p::Error(
            "release files differ from the signed closed set".into(),
        ));
    }
    let mut ui_files = Vec::new();
    for file in &manifest.files {
        let path = root.join(&file.relative_path);
        reject_linked_ancestors(&path)?;
        if fs::metadata(&path).map_err(|_| content_error())?.len() != file.size
            || file_digest(&path)? != file.digest
        {
            return Err(content_error());
        }
        if file.relative_path.starts_with("web/") {
            ui_files.push(file.clone());
        }
    }
    ui_files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    if ui_files.is_empty() || p::canonical_digest(&ui_files)? != manifest.ui_digest {
        return Err(p::Error(
            "static UI is not bound to the signed release".into(),
        ));
    }
    let suffix = if manifest.target == p::ProductTarget::WindowsX86_64 {
        ".exe"
    } else {
        ""
    };
    for binary in [
        "forme-hostd",
        "forme-webd",
        "forme-gatewayd",
        "forme-cli",
        "forme-hostctl",
    ] {
        if !expected.contains(&format!("bin/{binary}{suffix}")) {
            return Err(p::Error(
                "release is missing a required product entry point".into(),
            ));
        }
    }
    Ok(())
}

fn collect_files(
    root: &Path,
    current: &Path,
    result: &mut BTreeSet<String>,
    depth: usize,
) -> p::Result<()> {
    if depth > 16 {
        return Err(content_error());
    }
    for entry in fs::read_dir(current).map_err(|_| content_error())? {
        let path = entry.map_err(|_| content_error())?.path();
        reject_linked_ancestors(&path)?;
        let kind = fs::symlink_metadata(&path)
            .map_err(|_| content_error())?
            .file_type();
        if kind.is_dir() {
            collect_files(root, &path, result, depth + 1)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| content_error())?
                .to_str()
                .ok_or_else(content_error)?
                .replace('\\', "/");
            p::validate_release_relative_path(&relative)?;
            if result.len() >= 10_000 || !result.insert(relative) {
                return Err(content_error());
            }
        } else {
            return Err(content_error());
        }
    }
    Ok(())
}
fn signature_error() -> p::Error {
    p::Error("release signature verification failed".into())
}
fn content_error() -> p::Error {
    p::Error("release content verification failed".into())
}
