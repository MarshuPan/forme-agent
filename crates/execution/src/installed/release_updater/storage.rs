use super::*;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
};

pub(super) fn read<T: DeserializeOwned>(path: &Path) -> p::Result<T> {
    super::super::reject_linked_ancestors(path)?;
    let mut bytes = Vec::new();
    let file = File::open(path).map_err(|_| invalid())?;
    if !file.metadata().map_err(|_| invalid())?.is_file() {
        return Err(invalid());
    }
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > 65536 {
        return Err(invalid());
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}
pub(super) fn publish(path: &Path, value: &impl Serialize) -> p::Result<()> {
    super::super::reject_linked_ancestors(path)?;
    let parent = path.parent().ok_or_else(invalid)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| invalid())?;
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    file.write_all(&bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| invalid())?;
    file.persist(path).map_err(|_| invalid())?;
    sync(parent)
}
pub(super) fn claim(
    updater: &InstalledReleaseUpdater,
    takeover: &p::ProductReleaseTakeover,
) -> p::Result<()> {
    let name = p::canonical_digest(&(
        "release-dispatch-v1",
        &takeover.release.operation_id,
        &takeover.release.command_digest,
    ))?;
    let path = updater
        .installation
        .paths
        .host_state
        .join(format!("release-dispatch-{}.claim", &name.0[7..]));
    super::super::reject_linked_ancestors(&path)?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(|_| invalid())?;
    let bytes = serde_json::to_vec(takeover).map_err(|_| invalid())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| invalid())?;
    sync(path.parent().ok_or_else(invalid)?)
}
fn sync(path: &Path) -> p::Result<()> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| invalid())?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
