#![cfg(any(target_os = "linux", target_os = "macos"))]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use forme_protocol as p;
use serde::{de::DeserializeOwned, Serialize};

use super::process::write_private_json;
use super::reject_linked_ancestors;

const MAX_ENTRIES: usize = 1024;
const MAX_DEPTH: usize = 32;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RECORD_BYTES: u64 = 4 * 1024 * 1024;
#[cfg(target_os = "linux")]
const CHOWN: &str = "/usr/bin/chown";
#[cfg(target_os = "macos")]
const CHOWN: &str = "/usr/sbin/chown";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileIdentity {
    pub(super) device: u64,
    pub(super) inode: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub(super) directory: bool,
    pub(super) bytes: u64,
    pub(super) digest: p::SchemaDigest,
    pub(super) identity: FileIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Tree {
    pub(super) root: PathBuf,
    pub(super) identity: FileIdentity,
    pub(super) entries: BTreeMap<String, Entry>,
}

impl Tree {
    pub(super) fn capture(root: &Path) -> p::Result<Self> {
        reject_linked_ancestors(root)?;
        let metadata = fs::symlink_metadata(root)
            .map_err(|_| p::Error("POSIX worker workspace is unavailable".into()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(p::Error("POSIX worker workspace is not a directory".into()));
        }
        let root = fs::canonicalize(root)
            .map_err(|_| p::Error("POSIX worker workspace cannot be resolved".into()))?;
        let identity = file_identity(&root)?;
        let mut entries = BTreeMap::new();
        visit(&root, "", 0, &mut entries)?;
        if entries.values().map(|entry| entry.bytes).sum::<u64>() > MAX_BYTES {
            return Err(p::Error("POSIX worker workspace is too large".into()));
        }
        Ok(Self {
            root,
            identity,
            entries,
        })
    }

    pub(super) fn copy_to(&self, destination: &Path) -> p::Result<()> {
        reject_linked_ancestors(destination)?;
        fs::create_dir(destination)
            .map_err(|_| p::Error("POSIX worker overlay cannot be created".into()))?;
        for (key, entry) in &self.entries {
            let target = destination.join(key);
            if entry.directory {
                fs::create_dir(&target).map_err(|_| {
                    p::Error("POSIX worker overlay directory cannot be created".into())
                })?;
                continue;
            }
            let parent = target
                .parent()
                .ok_or_else(|| p::Error("POSIX worker overlay parent is unavailable".into()))?;
            fs::create_dir_all(parent)
                .map_err(|_| p::Error("POSIX worker overlay parent cannot be created".into()))?;
            let mut source = File::open(self.root.join(key))
                .map_err(|_| p::Error("POSIX worker workspace file cannot be read".into()))?;
            let mut output = private_file(&target)?;
            copy_bounded(&mut source, &mut output, entry.bytes)?;
            output
                .sync_all()
                .map_err(|_| p::Error("POSIX worker overlay file cannot be synchronized".into()))?;
        }
        Ok(())
    }
}

pub(super) fn same_contents(
    left: &BTreeMap<String, Entry>,
    right: &BTreeMap<String, Entry>,
) -> bool {
    left.len() == right.len()
        && left.iter().all(|(key, entry)| {
            right.get(key).is_some_and(|other| {
                entry.directory == other.directory
                    && entry.bytes == other.bytes
                    && entry.digest == other.digest
            })
        })
}

pub(super) fn write_record<T: Serialize>(path: &Path, value: &T) -> p::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| writeback_error())?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(writeback_error());
    }
    write_private_json(path, value)
}

pub(super) fn write_private_bytes(path: &Path, bytes: &[u8]) -> p::Result<()> {
    let mut file = private_file(path)?;
    file.write_all(bytes)
        .map_err(|_| p::Error("POSIX worker private file cannot be written".into()))?;
    file.sync_all()
        .map_err(|_| p::Error("POSIX worker private file cannot be synchronized".into()))
}

pub(super) fn read_record<T: DeserializeOwned>(path: &Path) -> p::Result<T> {
    let mut file = File::open(path).map_err(|_| writeback_error())?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| writeback_error())?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(writeback_error());
    }
    serde_json::from_slice(&bytes).map_err(|_| writeback_error())
}

pub(super) fn private_file(path: &Path) -> p::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    options
        .open(path)
        .map_err(|_| p::Error("POSIX worker private file cannot be created".into()))
}

pub(super) fn copy_bounded(
    source: &mut File,
    destination: &mut File,
    expected: u64,
) -> p::Result<()> {
    let mut limited = source.take(expected + 1);
    let copied = std::io::copy(&mut limited, destination).map_err(|_| writeback_error())?;
    if copied != expected {
        return Err(writeback_error());
    }
    let mut probe = [0_u8; 1];
    if limited.read(&mut probe).map_err(|_| writeback_error())? != 0 {
        return Err(writeback_error());
    }
    Ok(())
}

pub(super) fn set_group_directory(path: &Path, group: &str) -> p::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o750))
        .map_err(|_| p::Error("POSIX worker directory permissions cannot be set".into()))?;
    chown(path, &format!("root:{group}"))
}

pub(super) fn execution_account() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "forme-exec"
    }
    #[cfg(target_os = "macos")]
    {
        "_formeexec"
    }
}

pub(super) fn chown_recursive(path: &Path, owner: &str) -> p::Result<()> {
    chown(path, &format!("{owner}:{owner}"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| p::Error("POSIX worker overlay permissions cannot be set".into()))?;
    for entry in walk_paths(path)? {
        chown(&entry, &format!("{owner}:{owner}"))?;
        let permissions = if entry.is_dir() { 0o700 } else { 0o600 };
        fs::set_permissions(&entry, fs::Permissions::from_mode(permissions))
            .map_err(|_| p::Error("POSIX worker overlay permissions cannot be set".into()))?;
    }
    Ok(())
}

pub(super) fn chown_readonly(path: &Path, owner: &str) -> p::Result<()> {
    chown(path, &format!("{owner}:{owner}"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o400))
        .map_err(|_| p::Error("POSIX worker input permissions cannot be set".into()))
}

fn chown(path: &Path, owner: &str) -> p::Result<()> {
    let status = Command::new(CHOWN)
        .args(["--", owner, path.to_string_lossy().as_ref()])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| p::Error("POSIX worker ownership helper is unavailable".into()))?;
    if status.success() {
        Ok(())
    } else {
        Err(p::Error(
            "POSIX worker ownership helper rejected the path".into(),
        ))
    }
}

fn visit(
    root: &Path,
    prefix: &str,
    depth: usize,
    entries: &mut BTreeMap<String, Entry>,
) -> p::Result<()> {
    if depth > MAX_DEPTH {
        return Err(p::Error("POSIX worker workspace is too deep".into()));
    }
    let mut children = fs::read_dir(root)
        .map_err(|_| p::Error("POSIX worker workspace cannot be listed".into()))?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| p::Error("POSIX worker workspace entry cannot be read".into()))?;
    children.sort();
    for path in children {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| p::Error("POSIX worker workspace name is not valid UTF-8".into()))?;
        let key = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| p::Error("POSIX worker workspace entry cannot be inspected".into()))?;
        if metadata.file_type().is_symlink() || metadata.nlink() != 1 {
            return Err(p::Error(
                "POSIX worker workspace links are not supported".into(),
            ));
        }
        let directory = metadata.is_dir();
        if !directory && !metadata.is_file() {
            return Err(p::Error(
                "POSIX worker workspace contains an unsupported file".into(),
            ));
        }
        let entry = if directory {
            Entry {
                directory: true,
                bytes: 0,
                digest: p::canonical_digest(&("linux-worker-directory-v1", &key))?,
                identity: file_identity(&path)?,
            }
        } else {
            if metadata.len() > MAX_FILE_BYTES {
                return Err(p::Error("POSIX worker workspace file is too large".into()));
            }
            let mut file = File::open(&path)
                .map_err(|_| p::Error("POSIX worker workspace file cannot be opened".into()))?;
            let mut bytes = Vec::new();
            std::io::Read::by_ref(&mut file)
                .take(MAX_FILE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| p::Error("POSIX worker workspace file cannot be read".into()))?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(p::Error("POSIX worker workspace file is too large".into()));
            }
            Entry {
                directory: false,
                bytes: bytes.len() as u64,
                digest: p::canonical_digest(&bytes)?,
                identity: file_identity(&path)?,
            }
        };
        entries.insert(key.clone(), entry);
        if entries.len() > MAX_ENTRIES {
            return Err(p::Error(
                "POSIX worker workspace has too many entries".into(),
            ));
        }
        if directory {
            visit(&path, &key, depth + 1, entries)?;
        }
    }
    Ok(())
}

pub(super) fn file_identity(path: &Path) -> p::Result<FileIdentity> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| p::Error("POSIX worker file identity is unavailable".into()))?;
    if metadata.file_type().is_symlink() || metadata.nlink() != 1 {
        return Err(p::Error("POSIX worker file identity is unsafe".into()));
    }
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

fn walk_paths(root: &Path) -> p::Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).map_err(|_| writeback_error())?;
        if metadata.file_type().is_symlink() {
            return Err(writeback_error());
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).map_err(|_| writeback_error())? {
                pending.push(entry.map_err(|_| writeback_error())?.path());
            }
        }
        if path != root {
            result.push(path);
        }
    }
    result.sort();
    Ok(result)
}

pub(super) fn writeback_error() -> p::Error {
    p::Error("POSIX worker writeback requires recovery; no retry was performed".into())
}
