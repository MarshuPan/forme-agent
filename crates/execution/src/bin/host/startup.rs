//! Bounded, secret-free service failure diagnostics in the Host-only log area.
#[cfg(any(windows, test))]
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

#[cfg(any(windows, test))]
use forme_protocol as p;
use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    #[cfg(windows)]
    ReportingStartup,
    #[cfg(windows)]
    ScmBinding,
    #[cfg(any(windows, test))]
    Configuration,
    SetupAuthority,
    ProductConfiguration,
    HostJournal,
    InstallationVerification,
    Worker,
    AgentEndpoint,
    ManagementEndpoints,
    Readiness,
    Serving,
    Draining,
}

#[derive(Serialize)]
#[cfg(any(windows, test))]
struct Failure {
    schema_version: p::SchemaVersion,
    code: &'static str,
    stage: Stage,
    reason_ref: p::SchemaDigest,
}

/// Best-effort diagnostic only: failure never changes the service outcome or
/// creates a substitute journal. The configured directory must already exist.
/// Error bodies, paths, credentials and provider responses are never written.
#[cfg(any(windows, test))]
pub(super) fn record_failure(directory: &Path, stage: Stage, error: &p::Error) -> p::Result<()> {
    forme_execution::installed::reject_linked_ancestors(directory)?;
    if !fs::metadata(directory).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(unavailable());
    }
    let destination = directory.join("startup-failure.json");
    forme_execution::installed::reject_linked_ancestors(&destination)?;
    match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= 4096 => {}
        Ok(_) => return Err(unavailable()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(unavailable()),
    }
    let failure = Failure {
        schema_version: p::SchemaVersion(1),
        code: "ServiceUnavailable",
        stage,
        reason_ref: p::canonical_digest(&("host-service-failure-v1", &error.0))?,
    };
    let bytes = serde_json::to_vec(&failure).map_err(|_| unavailable())?;
    let temporary = directory.join(format!("startup-failure-{}.tmp", std::process::id()));
    forme_execution::installed::reject_linked_ancestors(&temporary)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|_| unavailable())?;
    let result = (|| {
        file.write_all(&bytes).map_err(|_| unavailable())?;
        file.sync_all().map_err(|_| unavailable())?;
        drop(file);
        forme_execution::installed::reject_linked_ancestors(&destination)?;
        fs::rename(&temporary, &destination).map_err(|_| unavailable())
    })();
    if result.is_err() {
        // Only the newly created, fixed diagnostic temporary is ours to remove.
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(any(windows, test))]
fn unavailable() -> p::Error {
    p::Error("Host startup diagnostic could not be saved".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_is_bounded_and_never_persists_error_bodies_or_credentials() {
        let root = tempfile::tempdir().unwrap();
        let error = p::Error("private-error-body-with-an-owner-credential".into());
        record_failure(root.path(), Stage::Worker, &error).unwrap();
        let first = fs::read(root.path().join("startup-failure.json")).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&first).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["stage"], "worker");
        assert_eq!(value["code"], "ServiceUnavailable");
        assert!(first.len() < 512);
        assert!(!String::from_utf8(first).unwrap().contains(&error.0));
        record_failure(root.path(), Stage::Readiness, &error).unwrap();
        let next: serde_json::Value =
            serde_json::from_slice(&fs::read(root.path().join("startup-failure.json")).unwrap())
                .unwrap();
        assert_eq!(next["stage"], "readiness");
        assert_eq!(next["reason_ref"], value["reason_ref"]);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn failure_refuses_missing_directory_or_an_unexpected_existing_target() {
        let root = tempfile::tempdir().unwrap();
        let error = p::Error("fixture".into());
        let missing = root.path().join("missing");
        assert!(record_failure(&missing, Stage::Configuration, &error).is_err());
        assert!(!missing.exists());
        let target = root.path().join("startup-failure.json");
        let original = vec![b'x'; 4097];
        fs::write(&target, &original).unwrap();
        assert!(record_failure(root.path(), Stage::Worker, &error).is_err());
        assert_eq!(fs::read(&target).unwrap(), original);
        fs::remove_file(&target).unwrap();
        fs::create_dir(&target).unwrap();
        assert!(record_failure(root.path(), Stage::Worker, &error).is_err());
        assert!(target.is_dir());
    }
}
