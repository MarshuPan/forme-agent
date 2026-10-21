#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_protocol as p;

pub(super) fn write_transaction(
    files: &[(PathBuf, Vec<u8>, u32)],
    manifest_path: &Path,
    manifest: &p::InstallationManifest,
) -> Result<(), String> {
    let mut created = Vec::new();
    let result = (|| {
        for (path, bytes, mode) in files {
            write_new(path, bytes, *mode)?;
            created.push(path.clone());
        }
        let manifest_bytes = serde_json::to_vec_pretty(manifest)
            .map_err(|_| "installation manifest encoding failed".to_owned())?;
        write_new(manifest_path, &manifest_bytes, 0o600)?;
        created.push(manifest_path.to_owned());
        Ok(())
    })();
    if result.is_err() {
        for path in created.into_iter().rev() {
            let _ = fs::remove_file(path);
        }
    }
    result
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent", path.display()))?;
    reject_linked_ancestors(parent)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("cannot commit {}: {error}", path.display()))
}

pub(super) fn json_bytes(value: &serde_json::Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(value).map_err(|_| "configuration encoding failed".into())
}

pub(super) fn random_key() -> Result<[u8; 32], String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "secure random source is unavailable".to_owned())?;
    Ok(bytes)
}

pub(super) fn random_token() -> Result<String, String> {
    let bytes = random_key()?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) fn random_ref(prefix: &str) -> Result<String, String> {
    Ok(format!("{prefix}:{}", random_token()?))
}

pub(super) fn now_ms() -> Result<p::Timestamp, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .map_err(|_| "system clock is before the Unix epoch".into())
}

pub(super) fn absolute(value: String, name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    absolute_path(&path, name)?;
    Ok(path)
}

pub(super) fn absolute_path(path: &Path, name: &str) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(format!("{name} must be an absolute normalized path"));
    }
    reject_linked_ancestors(path)
}

pub(super) fn reject_linked_ancestors(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("product path must be absolute".into());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "product path contains a link: {}",
                    current.display()
                ))
            }
            Ok(metadata) if is_reparse_point(&metadata) => {
                return Err(format!(
                    "product path contains a reparse point: {}",
                    current.display()
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect {}: {error}", current.display())),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_: &fs::Metadata) -> bool {
    false
}

pub(super) fn is_empty_directory(path: &Path) -> Result<bool, String> {
    reject_linked_ancestors(path)?;
    if !path.exists() {
        return Ok(true);
    }
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    Ok(fs::read_dir(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?
        .next()
        .is_none())
}

pub(super) fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    reject_linked_ancestors(path)?;
    let file =
        File::open(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(format!("{} is not a bounded regular file", path.display()));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} exceeds its size limit", path.display()));
    }
    Ok(bytes)
}
