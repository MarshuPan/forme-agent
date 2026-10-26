use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
use super::LinuxSystemdWorkerIsolation;
#[cfg(target_os = "macos")]
use super::MacOsLaunchdWorkerIsolation;
use super::{
    reject_linked_ancestors, ProcessWorkerIsolation, WorkerExecutorHost, WorkerLaunchRegistry,
    WorkerLaunchSpec,
};
#[cfg(windows)]
use super::{WindowsAppContainerWorkerIsolation, WindowsJobWorkerIsolation};

const MAX_WORKER_CONFIG_BYTES: u64 = 4 * 1024 * 1024;

/// A managed launch profile. It is loaded from installation configuration, not
/// from a WorkerJob or from model output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerLaunchProfileConfig {
    pub schema_version: p::SchemaVersion,
    pub sandbox_profile_ref: String,
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// Typed product configuration for the worker host. The file
/// is expected to live under the protected installation config root; this
/// loader adds structural checks but does not replace platform ACL validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRuntimeConfig {
    pub schema_version: p::SchemaVersion,
    pub authority_epoch: p::AuthorityEpoch,
    pub journal_path: PathBuf,
    pub runtime_root: PathBuf,
    pub allowed_scopes: Vec<p::Scope>,
    pub workspace_roots: BTreeMap<String, PathBuf>,
    pub launch_profiles: Vec<WorkerLaunchProfileConfig>,
}

impl WorkerRuntimeConfig {
    pub fn load(path: &Path) -> p::Result<Self> {
        if !path.is_absolute() {
            return Err(p::Error("worker config path must be absolute".into()));
        }
        reject_linked_ancestors(path)?;
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| p::Error("worker config is unavailable".into()))?;
        if !metadata.is_file() {
            return Err(p::Error("worker config is not a regular file".into()));
        }
        if metadata.len() > MAX_WORKER_CONFIG_BYTES {
            return Err(p::Error("worker config exceeds its size bound".into()));
        }
        let mut bytes = Vec::new();
        fs::File::open(path)
            .and_then(|file| {
                file.take(MAX_WORKER_CONFIG_BYTES + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|_| p::Error("worker config cannot be read".into()))?;
        if bytes.len() as u64 > MAX_WORKER_CONFIG_BYTES {
            return Err(p::Error("worker config exceeds its size bound".into()));
        }
        let config: Self = serde_json::from_slice(&bytes)
            .map_err(|_| p::Error("worker config is not valid JSON".into()))?;
        config.validate()?;
        config.validate_protected_paths(&[path])?;
        Ok(config)
    }

    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1) {
            return Err(p::Error("unsupported worker config schema".into()));
        }
        if self.authority_epoch.0 == 0 {
            return Err(p::Error("worker config authority epoch is invalid".into()));
        }
        validate_absolute_path(&self.journal_path, "worker journal")?;
        validate_absolute_path(&self.runtime_root, "worker runtime")?;
        if self.journal_path.exists() && !self.journal_path.is_file() {
            return Err(p::Error("worker journal is not a regular file".into()));
        }
        if self.runtime_root.exists() && !self.runtime_root.is_dir() {
            return Err(p::Error("worker runtime is not a directory".into()));
        }
        if self.allowed_scopes.is_empty() {
            return Err(p::Error("worker config scope allowlist is empty".into()));
        }
        let mut scopes = std::collections::BTreeSet::new();
        for scope in &self.allowed_scopes {
            validate_scope(scope)?;
            if !scopes.insert(scope.0.clone()) {
                return Err(p::Error("worker config scope is duplicated".into()));
            }
            if !self.workspace_roots.contains_key(&scope.0) {
                return Err(p::Error("worker config has no path for a scope".into()));
            }
        }
        if self.workspace_roots.len() != scopes.len() {
            return Err(p::Error(
                "worker config contains an unallowlisted workspace".into(),
            ));
        }
        for (scope, path) in &self.workspace_roots {
            validate_scope(&p::Scope(scope.clone()))?;
            validate_absolute_path(path, "worker workspace")?;
            if !path.is_dir() {
                return Err(p::Error("worker workspace is not a directory".into()));
            }
        }
        if self.launch_profiles.is_empty() {
            return Err(p::Error(
                "worker config launch profile list is empty".into(),
            ));
        }
        let mut profiles = std::collections::BTreeSet::new();
        for profile in &self.launch_profiles {
            if profile.schema_version != p::SchemaVersion(1) {
                return Err(p::Error("unsupported worker launch profile schema".into()));
            }
            if !profiles.insert(profile.sandbox_profile_ref.clone()) {
                return Err(p::Error(
                    "worker config launch profile is duplicated".into(),
                ));
            }
            WorkerLaunchSpec::new(
                profile.sandbox_profile_ref.clone(),
                profile.program.clone(),
                profile.args.clone(),
            )?;
        }
        self.validate_protected_paths(&[])
    }

    /// Preflight product files against every writable worker path. This is
    /// not a substitute for platform ACLs or execution-time identity checks.
    pub fn validate_protected_paths(&self, protected: &[&Path]) -> p::Result<()> {
        let result_root = self.result_root();
        let result_keys = result_root.with_extension("result-keys");
        let writable: Vec<_> = [&self.journal_path, &self.runtime_root]
            .into_iter()
            .chain(std::iter::once(&result_root))
            .chain(std::iter::once(&result_keys))
            .chain(self.workspace_roots.values())
            .map(|path| path_identity(path))
            .collect::<p::Result<_>>()?;
        for (index, left) in writable.iter().enumerate() {
            if writable[index + 1..]
                .iter()
                .any(|right| paths_overlap(left, right))
            {
                return Err(p::Error("worker writable paths must be disjoint".into()));
            }
        }
        for path in protected.iter().copied().chain(
            self.launch_profiles
                .iter()
                .map(|profile| profile.program.as_path()),
        ) {
            let identity = path_identity(path)?;
            if writable.iter().any(|root| paths_overlap(root, &identity)) {
                return Err(p::Error(
                    "worker paths overlap protected product files".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn result_root(&self) -> PathBuf {
        self.journal_path.with_extension("results")
    }

    /// Service default: a config file cannot assert that a platform sandbox
    /// exists. Keep execution blocked until a real adapter is installed.
    pub fn open_host(
        &self,
        agent_credential_digest: p::SchemaDigest,
    ) -> p::Result<WorkerExecutorHost> {
        self.validate()?;
        WorkerExecutorHost::open(
            &self.journal_path,
            agent_credential_digest,
            self.authority_epoch,
            self.allowed_scopes.iter().cloned().collect(),
        )
    }

    /// Explicit foreground development only. This adapter inherits the OS
    /// identity of its caller and must never be selected by a system service.
    pub fn open_development_process_host(
        &self,
        agent_credential_digest: p::SchemaDigest,
    ) -> p::Result<WorkerExecutorHost<ProcessWorkerIsolation>> {
        self.validate()?;
        let launches = self
            .launch_profiles
            .iter()
            .map(|profile| {
                WorkerLaunchSpec::new(
                    profile.sandbox_profile_ref.clone(),
                    profile.program.clone(),
                    profile.args.clone(),
                )
            })
            .collect::<p::Result<Vec<_>>>()?;
        let isolation = ProcessWorkerIsolation::open(
            self.runtime_root.clone(),
            WorkerLaunchRegistry::from_specs(launches)?,
            self.workspace_roots.clone(),
        )?;
        WorkerExecutorHost::open_with_isolation(
            &self.journal_path,
            agent_credential_digest,
            self.authority_epoch,
            self.allowed_scopes.iter().cloned().collect(),
            isolation,
        )
    }

    /// Managed Linux composition. The adapter proves the fixed root/systemd
    /// boundary and keeps ordinary process workers unavailable on failure.
    #[cfg(target_os = "linux")]
    pub fn open_managed_linux_systemd_host(
        &self,
        agent_credential_digest: p::SchemaDigest,
    ) -> p::Result<WorkerExecutorHost<LinuxSystemdWorkerIsolation>> {
        self.validate()?;
        let launches = self
            .launch_profiles
            .iter()
            .map(|profile| {
                WorkerLaunchSpec::new(
                    profile.sandbox_profile_ref.clone(),
                    profile.program.clone(),
                    profile.args.clone(),
                )
            })
            .collect::<p::Result<Vec<_>>>()?;
        let isolation = LinuxSystemdWorkerIsolation::open(
            self.runtime_root.clone(),
            WorkerLaunchRegistry::from_specs(launches)?,
            self.workspace_roots.clone(),
        )?;
        WorkerExecutorHost::open_with_isolation(
            &self.journal_path,
            agent_credential_digest,
            self.authority_epoch,
            self.allowed_scopes.iter().cloned().collect(),
            isolation,
        )
    }

    /// Managed macOS composition. The adapter proves the fixed root/launchd
    /// boundary and keeps ordinary process workers unavailable on failure.
    #[cfg(target_os = "macos")]
    pub fn open_managed_macos_launchd_host(
        &self,
        agent_credential_digest: p::SchemaDigest,
    ) -> p::Result<WorkerExecutorHost<MacOsLaunchdWorkerIsolation>> {
        self.validate()?;
        let launches = self
            .launch_profiles
            .iter()
            .map(|profile| {
                WorkerLaunchSpec::new(
                    profile.sandbox_profile_ref.clone(),
                    profile.program.clone(),
                    profile.args.clone(),
                )
            })
            .collect::<p::Result<Vec<_>>>()?;
        let isolation = MacOsLaunchdWorkerIsolation::open(
            self.runtime_root.clone(),
            WorkerLaunchRegistry::from_specs(launches)?,
            self.workspace_roots.clone(),
        )?;
        WorkerExecutorHost::open_with_isolation(
            &self.journal_path,
            agent_credential_digest,
            self.authority_epoch,
            self.allowed_scopes.iter().cloned().collect(),
            isolation,
        )
    }

    /// Explicit Windows platform-adapter composition. The managed service
    /// does not select this until the independent service-account, ACL and
    /// network gates are also installed and verified.
    #[cfg(windows)]
    pub fn open_windows_job_host(
        &self,
        agent_credential_digest: p::SchemaDigest,
    ) -> p::Result<WorkerExecutorHost<WindowsJobWorkerIsolation>> {
        self.validate()?;
        let launches = self
            .launch_profiles
            .iter()
            .map(|profile| {
                WorkerLaunchSpec::new(
                    profile.sandbox_profile_ref.clone(),
                    profile.program.clone(),
                    profile.args.clone(),
                )
            })
            .collect::<p::Result<Vec<_>>>()?;
        let isolation = WindowsJobWorkerIsolation::open(
            self.runtime_root.clone(),
            WorkerLaunchRegistry::from_specs(launches)?,
            self.workspace_roots.clone(),
        )?;
        WorkerExecutorHost::open_with_isolation(
            &self.journal_path,
            agent_credential_digest,
            self.authority_epoch,
            self.allowed_scopes.iter().cloned().collect(),
            isolation,
        )
    }

    /// Explicit Windows AppContainer composition. This is a platform adapter,
    /// not a claim that the surrounding installation/service gates passed.
    #[cfg(windows)]
    pub fn open_windows_appcontainer_host(
        &self,
        agent_credential_digest: p::SchemaDigest,
    ) -> p::Result<WorkerExecutorHost<WindowsAppContainerWorkerIsolation>> {
        self.validate()?;
        let launches = self
            .launch_profiles
            .iter()
            .map(|profile| {
                WorkerLaunchSpec::new(
                    profile.sandbox_profile_ref.clone(),
                    profile.program.clone(),
                    profile.args.clone(),
                )
            })
            .collect::<p::Result<Vec<_>>>()?;
        let isolation = WindowsAppContainerWorkerIsolation::open(
            self.runtime_root.clone(),
            WorkerLaunchRegistry::from_specs(launches)?,
            self.workspace_roots.clone(),
        )?;
        WorkerExecutorHost::open_with_isolation(
            &self.journal_path,
            agent_credential_digest,
            self.authority_epoch,
            self.allowed_scopes.iter().cloned().collect(),
            isolation,
        )
    }

    /// Managed Windows service composition. The caller must already have
    /// proven the fixed SCM binding; the adapter independently proves that
    /// this process is the LocalSystem broker before opening state.
    #[cfg(windows)]
    pub fn open_managed_windows_appcontainer_host(
        &self,
        agent_credential_digest: p::SchemaDigest,
    ) -> p::Result<WorkerExecutorHost<WindowsAppContainerWorkerIsolation>> {
        self.validate()?;
        let launches = self
            .launch_profiles
            .iter()
            .map(|profile| {
                WorkerLaunchSpec::new(
                    profile.sandbox_profile_ref.clone(),
                    profile.program.clone(),
                    profile.args.clone(),
                )
            })
            .collect::<p::Result<Vec<_>>>()?;
        let isolation = WindowsAppContainerWorkerIsolation::open_managed(
            self.runtime_root.clone(),
            WorkerLaunchRegistry::from_specs(launches)?,
            self.workspace_roots.clone(),
        )?;
        WorkerExecutorHost::open_with_isolation(
            &self.journal_path,
            agent_credential_digest,
            self.authority_epoch,
            self.allowed_scopes.iter().cloned().collect(),
            isolation,
        )
    }
}

fn validate_absolute_path(path: &Path, name: &str) -> p::Result<()> {
    if !path.is_absolute() || path.file_name().is_none() || path.as_os_str().len() > 4096 {
        return Err(p::Error(format!(
            "{name} path must be absolute and bounded"
        )));
    }
    for component in path.components() {
        if let std::path::Component::Normal(part) = component {
            p::validate_release_relative_path(
                part.to_str()
                    .ok_or_else(|| p::Error("worker path encoding is invalid".into()))?,
            )?;
        }
    }
    reject_linked_ancestors(path)
}

fn path_identity(path: &Path) -> p::Result<String> {
    validate_absolute_path(path, "worker")?;
    let mut existing = path;
    let mut suffix = Vec::new();
    while !existing.exists() {
        suffix.push(
            existing
                .file_name()
                .ok_or_else(|| p::Error("worker path cannot be resolved".into()))?,
        );
        existing = existing
            .parent()
            .ok_or_else(|| p::Error("worker path cannot be resolved".into()))?;
    }
    let mut resolved = fs::canonicalize(existing)
        .map_err(|_| p::Error("worker path cannot be resolved".into()))?;
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }
    let identity = resolved.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    let identity = identity.to_lowercase();
    Ok(identity)
}

fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn validate_scope(scope: &p::Scope) -> p::Result<()> {
    if scope.0.is_empty()
        || scope.0.len() > 512
        || !scope.0.starts_with("workspace:")
        || scope.0.contains(['\\', '\0'])
        || scope.0.chars().any(char::is_control)
        || scope
            .0
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(p::Error("worker config workspace scope is invalid".into()));
    }
    Ok(())
}
