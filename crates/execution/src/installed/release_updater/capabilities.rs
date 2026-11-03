//! Probe the fixed updater binary, never infer its version from the business Host.
#[cfg(windows)]
pub(super) fn verify(
    installation: &forme_protocol::InstallationManifest,
) -> forme_protocol::Result<()> {
    use crate::installed::{
        file_digest, maintenance_pipe::MaintenancePipe, reject_linked_ancestors,
    };
    use std::os::windows::process::CommandExt;
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let program = installation.paths.releases.join("bin/forme-hostd.exe");
    reject_linked_ancestors(&program)?;
    let before = file_digest(&program)?;
    let mut command = Command::new(&program);
    command
        .arg("--release-recovery-capabilities")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000);
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    let mut pipe: MaintenancePipe<forme_protocol::ProductReleaseRecoveryCapabilities> =
        MaintenancePipe::spawn(command, Instant::now() + Duration::from_secs(5))?;
    let capabilities = pipe.exchange(&serde_json::json!({"schema_version":1}))?;
    capabilities.validate()?;
    pipe.finish()?;
    reject_linked_ancestors(&program)?;
    if file_digest(&program)? != before {
        return Err(super::invalid());
    }
    Ok(())
}
