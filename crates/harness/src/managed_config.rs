//! Versioned installed-brain configuration. No implicit environment overrides.
use crate::{
    CapabilityRuntimeConfig, CognitiveRuntimeConfig, DataProtectionConfig, ProductionRuntimeConfig,
};
use forme_models::{ModelRegistryConfig, ModelRuntimeConfig};
use forme_protocol as p;
use serde::Deserialize;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedBrainConfig {
    pub schema_version: p::SchemaVersion,
    pub owner: p::OwnerPrincipalRef,
    pub store_path: PathBuf,
    /// Recovery may select a new database while retaining the original vault.
    /// No keys are copied into backups or derived from the login password.
    #[serde(default)]
    pub content_key_directory: Option<PathBuf>,
    #[serde(default)]
    pub expected_restoration_fence: Option<p::SchemaDigest>,
    pub artifact_root: PathBuf,
    pub workspace: p::WorkspaceRef,
    pub agent_profile: p::AgentProfileRef,
    pub models: Vec<ManagedModelProfile>,
    #[serde(default)]
    pub model_routing: p::ProductModelRouting,
    pub scheduler_enabled: bool,
    pub scheduler: p::SchedulerConfig,
    pub cognition: CognitiveRuntimeConfig,
    pub authority_lease: p::DurationMs,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedModelProfile {
    pub schema_version: p::SchemaVersion,
    pub profile: ModelRuntimeConfig,
    pub credential_path: PathBuf,
}

impl ManagedBrainConfig {
    pub fn runtime_config(&self) -> p::Result<ProductionRuntimeConfig> {
        if let Some(digest) = &self.expected_restoration_fence {
            p::sha256_digest_bytes(digest)?;
            if self.content_key_directory.is_none() {
                return Err(invalid());
            }
        }
        if self.schema_version != p::SchemaVersion(1)
            || self.models.is_empty()
            || self.models.len() > 32
        {
            return Err(invalid());
        }
        for path in [&self.store_path, &self.artifact_root] {
            check_managed_path(path)?;
        }
        let keys = self.content_key_directory()?;
        if self.store_path.starts_with(&keys)
            || keys.starts_with(&self.artifact_root)
            || self.artifact_root.starts_with(&keys)
        {
            return Err(invalid());
        }
        for model in &self.models {
            if model.schema_version != p::SchemaVersion(1) {
                return Err(invalid());
            }
            check_managed_path(&model.credential_path)?;
            if model.credential_path == self.store_path
                || model.credential_path.starts_with(&self.artifact_root)
            {
                return Err(invalid());
            }
        }
        let profiles: Vec<_> = self
            .models
            .iter()
            .map(|model| model.profile.clone())
            .collect();
        ModelRegistryConfig {
            schema_version: p::SchemaVersion(1),
            profiles: profiles.clone(),
        }
        .validate()?;
        self.model_routing.validate(
            &profiles
                .iter()
                .map(|profile| profile.id.clone())
                .collect::<Vec<_>>(),
        )?;
        if profiles.iter().filter(|model| model.default).count() != 1 {
            return Err(invalid());
        }
        // No direct executable tool inherits the brain identity. Managed worker
        // placement is a separate capability integration, never an env switch.
        let config = ProductionRuntimeConfig {
            schema_version: self.schema_version,
            owner: self.owner.clone(),
            store_path: self.store_path.clone(),
            workspace: self.workspace.clone(),
            agent_profile: self.agent_profile.clone(),
            models: profiles,
            capabilities: CapabilityRuntimeConfig::default(),
            scheduler_enabled: self.scheduler_enabled,
            scheduler: self.scheduler.clone(),
            cognition: self.cognition.clone(),
            data: DataProtectionConfig {
                schema_version: p::SchemaVersion(1),
                artifact_root: self.artifact_root.clone(),
            },
            authority_lease: self.authority_lease,
            context_limits: Default::default(),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn content_key_directory(&self) -> p::Result<PathBuf> {
        let path = self
            .content_key_directory
            .clone()
            .unwrap_or_else(|| self.store_path.with_extension("content-keys"));
        check_managed_path(&path)?;
        Ok(path)
    }
}

pub(crate) fn check_managed_path(path: &Path) -> p::Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid());
    }
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(invalid());
                    }
                }
                if metadata.file_type().is_symlink() {
                    return Err(invalid());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(invalid()),
        }
    }
    Ok(())
}

pub(crate) fn read_managed_file(path: &Path, limit: u64) -> p::Result<Vec<u8>> {
    check_managed_path(path)?;
    let file = std::fs::File::open(path).map_err(|_| invalid())?;
    if !file.metadata().map_err(|_| invalid())?.is_file() {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    Ok(bytes)
}

fn invalid() -> p::Error {
    p::Error("managed brain configuration or credential is unavailable or invalid".into())
}
