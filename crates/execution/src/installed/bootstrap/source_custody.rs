//! Safe product-source file custody using the existing audited OS boundary.
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

use forme_protocol as p;

use crate::installed::management_repair::storage::ReadCustody;

/// Retains read handles and rejects links/aliases before offline storage access.
/// This is filesystem custody, never proof of a valid brain or a claim grant.
#[derive(Default)]
pub struct BootstrapSourceCustody {
    inner: ReadCustody,
    fences: Vec<(PathBuf, File)>,
}

impl BootstrapSourceCustody {
    /// Bounded read of a configuration/key file. Bytes must never be logged.
    pub fn read(&mut self, path: &Path, limit: u64) -> p::Result<Vec<u8>> {
        self.inner.read(path, limit)
    }

    /// Pin a database for alias checks; Store separately owns the writer fence.
    pub fn pin(&mut self, path: &Path, limit: u64) -> p::Result<u64> {
        self.inner.pin(path, limit)
    }

    pub fn recheck(&self) -> p::Result<()> {
        self.inner.recheck_records()?;
        for (path, file) in &self.fences {
            check_fence(path, file)?;
        }
        Ok(())
    }

    /// An existing access fence must remain writable by Store's own locking
    /// handle. Pin identity/links without taking over that handle or creating it.
    pub fn pin_existing_fence(&mut self, path: &Path) -> p::Result<()> {
        crate::installed::reject_linked_ancestors(path)?;
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(invalid()),
            Ok(_) => {}
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(3);
        }
        let file = options.open(path).map_err(|_| invalid())?;
        check_fence(path, &file)?;
        self.fences.push((path.to_path_buf(), file));
        Ok(())
    }
}

fn check_fence(path: &Path, file: &File) -> p::Result<()> {
    crate::installed::reject_linked_ancestors(path)?;
    let metadata = file.metadata().map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err(invalid());
    }
    #[cfg(windows)]
    if crate::installed::windows_appcontainer_acl::inspect_file(file)?.1 != 1 {
        return Err(invalid());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let current = std::fs::metadata(path).map_err(|_| invalid())?;
        if metadata.nlink() != 1
            || metadata.dev() != current.dev()
            || metadata.ino() != current.ino()
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn invalid() -> p::Error {
    p::Error("bootstrap source fence is invalid or aliased".into())
}
