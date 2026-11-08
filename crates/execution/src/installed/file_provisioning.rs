//! Host-derived installation inputs. Never accepted by the public settings API.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::{reject_linked_ancestors, ManagedWorkerBinding, ManagedWorkerEndpoint};

pub const INSTALLED_FILE_PROFILE: &str = "sandbox:installed-file-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledFileProvisioning {
    schema_version: p::SchemaVersion,
    binding: ManagedWorkerBinding,
    workspace_roots: BTreeMap<String, PathBuf>,
}

impl InstalledFileProvisioning {
    /// Only a verified Windows installation may produce this projection.
    /// This validates placement, not permissions or native service readiness.
    #[cfg(windows)]
    pub fn derive(
        manifest: &p::InstallationManifest,
        config: &super::WorkerRuntimeConfig,
        credential_path: &Path,
    ) -> p::Result<Self> {
        super::validate_installation_paths(manifest)?;
        config.validate()?;
        let roots = BTreeMap::from([(
            "workspace:default".to_owned(),
            manifest.paths.workspaces.join("default"),
        )]);
        if manifest.target != p::ProductTarget::WindowsX86_64
            || config.authority_epoch != p::AuthorityEpoch(1)
            || config.allowed_scopes != [p::Scope("workspace:default".into())]
            || config.workspace_roots != roots
            || config.journal_path != manifest.paths.host_state.join("file-worker.journal")
            || config.runtime_root != manifest.paths.host_state.join("file-worker-runtime")
            || credential_path != manifest.paths.keys.join("agent-execution.token")
            || config.launch_profiles.len() != 1
        {
            return Err(invalid());
        }
        let profile = &config.launch_profiles[0];
        let accepted_programs = [
            manifest.paths.releases.join("bin/forme-executord.exe"),
            manifest.paths.releases.join("bin/forme-file-worker.exe"),
        ];
        if profile.sandbox_profile_ref != INSTALLED_FILE_PROFILE
            || !accepted_programs.contains(&profile.program)
            || profile.args != ["--installed-file-worker"]
        {
            return Err(invalid());
        }
        config.validate_protected_paths(&[
            &manifest.paths.brain,
            &manifest.paths.keys,
            &manifest.paths.web_auth,
            &manifest.paths.config,
            &manifest.paths.backups,
            &manifest.paths.releases,
        ])?;
        let projection = Self {
            schema_version: p::SchemaVersion(1),
            binding: ManagedWorkerBinding {
                schema_version: p::SchemaVersion(1),
                endpoint: ManagedWorkerEndpoint::WindowsNamedPipe {
                    name: super::windows_agent_execution_pipe_name(&manifest.install_id)?,
                },
                credential_path: credential_path.to_owned(),
                authority_epoch: config.authority_epoch,
                sandbox_profile_ref: INSTALLED_FILE_PROFILE.into(),
                backend_kinds: BTreeSet::from([p::BackendKind::File]),
            },
            workspace_roots: roots,
        };
        projection.validate()?;
        Ok(projection)
    }

    pub(super) fn validate(&self) -> p::Result<()> {
        self.binding.validate()?;
        if self.schema_version != p::SchemaVersion(1)
            || self.binding.authority_epoch != p::AuthorityEpoch(1)
            || self.binding.sandbox_profile_ref != INSTALLED_FILE_PROFILE
            || self.binding.backend_kinds != BTreeSet::from([p::BackendKind::File])
            || !matches!(
                self.binding.endpoint,
                ManagedWorkerEndpoint::WindowsNamedPipe { .. }
            )
            || self.workspace_roots.len() != 1
            || !self.workspace_roots.contains_key("workspace:default")
        {
            return Err(invalid());
        }
        for root in self.workspace_roots.values() {
            reject_linked_ancestors(root)?;
            if root.parent().is_none() || root.file_name().is_none() {
                return Err(invalid());
            }
        }
        Ok(())
    }

    pub(super) fn validate_template(&self, template: &p::ProductConfigTemplate) -> p::Result<()> {
        self.validate()?;
        let digest =
            p::canonical_digest(&("forme-agent-execution-pipe-v1", &template.install_ref))?;
        let expected = format!(
            r"\\.\pipe\forme-agent-execution-v1-{}",
            digest.0.strip_prefix("sha256:").ok_or_else(invalid)?
        );
        if self.binding.endpoint != (ManagedWorkerEndpoint::WindowsNamedPipe { name: expected })
            || self.binding.credential_path
                != template
                    .model_credentials_directory
                    .parent()
                    .ok_or_else(invalid)?
                    .join("agent-execution.token")
        {
            return Err(invalid());
        }
        for root in self.workspace_roots.values() {
            for protected in [
                &template.brain_store_path,
                &template.artifact_root,
                &template.model_credentials_directory,
                &template.core_config_path,
                &template.core_owner_credential_path,
                &template.core_web_credential_path,
                &template.core_session_fence_path,
            ] {
                if overlaps(root, protected) {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }

    pub(super) fn materialize(&self, brain: &mut serde_json::Value) -> p::Result<()> {
        brain["worker_binding"] = serde_json::to_value(&self.binding).map_err(|_| invalid())?;
        brain["worker_file_roots"] =
            serde_json::to_value(&self.workspace_roots).map_err(|_| invalid())?;
        Ok(())
    }
}

impl super::WorkerRuntimeConfig {
    /// Keep the protected profile history intact while binding the reserved
    /// entry to a journal-selected, reauthenticated release in memory.
    pub fn for_active_release(&self, release: &super::VerifiedStagedRelease) -> p::Result<Self> {
        self.validate()?;
        let mut configured = self.clone();
        if !configured
            .launch_profiles
            .iter()
            .any(|profile| profile.sandbox_profile_ref == INSTALLED_FILE_PROFILE)
        {
            return Ok(configured);
        }
        release.recheck()?;
        if release.manifest().target != p::ProductTarget::WindowsX86_64 {
            return Err(invalid());
        }
        // Historical signed releases retain their six-image layout. A new
        // image is selected only when covered by the verified manifest; an
        // unlisted file must never become an executable fallback.
        let entry = if release
            .manifest()
            .files
            .iter()
            .any(|file| file.relative_path == "bin/forme-file-worker.exe")
        {
            "bin/forme-file-worker.exe"
        } else {
            "bin/forme-executord.exe"
        };
        if !release
            .manifest()
            .files
            .iter()
            .any(|file| file.relative_path == entry)
        {
            return Err(invalid());
        }
        let program = release.content_root().join(entry);
        for profile in &mut configured.launch_profiles {
            if profile.sandbox_profile_ref == INSTALLED_FILE_PROFILE {
                if profile.args != ["--installed-file-worker"] {
                    return Err(invalid());
                }
                profile.program = program.clone();
            }
        }
        configured.validate()?;
        Ok(configured)
    }
}

fn overlaps(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        let text = path.to_string_lossy().replace('\\', "/");
        #[cfg(windows)]
        let text = text.to_lowercase();
        text.trim_end_matches('/').to_owned()
    };
    let left = normalize(left);
    let right = normalize(right);
    left == right || left.starts_with(&(right.clone() + "/")) || right.starts_with(&(left + "/"))
}

fn invalid() -> p::Error {
    p::Error("installed file placement is unavailable or conflicts with the installation".into())
}
