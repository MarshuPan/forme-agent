//! Bounded private files and atomic publication; no delete-before-replace window.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use forme_protocol as p;
use serde::{de::DeserializeOwned, Serialize};

pub(super) fn invalid() -> p::Error {
    p::Error("managed product configuration requires recovery or is invalid".into())
}

pub(super) fn check(path: &Path) -> p::Result<()> {
    crate::installed::reject_linked_ancestors(path).map_err(|_| invalid())
}

pub(super) fn read(path: &Path, limit: u64) -> p::Result<Vec<u8>> {
    check(path)?;
    let file = File::open(path).map_err(|_| invalid())?;
    let metadata = file.metadata().map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    Ok(bytes)
}

pub(super) fn json<T: DeserializeOwned>(path: &Path, limit: u64) -> p::Result<T> {
    serde_json::from_slice(&read(path, limit)?).map_err(|_| invalid())
}

pub(super) fn encode<T: Serialize>(value: &T) -> p::Result<Vec<u8>> {
    serde_json::to_vec_pretty(value).map_err(|_| invalid())
}

pub(super) fn directory(path: &Path) -> p::Result<()> {
    check(path)?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(|_| invalid())?;
    check(path)
}

pub(super) fn sync_directory(path: &Path) -> p::Result<()> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| invalid())?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(super) fn immutable(path: &Path, bytes: &[u8]) -> p::Result<()> {
    check(path)?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| invalid())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| invalid())?;
    sync_directory(path.parent().ok_or_else(invalid)?)
}

pub(super) fn publish(path: &Path, bytes: &[u8]) -> p::Result<()> {
    check(path)?;
    let parent = path.parent().ok_or_else(invalid)?;
    directory(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|_| invalid())?;
    staged
        .write_all(bytes)
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|_| invalid())?;
    check(path)?;
    // tempfile uses the platform's replace-existing primitive on Windows too.
    staged.persist(path).map_err(|_| invalid())?;
    sync_directory(parent)
}

pub(super) fn secret(path: &Path) -> p::Result<Vec<u8>> {
    let bytes = read(path, 16 * 1024)?;
    let value = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    if value.trim().is_empty() || value.contains(['\r', '\n']) {
        return Err(invalid());
    }
    Ok(bytes)
}
