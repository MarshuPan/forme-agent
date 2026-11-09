//! A separate installed desktop placement; File launch profiles confer none of it.
use super::{ManagedWorkerBinding, ManagedWorkerEndpoint};
use forme_protocol as p;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[cfg(windows)]
use std::path::Path;

pub const INSTALLED_DESKTOP_PROFILE: &str = "sandbox:installed-desktop-v1";
pub const INSTALLED_DESKTOP_SURFACE: &str = "surface:windows-default";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledDesktopProvisioning {
    schema_version: p::SchemaVersion,
    binding: ManagedWorkerBinding,
    scope: p::Scope,
    surface_refs: Vec<String>,
}
impl InstalledDesktopProvisioning {
    #[cfg(windows)]
    pub fn derive(
        manifest: &p::InstallationManifest,
        worker: &super::WorkerRuntimeConfig,
        credential: &Path,
    ) -> p::Result<Self> {
        // Reuse only installation-layout validation. The resulting binding is
        // independent and never grants desktop use to the File profile.
        super::InstalledFileProvisioning::derive(manifest, worker, credential)?;
        let result = Self {
            schema_version: p::SchemaVersion(1),
            binding: ManagedWorkerBinding {
                schema_version: p::SchemaVersion(1),
                endpoint: ManagedWorkerEndpoint::WindowsNamedPipe {
                    name: super::windows_agent_execution_pipe_name(&manifest.install_id)?,
                },
                credential_path: credential.to_owned(),
                authority_epoch: worker.authority_epoch,
                sandbox_profile_ref: INSTALLED_DESKTOP_PROFILE.into(),
                backend_kinds: BTreeSet::from([p::BackendKind::Computer]),
            },
            scope: p::Scope("workspace:default".into()),
            surface_refs: vec![INSTALLED_DESKTOP_SURFACE.into()],
        };
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> p::Result<()> {
        self.binding.validate()?;
        if self.schema_version != p::SchemaVersion(1)
            || self.binding.authority_epoch != p::AuthorityEpoch(1)
            || self.binding.sandbox_profile_ref != INSTALLED_DESKTOP_PROFILE
            || self.binding.backend_kinds != BTreeSet::from([p::BackendKind::Computer])
            || !matches!(
                self.binding.endpoint,
                ManagedWorkerEndpoint::WindowsNamedPipe { .. }
            )
            || self.scope.0 != "workspace:default"
            || self.surface_refs != [INSTALLED_DESKTOP_SURFACE]
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn validate_template(&self, template: &p::ProductConfigTemplate) -> p::Result<()> {
        self.validate()?;
        let digest =
            p::canonical_digest(&("forme-agent-execution-pipe-v1", &template.install_ref))?;
        let endpoint = ManagedWorkerEndpoint::WindowsNamedPipe {
            name: format!(r"\\.\pipe\forme-agent-execution-v1-{}", &digest.0[7..]),
        };
        if self.binding.endpoint != endpoint
            || self.binding.credential_path
                != template
                    .model_credentials_directory
                    .parent()
                    .ok_or_else(invalid)?
                    .join("agent-execution.token")
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn validate_settings(
        &self,
        settings: &p::ProductComputerPlacementSettings,
    ) -> p::Result<()> {
        self.validate()?;
        settings.validate()?;
        if settings.enabled
            && (settings.scope != self.scope || settings.surface_refs != self.surface_refs)
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn binding(&self) -> &ManagedWorkerBinding {
        &self.binding
    }
    pub fn scope(&self) -> &p::Scope {
        &self.scope
    }
    pub fn surface_refs(&self) -> &[String] {
        &self.surface_refs
    }
    /// This is a coordinate authorization ceiling, never a reported display size.
    pub fn coordinate_bounds() -> p::CoordinateBounds {
        p::CoordinateBounds {
            schema_version: p::SchemaVersion(1),
            min_x: -32768,
            min_y: -32768,
            max_x_exclusive: 32768,
            max_y_exclusive: 32768,
        }
    }
    pub(super) fn validate_pending(&self, pending: &p::DesktopPendingAction) -> p::Result<()> {
        self.validate()?;
        pending.validate()?;
        let resources = p::normalize_resource_set(&[
            p::ResourceRef("computer-driver:computer:native".into()),
            p::ResourceRef(format!("surface:{INSTALLED_DESKTOP_SURFACE}")),
        ]);
        if pending.authority_epoch != self.binding.authority_epoch
            || pending.affected_resources != resources
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn validate_execution(
        &self,
        job: &p::WorkerJob,
        execution: &p::WorkerExecutionSpec,
    ) -> p::Result<()> {
        self.validate()?;
        let p::ActionParameters::Computer(spec) = &execution.intent.parameters else {
            return Err(invalid());
        };
        spec.bounds.validate()?;
        let allowed = Self::coordinate_bounds();
        if job.sandbox_profile_ref != self.binding.sandbox_profile_ref
            || job.authority_epoch != self.binding.authority_epoch
            || job.workspace_scope != self.scope
            || execution.scope != self.scope
            || execution.backend != p::BackendKind::Computer
            || spec.driver.0 != "computer:native"
            || !self.surface_refs.contains(&spec.surface.0)
            || spec.artifact_scope != self.scope
            || spec.bounds.min_x < allowed.min_x
            || spec.bounds.min_y < allowed.min_y
            || spec.bounds.max_x_exclusive > allowed.max_x_exclusive
            || spec.bounds.max_y_exclusive > allowed.max_y_exclusive
            || !crate::computer::computer_operation_within_bounds(spec)
        {
            return Err(invalid());
        }
        p::desktop_intent_capability(&execution.intent)?;
        Ok(())
    }
}
fn invalid() -> p::Error {
    p::Error(
        "installed desktop placement is unavailable or does not match the configured action".into(),
    )
}
