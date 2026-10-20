use std::fs;
use std::path::{Component, Path, PathBuf};

use forme_protocol as p;

/// Validate every existing ancestor without following a link or reparse point.
/// This is a preflight; privileged effects must recheck immediately before use.
pub fn reject_linked_ancestors(path: &Path) -> p::Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
    {
        return Err(p::Error("host path is not absolute and normalized".into()));
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) => {
                if meta.file_type().is_symlink() || is_reparse_point(&meta) {
                    return Err(p::Error("host path contains filesystem indirection".into()));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(p::Error("host path cannot be inspected".into())),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x400 != 0
}
#[cfg(not(windows))]
fn is_reparse_point(_: &fs::Metadata) -> bool {
    false
}

pub fn validate_installation_paths(manifest: &p::InstallationManifest) -> p::Result<()> {
    manifest.validate()?;
    let paths = &manifest.paths;
    for path in [
        &paths.releases,
        &paths.config,
        &paths.brain,
        &paths.web_auth,
        &paths.host_state,
        &paths.keys,
        &paths.logs,
        &paths.backups,
        &paths.workspaces,
        &paths.ipc,
    ] {
        reject_linked_ancestors(path)?;
    }
    Ok(())
}
