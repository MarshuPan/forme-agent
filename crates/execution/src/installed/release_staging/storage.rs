use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use forme_protocol as p;
use serde::de::DeserializeOwned;

use crate::installed::{file_digest, reject_linked_ancestors};

pub(super) fn invalid() -> p::Error {
    p::Error("release staging requires recovery or contains invalid files".into())
}

pub(super) fn read(path: &Path, limit: u64) -> p::Result<Vec<u8>> {
    reject_linked_ancestors(path)?;
    let file = File::open(path).map_err(|_| invalid())?;
    let metadata = file.metadata().map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 != metadata.len() {
        return Err(invalid());
    }
    Ok(bytes)
}

pub(super) fn json<T: DeserializeOwned>(path: &Path, limit: u64) -> p::Result<T> {
    serde_json::from_slice(&read(path, limit)?).map_err(|_| invalid())
}

pub(super) fn directory(path: &Path) -> p::Result<()> {
    reject_linked_ancestors(path)?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o755);
    }
    builder.create(path).map_err(|_| invalid())?;
    reject_linked_ancestors(path)
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
    reject_linked_ancestors(path)?;
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

pub(super) fn copy(source: &Path, target: &Path, entry: &p::ReleaseFile) -> p::Result<()> {
    reject_linked_ancestors(source)?;
    reject_linked_ancestors(target)?;
    let input = File::open(source).map_err(|_| invalid())?;
    if !input.metadata().map_err(|_| invalid())?.is_file() {
        return Err(invalid());
    }
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if entry.relative_path.starts_with("bin/") {
            0o755
        } else {
            0o644
        });
    }
    let mut output = options.open(target).map_err(|_| invalid())?;
    let copied = std::io::copy(
        &mut input.take(entry.size.checked_add(1).ok_or_else(invalid)?),
        &mut output,
    )
    .map_err(|_| invalid())?;
    output.sync_all().map_err(|_| invalid())?;
    if copied != entry.size || file_digest(target)? != entry.digest {
        return Err(invalid());
    }
    sync_directory(target.parent().ok_or_else(invalid)?)
}
