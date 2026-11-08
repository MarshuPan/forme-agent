//! New-install file worker configuration; reopening never migrates old layouts.
use std::path::PathBuf;

use forme_protocol as p;

pub(super) fn configuration(
    manifest: &p::InstallationManifest,
) -> Option<(PathBuf, serde_json::Value)> {
    if manifest.target != p::ProductTarget::WindowsX86_64 {
        return None;
    }
    Some((
        manifest.paths.config.join("worker.json"),
        serde_json::json!({
            "schema_version": 1,
            "authority_epoch": 1,
            "journal_path": manifest.paths.host_state.join("file-worker.journal"),
            "runtime_root": manifest.paths.host_state.join("file-worker-runtime"),
            "allowed_scopes": ["workspace:default"],
            "workspace_roots": { "workspace:default": manifest.paths.workspaces.join("default") },
            "launch_profiles": [{
                "schema_version": 1,
                "sandbox_profile_ref": "sandbox:installed-file-v1",
                "program": manifest.paths.releases.join("bin").join("forme-file-worker.exe"),
                "args": ["--installed-file-worker"]
            }]
        }),
    ))
}
