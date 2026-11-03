//! Installer-owned setup configuration and registered new-brain target checks.
use std::io::Read;
use std::path::{Path, PathBuf};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::{BootstrapError, BootstrapResult};
use p::BootstrapProblemCode as Code;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapAuthorityConfig {
    pub schema_version: p::SchemaVersion,
    pub install_ref: String,
    pub journal_path: PathBuf,
    pub signing_key_path: PathBuf,
    pub web_credential_path: PathBuf,
    pub new_brain_directory: PathBuf,
    pub candidate_ref: String,
    /// Installer-derived root for a historical forme brain. This is optional
    /// for schema-1 installations created before legacy bootstrap support.
    #[serde(default)]
    pub historical_brain_directory: Option<PathBuf>,
    /// Installer-derived root containing verified backup candidates. This is
    /// optional for schema-1 installations created before legacy bootstrap.
    #[serde(default)]
    pub verified_backup_directory: Option<PathBuf>,
}

impl BootstrapAuthorityConfig {
    pub fn load(path: &Path) -> BootstrapResult<Self> {
        let bytes = read_bounded(path, 32 * 1024)?;
        let value: Self =
            serde_json::from_slice(&bytes).map_err(|_| BootstrapError(Code::InvalidInput))?;
        value.validate()?;
        for target in value.managed_paths() {
            if overlaps(path, target) {
                return Err(BootstrapError(Code::InvalidInput));
            }
        }
        Ok(value)
    }

    pub fn validate(&self) -> BootstrapResult<()> {
        super::version(self.schema_version)?;
        p::validate_product_id(&self.install_ref)
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        p::validate_product_id(&self.candidate_ref)
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        let paths = self.managed_paths();
        for (index, path) in paths.iter().enumerate() {
            super::super::reject_linked_ancestors(path)
                .map_err(|_| BootstrapError(Code::InvalidInput))?;
            if path.parent().is_none() || !path.parent().is_some_and(|p| p.is_dir()) {
                return Err(BootstrapError(Code::InvalidInput));
            }
            for other in paths.iter().skip(index + 1) {
                if overlaps(path, other) {
                    return Err(BootstrapError(Code::InvalidInput));
                }
            }
        }
        Ok(())
    }

    pub fn reject_protected_paths(&self, paths: &[PathBuf]) -> BootstrapResult<()> {
        for protected in paths {
            super::super::reject_linked_ancestors(protected)
                .map_err(|_| BootstrapError(Code::InvalidInput))?;
            for target in self.managed_paths() {
                if overlaps(protected, target) {
                    return Err(BootstrapError(Code::InvalidInput));
                }
            }
        }
        Ok(())
    }

    pub(super) fn check_empty_target(&self) -> BootstrapResult<()> {
        let path = &self.new_brain_directory;
        super::super::reject_linked_ancestors(path)
            .map_err(|_| BootstrapError(Code::DataUnavailable))?;
        if !path.is_dir() {
            return Err(BootstrapError(Code::DataUnavailable));
        }
        if std::fs::read_dir(path)
            .map_err(|_| BootstrapError(Code::DataUnavailable))?
            .next()
            .is_some()
        {
            // Includes historical data, unknown files and an unreadable entry.
            return Err(BootstrapError(Code::DataUnavailable));
        }
        Ok(())
    }

    /// Paths are configuration facts, never request-supplied paths. Missing
    /// optional roots are treated as an unavailable candidate by discovery.
    pub(super) fn candidate_roots(&self) -> impl Iterator<Item = (&'static str, &Path)> {
        self.historical_brain_directory
            .as_deref()
            .map(|path| ("historical-brain", path))
            .into_iter()
            .chain(
                self.verified_backup_directory
                    .as_deref()
                    .map(|path| ("verified-backups", path)),
            )
    }

    fn managed_paths(&self) -> Vec<&Path> {
        let mut paths = vec![
            self.journal_path.as_path(),
            self.signing_key_path.as_path(),
            self.web_credential_path.as_path(),
            self.new_brain_directory.as_path(),
        ];
        paths.extend(self.candidate_roots().map(|(_, path)| path));
        paths
    }
}

pub(super) fn read_key(path: &Path) -> BootstrapResult<[u8; 32]> {
    read_bounded(path, 32)?
        .try_into()
        .map_err(|_| BootstrapError(Code::RecoveryRequired))
}

pub(super) fn read_credential(path: &Path) -> BootstrapResult<String> {
    let bytes = read_bounded(path, 258)?;
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| BootstrapError(Code::InvalidInput))?
        .trim_end_matches(['\r', '\n']);
    if !(32..=256).contains(&value.len()) || value.chars().any(char::is_whitespace) {
        return Err(BootstrapError(Code::InvalidInput));
    }
    Ok(value.into())
}

fn read_bounded(path: &Path, limit: u64) -> BootstrapResult<Vec<u8>> {
    super::super::reject_linked_ancestors(path).map_err(|_| BootstrapError(Code::InvalidInput))?;
    let file = std::fs::File::open(path).map_err(|_| BootstrapError(Code::Unavailable))?;
    let metadata = file
        .metadata()
        .map_err(|_| BootstrapError(Code::Unavailable))?;
    if !metadata.is_file() {
        return Err(BootstrapError(Code::InvalidInput));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(BootstrapError(Code::InvalidInput));
        }
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| BootstrapError(Code::Unavailable))?;
    if bytes.len() as u64 > limit {
        return Err(BootstrapError(Code::InvalidInput));
    }
    Ok(bytes)
}

fn overlaps(left: &Path, right: &Path) -> bool {
    fn normalized(value: &Path) -> String {
        let text = value.to_string_lossy().replace('\\', "/");
        #[cfg(windows)]
        let text = text.to_ascii_lowercase();
        text.trim_end_matches('/').to_owned()
    }
    let left = normalized(left);
    let right = normalized(right);
    left == right || left.starts_with(&(right.clone() + "/")) || right.starts_with(&(left + "/"))
}
