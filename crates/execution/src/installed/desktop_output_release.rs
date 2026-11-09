//! Signed output declaration bound to retained, immutable helper bytes.
#![cfg(windows)]
use super::{reject_linked_ancestors, VerifiedStagedRelease};
use forme_protocol as p;
use serde::Deserialize;
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};

const DECLARATION: &str = "share/forme/desktop-output-v2.json";
const HELPER: &str = "bin/forme-executord.exe";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    schema_version: p::SchemaVersion,
    helper_digest: p::SchemaDigest,
    helper_bytes: u64,
}
pub(super) struct DesktopOutputRelease {
    image: PathBuf,
    _custody: Vec<File>,
}
impl DesktopOutputRelease {
    pub(super) fn from_release(release: &VerifiedStagedRelease) -> p::Result<Option<Self>> {
        release.recheck()?;
        let manifest = release.manifest();
        if manifest.target != p::ProductTarget::WindowsX86_64 {
            return Err(invalid());
        }
        let Some(declared) = manifest
            .files
            .iter()
            .find(|file| file.relative_path == DECLARATION)
        else {
            return Ok(None);
        };
        let helper = manifest
            .files
            .iter()
            .find(|file| file.relative_path == HELPER)
            .ok_or_else(invalid)?;
        if declared.size == 0
            || declared.size > 4096
            || helper.size == 0
            || helper.size > 512 * 1024 * 1024
        {
            return Err(invalid());
        }
        let root = release.content_root();
        let image = root.join(HELPER);
        let declaration_path = root.join(DECLARATION);
        let mut custody = hold_ancestors(&image)?;
        custody.extend(hold_ancestors(&declaration_path)?);
        let helper_file = hold_file(&image)?;
        let mut declaration_file = hold_file(&declaration_path)?;
        if helper_file.metadata().map_err(|_| invalid())?.len() != helper.size
            || declaration_file.metadata().map_err(|_| invalid())?.len() != declared.size
        {
            return Err(invalid());
        }
        // Original file and all its ancestors stay held while the shared
        // content verifier reads this path, preventing replacement or writes.
        if super::file_digest(&image)? != helper.digest {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut declaration_file)
            .take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() as u64 != declared.size
            || p::sha256_content_digest(&bytes) != declared.digest
        {
            return Err(invalid());
        }
        let declaration: Declaration = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if declaration.schema_version != p::DESKTOP_OUTPUT_SCHEMA_VERSION
            || declaration.helper_digest != helper.digest
            || declaration.helper_bytes != helper.size
        {
            return Err(invalid());
        }
        let image = image.canonicalize().map_err(|_| invalid())?;
        custody.push(helper_file);
        custody.push(declaration_file);
        // Recheck the original signed receipt after acquiring custody as well.
        release.recheck()?;
        Ok(Some(Self {
            image,
            _custody: custody,
        }))
    }
    pub(super) fn image(&self) -> &Path {
        &self.image
    }
}
fn hold_file(path: &Path) -> p::Result<File> {
    reject_linked_ancestors(path)?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| invalid())?;
    if !file.metadata().map_err(|_| invalid())?.is_file()
        || super::windows_appcontainer_acl::inspect_file(&file)?.1 != 1
    {
        return Err(invalid());
    }
    reject_linked_ancestors(path)?;
    Ok(file)
}
fn hold_ancestors(path: &Path) -> p::Result<Vec<File>> {
    let mut parents: Vec<_> = path.parent().ok_or_else(invalid)?.ancestors().collect();
    parents.reverse();
    let mut held = Vec::new();
    for parent in parents {
        reject_linked_ancestors(parent)?;
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(parent)
            .map_err(|_| invalid())?;
        if !file.metadata().map_err(|_| invalid())?.is_dir() {
            return Err(invalid());
        }
        held.push(file);
        reject_linked_ancestors(parent)?;
    }
    Ok(held)
}
fn invalid() -> p::Error {
    p::Error("desktop output support is not bound to the signed active helper".into())
}
