//! File content-key vault; service setup and read-only recovery are distinct.
#![forbid(unsafe_code)]

use crate::ContentKeyResolver;
use forme_protocol as p;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::RwLock,
};

/// Local resolver. The vault must be outside artifact/backup directories.
/// Debug deliberately exposes neither its location nor key material.
pub struct FileContentKeyResolver {
    root: PathBuf,
    index: RwLock<std::collections::BTreeMap<String, PathBuf>>,
    read_only: bool,
}
impl std::fmt::Debug for FileContentKeyResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileContentKeyResolver { [local resolver] }")
    }
}
impl FileContentKeyResolver {
    pub fn new(root: impl AsRef<Path>) -> p::Result<Self> {
        let absolute = if root.as_ref().is_absolute() {
            root.as_ref().to_owned()
        } else {
            std::env::current_dir()
                .map_err(|_| invalid())?
                .join(root.as_ref())
        };
        reject_indirection(&absolute)?;
        fs::create_dir_all(root.as_ref())
            .map_err(|_| p::Error("content key vault could not be created".into()))?;
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|_| p::Error("content key vault is unavailable".into()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(|_| {
                p::Error("content key vault permissions could not be restricted".into())
            })?;
        }
        #[cfg(windows)]
        {
            let user = std::env::var("USERNAME")
                .map_err(|_| p::Error("local key vault owner is unavailable".into()))?;
            let output = std::process::Command::new("icacls")
                .arg(&root)
                .args(["/inheritance:r", "/grant:r", &format!("{user}:(OI)(CI)F")])
                .output()
                .map_err(|_| p::Error("local key vault access could not be restricted".into()))?;
            if !output.status.success() {
                return Err(p::Error("local key vault access restriction failed".into()));
            }
        }
        Self::index(root, false)
    }

    /// Inherit installer-protected service ACLs. Never consult an interactive
    /// username or rewrite Windows security descriptors from the brain process.
    pub fn new_managed(root: impl AsRef<Path>) -> p::Result<Self> {
        let path = root.as_ref();
        reject_indirection(path)?;
        let mut directory = fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory.create(path).map_err(|_| invalid())?;
        Self::index(path.to_owned(), false)
    }

    /// Existing recovery material only. Construction, allocation and destruction
    /// cannot create a replacement key, folder or ACL.
    pub fn open_existing(root: impl AsRef<Path>) -> p::Result<Self> {
        Self::index(root.as_ref().to_owned(), true)
    }

    fn index(root: PathBuf, read_only: bool) -> p::Result<Self> {
        reject_indirection(&root)?;
        if !fs::metadata(&root).map_err(|_| invalid())?.is_dir() {
            return Err(invalid());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(&root)
                .map_err(|_| invalid())?
                .permissions()
                .mode()
                & 0o077
                != 0
            {
                return Err(p::Error(
                    "content key vault permissions are not private".into(),
                ));
            }
        }
        let root = root.canonicalize().map_err(|_| invalid())?;
        let mut index = std::collections::BTreeMap::new();
        for entry in
            fs::read_dir(&root).map_err(|_| p::Error("content key vault cannot be read".into()))?
        {
            let path = entry
                .map_err(|_| p::Error("content key vault entry is unavailable".into()))?
                .path();
            if path.extension().is_some_and(|e| e == "key") {
                let bytes = read_key(&path)?;
                if index.len() >= 1_000_000 {
                    return Err(invalid());
                }
                index.insert(p::sha256_content_digest(&bytes).0, path);
            }
        }
        Ok(Self {
            root,
            index: RwLock::new(index),
            read_only,
        })
    }
}
impl ContentKeyResolver for FileContentKeyResolver {
    fn resolve(&self, digest: &p::SchemaDigest) -> p::Result<Vec<u8>> {
        p::sha256_digest_bytes(digest)?;
        let path = self
            .index
            .read()
            .map_err(|_| p::Error("content key index is unavailable".into()))?
            .get(&digest.0)
            .cloned()
            .ok_or_else(|| p::Error("content key is unavailable".into()))?;
        reject_indirection(&path)?;
        let revoked = path.with_extension("revoked");
        reject_indirection(&revoked)?;
        if revoked.try_exists().map_err(|_| invalid())? {
            return Err(p::Error("content key has been revoked".into()));
        }
        let bytes = read_key(&path)?;
        if p::sha256_content_digest(&bytes) != *digest {
            return Err(invalid());
        }
        Ok(bytes)
    }
    fn subject_key(
        &self,
        binding: &p::ContentBinding,
        subject: &p::SchemaDigest,
    ) -> p::Result<p::SchemaDigest> {
        if self.read_only {
            return Err(p::Error(
                "existing content key resolver is read-only".into(),
            ));
        }
        binding.validate()?;
        p::sha256_digest_bytes(subject)?;
        let name = p::canonical_digest(&(
            binding.owner_digest.clone(),
            binding.scope.clone(),
            subject,
            binding.object.clone(),
        ))?;
        let stem = name
            .0
            .strip_prefix("sha256:")
            .ok_or_else(|| p::Error("subject key identity is invalid".into()))?;
        let path = self.root.join(format!("{stem}.key"));
        reject_indirection(&path)?;
        reject_indirection(&path.with_extension("revoked"))?;
        if self.root.join(format!("{stem}.revoked")).exists() {
            return Err(p::Error("subject content key has been destroyed".into()));
        }
        if !path.exists() {
            let mut key = vec![0; p::CHACHA20_POLY1305_KEY_BYTES];
            getrandom::fill(&mut key)
                .map_err(|_| p::Error("content key generation failed".into()))?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    file.write_all(&key)
                        .and_then(|()| file.sync_all())
                        .map_err(|_| p::Error("content key could not be persisted".into()))?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(p::Error("content key could not be created".into())),
            }
        }
        let bytes = read_key(&path)?;
        let digest = p::sha256_content_digest(&bytes);
        self.index
            .write()
            .map_err(|_| p::Error("content key index is unavailable".into()))?
            .insert(digest.0.clone(), path);
        Ok(digest)
    }
    fn destroy(&self, digest: &p::SchemaDigest) -> p::Result<()> {
        if self.read_only {
            return Err(p::Error(
                "existing content key resolver is read-only".into(),
            ));
        }
        p::sha256_digest_bytes(digest)?;
        let paths = self
            .index
            .read()
            .map_err(|_| p::Error("content key index is unavailable".into()))?
            .get(&digest.0)
            .cloned()
            .into_iter()
            .collect::<Vec<_>>();
        for path in paths {
            if path.extension().is_some_and(|e| e == "key") {
                let bytes = read_key(&path)?;
                if p::sha256_content_digest(&bytes) == *digest {
                    // Persist denial of regeneration before removing key bytes.
                    let revoked = path.with_extension("revoked");
                    reject_indirection(&revoked)?;
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(&revoked)
                        .map_err(|_| {
                            p::Error("content key revocation could not be recorded".into())
                        })?;
                    file.write_all(digest.0.as_bytes())
                        .and_then(|()| file.sync_all())
                        .map_err(|_| {
                            p::Error("content key revocation could not be persisted".into())
                        })?;
                    fs::remove_file(&path)
                        .map_err(|_| p::Error("content key destruction failed".into()))?;
                    self.index
                        .write()
                        .map_err(|_| p::Error("content key index is unavailable".into()))?
                        .remove(&digest.0);
                }
            }
        }
        if self.resolve(digest).is_ok() {
            return Err(p::Error("content key remains available".into()));
        }
        Ok(())
    }
}

fn invalid() -> p::Error {
    p::Error("content key vault is unavailable or invalid".into())
}

fn read_key(path: &Path) -> p::Result<Vec<u8>> {
    reject_indirection(path)?;
    let file = fs::File::open(path).map_err(|_| invalid())?;
    let metadata = file.metadata().map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.len() != p::CHACHA20_POLY1305_KEY_BYTES as u64 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(p::CHACHA20_POLY1305_KEY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() != p::CHACHA20_POLY1305_KEY_BYTES {
        return Err(invalid());
    }
    Ok(bytes)
}

fn reject_indirection(path: &Path) -> p::Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(invalid());
                    }
                }
                if metadata.file_type().is_symlink() {
                    return Err(invalid());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(invalid()),
        }
    }
    Ok(())
}
