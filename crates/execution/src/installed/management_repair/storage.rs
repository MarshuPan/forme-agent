//! Bounded read custody for the administrator-owned repair layout.
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use forme_protocol as p;
use serde::de::DeserializeOwned;

use super::invalid;
use crate::installed::reject_linked_ancestors;

#[derive(Default)]
pub(crate) struct ReadCustody {
    directories: BTreeSet<PathBuf>,
    files: Vec<File>,
    records: Vec<(PathBuf, p::SchemaDigest)>,
    exclusive_records: Vec<(PathBuf, usize, u64, p::SchemaDigest)>,
}

impl ReadCustody {
    pub fn pin(&mut self, path: &Path, limit: u64) -> p::Result<u64> {
        reject_linked_ancestors(path)?;
        let mut ancestors: Vec<_> = path
            .parent()
            .ok_or_else(invalid)?
            .ancestors()
            .filter(|path| path.has_root())
            .collect();
        ancestors.reverse();
        for directory in ancestors {
            if self.directories.insert(directory.to_path_buf()) {
                self.files.push(open(directory, true)?);
            }
        }
        let file = open(path, false)?;
        let length = file.metadata().map_err(|_| invalid())?.len();
        if length > limit {
            return Err(invalid());
        }
        self.files.push(file);
        Ok(length)
    }

    pub fn read(&mut self, path: &Path, limit: u64) -> p::Result<Vec<u8>> {
        let length = self.pin(path, limit)?;
        self.read_pinned(path, length, limit, false)
    }

    /// Inspect an existing original while retaining its OS lock until custody
    /// drops. A live writer is an error, never a reason to read a loose copy.
    pub fn read_exclusive(&mut self, path: &Path, limit: u64) -> p::Result<Vec<u8>> {
        let length = self.pin(path, limit)?;
        self.files
            .last()
            .ok_or_else(invalid)?
            .try_lock()
            .map_err(|_| invalid())?;
        self.read_pinned(path, length, limit, true)
    }

    fn read_pinned(
        &mut self,
        path: &Path,
        length: u64,
        limit: u64,
        exclusive: bool,
    ) -> p::Result<Vec<u8>> {
        let file = self.files.last().ok_or_else(invalid)?;
        let mut bytes = Vec::new();
        file.take(limit.checked_add(1).ok_or_else(invalid)?)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() as u64 != length {
            return Err(invalid());
        }
        let digest = p::sha256_content_digest(&bytes);
        if exclusive {
            self.exclusive_records
                .push((path.to_path_buf(), self.files.len() - 1, length, digest));
        } else {
            self.records.push((path.to_path_buf(), digest));
        }
        Ok(bytes)
    }

    pub fn json<T: DeserializeOwned>(&mut self, path: &Path, limit: u64) -> p::Result<T> {
        serde_json::from_slice(&self.read(path, limit)?).map_err(|_| invalid())
    }

    pub fn recheck_records(&self) -> p::Result<()> {
        for (path, digest) in &self.records {
            if crate::installed::file_digest(path)? != *digest {
                return Err(invalid());
            }
        }
        for (path, index, length, digest) in &self.exclusive_records {
            let mut file = self.files.get(*index).ok_or_else(invalid)?;
            reject_linked_ancestors(path)?;
            let current = fs::symlink_metadata(path).map_err(|_| invalid())?;
            let pinned = file.metadata().map_err(|_| invalid())?;
            if !current.is_file() || current.len() != *length || pinned.len() != *length {
                return Err(invalid());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if current.dev() != pinned.dev()
                    || current.ino() != pinned.ino()
                    || pinned.nlink() != 1
                {
                    return Err(invalid());
                }
            }
            #[cfg(windows)]
            if crate::installed::windows_appcontainer_acl::inspect_file(file)?.1 != 1 {
                return Err(invalid());
            }
            let position = file.stream_position().map_err(|_| invalid())?;
            let result = (|| {
                if file.metadata().map_err(|_| invalid())?.len() != *length {
                    return Err(invalid());
                }
                file.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
                let mut bytes = Vec::new();
                (&mut file)
                    .take(length.checked_add(1).ok_or_else(invalid)?)
                    .read_to_end(&mut bytes)
                    .map_err(|_| invalid())?;
                if bytes.len() as u64 != *length || p::sha256_content_digest(&bytes) != *digest {
                    return Err(invalid());
                }
                Ok(())
            })();
            file.seek(SeekFrom::Start(position))
                .map_err(|_| invalid())?;
            result?;
        }
        Ok(())
    }
}

fn open(path: &Path, directory: bool) -> p::Result<File> {
    reject_linked_ancestors(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ | if directory { FILE_SHARE_WRITE } else { 0 })
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    }
    let file = options.open(path).map_err(|_| invalid())?;
    let metadata = file.metadata().map_err(|_| invalid())?;
    if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
        return Err(invalid());
    }
    #[cfg(windows)]
    {
        let (_, links) = crate::installed::windows_appcontainer_acl::inspect_file(&file)?;
        if !directory && links != 1 {
            return Err(invalid());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if !directory && metadata.nlink() != 1 {
            return Err(invalid());
        }
    }
    reject_linked_ancestors(path)?;
    Ok(file)
}

pub(in crate::installed) fn closed_directory(path: &Path, expected: &[&str]) -> p::Result<()> {
    reject_linked_ancestors(path)?;
    let mut names = BTreeSet::new();
    for (count, entry) in fs::read_dir(path).map_err(|_| invalid())?.enumerate() {
        if count >= expected.len() {
            return Err(invalid());
        }
        let entry = entry.map_err(|_| invalid())?;
        reject_linked_ancestors(&entry.path())?;
        names.insert(entry.file_name().into_string().map_err(|_| invalid())?);
    }
    if names != expected.iter().map(|name| (*name).to_owned()).collect() {
        return Err(invalid());
    }
    Ok(())
}
