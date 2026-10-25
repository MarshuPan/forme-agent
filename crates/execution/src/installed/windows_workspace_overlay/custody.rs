//! Handle custody for bounded workspace snapshots and Host writeback.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use forme_protocol as p;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
    FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
};

use super::super::sandbox_lease::SandboxFileIdentity;
use super::super::windows_appcontainer_acl as acl;
use super::{MAX_BYTES, MAX_ENTRIES, MAX_FILE_BYTES};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub schema_version: p::SchemaVersion,
    pub directory: bool,
    pub identity: SandboxFileIdentity,
    pub bytes: u64,
    pub digest: Option<p::SchemaDigest>,
    pub modified: u64,
}

pub(super) struct Tree {
    pub root: PathBuf,
    pub identity: SandboxFileIdentity,
    pub entries: BTreeMap<String, Entry>,
    pub handles: BTreeMap<String, File>,
    _ancestors: Vec<File>,
}

impl Tree {
    pub fn open(root: &Path, writable: &BTreeSet<String>) -> p::Result<Self> {
        let mut ancestors = Vec::new();
        let mut paths = root
            .ancestors()
            .filter(|p| p.has_root())
            .collect::<Vec<_>>();
        paths.reverse();
        for path in paths {
            ancestors.push(open_directory(path, false)?);
        }
        let identity = acl::inspect_file(ancestors.last().ok_or_else(unavailable)?)?.0;
        let mut tree = Self {
            root: root.to_path_buf(),
            identity,
            entries: BTreeMap::new(),
            handles: BTreeMap::new(),
            _ancestors: ancestors,
        };
        let mut bytes = 0;
        tree.walk("", writable, &mut bytes, 0)?;
        Ok(tree)
    }

    fn walk(
        &mut self,
        parent: &str,
        writable: &BTreeSet<String>,
        total: &mut u64,
        depth: u32,
    ) -> p::Result<()> {
        if depth > 32 {
            return Err(unavailable());
        }
        let directory = self.root.join(parent);
        let names = names(&directory)?;
        for name in names {
            if self.entries.len() >= MAX_ENTRIES {
                return Err(unavailable());
            }
            let key = if parent.is_empty() {
                name
            } else {
                format!("{parent}/{name}")
            };
            let path = self.root.join(&key);
            let metadata = fs::symlink_metadata(&path).map_err(|_| unavailable())?;
            let file = if metadata.is_dir() {
                open_directory(&path, writable.contains(&key))?
            } else {
                open_file(&path, writable.contains(&key), false)?
            };
            let entry = inspect(&file)?;
            *total = total.checked_add(entry.bytes).ok_or_else(unavailable)?;
            if *total > MAX_BYTES {
                return Err(unavailable());
            }
            let recurse = entry.directory;
            self.entries.insert(key.clone(), entry);
            self.handles.insert(key.clone(), file);
            if recurse {
                self.walk(&key, writable, total, depth + 1)?;
            }
        }
        Ok(())
    }

    pub fn copy_to(&self, destination: &Path) -> p::Result<()> {
        fs::create_dir(destination).map_err(|_| unavailable())?;
        for (key, entry) in &self.entries {
            let path = destination.join(key);
            if entry.directory {
                fs::create_dir(&path).map_err(|_| unavailable())?;
            } else {
                let mut file = open_file(&path, true, true)?;
                file.write_all(&self.contents(key)?)
                    .map_err(|_| unavailable())?;
                file.sync_all().map_err(|_| unavailable())?;
            }
        }
        Ok(())
    }

    pub fn contents(&self, key: &str) -> p::Result<Vec<u8>> {
        contents(self.handles.get(key).ok_or_else(unavailable)?)
    }

    /// Verify using the same handles, without dropping custody and reopening
    /// mutable paths. Enumeration also detects new/unaccounted entries.
    pub fn verify_final(&self, expected: &BTreeMap<String, Entry>) -> p::Result<()> {
        let mut listed = BTreeSet::new();
        collect_names(&self.root, "", &mut listed, 0)?;
        if listed != self.handles.keys().cloned().collect() {
            return Err(unavailable());
        }
        let mut actual = BTreeMap::new();
        for (key, file) in &self.handles {
            actual.insert(key.clone(), inspect(file)?);
        }
        if semantic(&actual) != semantic(expected) {
            return Err(unavailable());
        }
        Ok(())
    }
}

pub(super) fn inspect(file: &File) -> p::Result<Entry> {
    let (identity, links) = acl::inspect_file(file)?;
    let metadata = file.metadata().map_err(|_| unavailable())?;
    if !(metadata.is_file() || metadata.is_dir())
        || (metadata.is_file() && (links != 1 || metadata.len() > MAX_FILE_BYTES))
    {
        return Err(unavailable());
    }
    let digest = if metadata.is_file() {
        Some(hash(&contents(file)?))
    } else {
        None
    };
    let after = file.metadata().map_err(|_| unavailable())?;
    if after.len() != metadata.len() || after.last_write_time() != metadata.last_write_time() {
        return Err(unavailable());
    }
    Ok(Entry {
        schema_version: p::SchemaVersion(1),
        directory: metadata.is_dir(),
        identity,
        bytes: if metadata.is_dir() { 0 } else { metadata.len() },
        digest,
        modified: metadata.last_write_time(),
    })
}

fn contents(file: &File) -> p::Result<Vec<u8>> {
    let mut stream = file;
    stream.seek(SeekFrom::Start(0)).map_err(|_| unavailable())?;
    let mut bytes = Vec::new();
    stream
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(unavailable());
    }
    Ok(bytes)
}

pub(super) fn hash(bytes: &[u8]) -> p::SchemaDigest {
    p::SchemaDigest(format!("sha256:{:x}", Sha256::digest(bytes)))
}

pub(super) fn semantic(
    entries: &BTreeMap<String, Entry>,
) -> Vec<(&String, bool, u64, &Option<p::SchemaDigest>)> {
    entries
        .iter()
        .map(|(key, e)| (key, e.directory, e.bytes, &e.digest))
        .collect()
}

pub(super) fn open_file(path: &Path, writable: bool, create: bool) -> p::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(writable)
        .create_new(create)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    if writable {
        use windows_sys::Win32::Storage::FileSystem::{FILE_GENERIC_READ, FILE_GENERIC_WRITE};
        options.access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE);
    }
    options.open(path).map_err(|_| unavailable())
}

pub(super) fn open_directory(path: &Path, removable: bool) -> p::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .access_mode(
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | if removable { DELETE } else { 0 },
        )
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| unavailable())?;
    acl::inspect_file(&file)?;
    if !file.metadata().map_err(|_| unavailable())?.is_dir() {
        return Err(unavailable());
    }
    Ok(file)
}

fn names(path: &Path) -> p::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(path)
        .map_err(|_| unavailable())?
        .take(MAX_ENTRIES + 1)
    {
        let entry = entry.map_err(|_| unavailable())?;
        let name = entry.file_name().into_string().map_err(|_| unavailable())?;
        validate_name(&name)?;
        names.push(name);
    }
    if names.len() > MAX_ENTRIES {
        return Err(unavailable());
    }
    names.sort();
    if names
        .iter()
        .map(|n| n.to_lowercase())
        .collect::<BTreeSet<_>>()
        .len()
        != names.len()
    {
        return Err(unavailable());
    }
    Ok(names)
}

pub(super) fn validate_name(name: &str) -> p::Result<()> {
    let stem = name.split('.').next().unwrap_or_default().to_uppercase();
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.ends_with([' ', '.'])
        || name.contains([':', '/', '\\', '\0'])
        || name.chars().any(char::is_control)
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
    {
        return Err(unavailable());
    }
    Ok(())
}

fn collect_names(
    root: &Path,
    parent: &str,
    output: &mut BTreeSet<String>,
    depth: u32,
) -> p::Result<()> {
    if depth > 32 {
        return Err(unavailable());
    }
    for name in names(&root.join(parent))? {
        let key = if parent.is_empty() {
            name
        } else {
            format!("{parent}/{name}")
        };
        if output.len() >= MAX_ENTRIES {
            return Err(unavailable());
        }
        output.insert(key.clone());
        let metadata = fs::symlink_metadata(root.join(&key)).map_err(|_| unavailable())?;
        if metadata.is_symlink() || metadata.file_attributes() & 0x400 != 0 {
            return Err(unavailable());
        }
        if metadata.is_dir() {
            collect_names(root, &key, output, depth + 1)?;
        }
    }
    Ok(())
}

pub(super) fn unavailable() -> p::Error {
    p::Error("workspace snapshot or writeback custody cannot be verified".into())
}
