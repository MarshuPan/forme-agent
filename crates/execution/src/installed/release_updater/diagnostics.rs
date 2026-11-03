//! Best-effort private diagnostics, with no authority to recover or retry.
use super::*;
use crate::installed::reject_linked_ancestors;
use serde::Serialize;
use std::{fs, io::Write};

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    #[default]
    ReadHandoff,
    VerifyUpdater,
    VerifyTarget,
    ClaimDispatch,
    ObserveOriginalService,
    StopOriginalService,
    AcquireJournal,
    VerifyTakeover,
    InspectOriginalData,
    BackupOriginalData,
    SwitchServiceImages,
    InspectTargetData,
    PublishReleaseBinding,
    PrepareChecking,
    StartTargetService,
}

#[derive(Default, Serialize)]
pub(super) struct Progress {
    stage: Stage,
    service: Option<p::BuiltinServiceId>,
    operation_id: Option<String>,
    command_digest: Option<p::SchemaDigest>,
}

impl Progress {
    pub(super) fn bind(&mut self, takeover: &p::ProductReleaseTakeover) {
        self.operation_id = Some(takeover.release.operation_id.clone());
        self.command_digest = Some(takeover.release.command_digest.clone());
    }

    pub(super) fn set(&mut self, stage: Stage, service: Option<p::BuiltinServiceId>) {
        self.stage = stage;
        self.service = service;
    }
}

#[derive(Serialize)]
struct Failure<'a> {
    schema_version: p::SchemaVersion,
    code: &'static str,
    progress: &'a Progress,
    reason_ref: p::SchemaDigest,
}

pub(super) fn record(directory: &Path, progress: &Progress, error: &p::Error) -> p::Result<()> {
    reject_linked_ancestors(directory)?;
    if !fs::metadata(directory).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(invalid());
    }
    let destination = directory.join(format!(
        "release-updater-failure-{}.json",
        std::process::id()
    ));
    reject_linked_ancestors(&destination)?;
    let bytes = serde_json::to_vec(&Failure {
        schema_version: p::SchemaVersion(1),
        code: "ServiceUnavailable",
        progress,
        reason_ref: p::canonical_digest(&("release-updater-failure-v1", &error.0))?,
    })
    .map_err(|_| invalid())?;
    if bytes.len() > 4096 {
        return Err(invalid());
    }
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(|_| invalid())?;
    temporary
        .write_all(&bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|_| invalid())?;
    reject_linked_ancestors(&destination)?;
    temporary
        .persist_noclobber(destination)
        .map_err(|_| invalid())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updater_failure_preserves_phase_without_recording_error_body_or_overwriting() {
        let root = tempfile::tempdir().unwrap();
        let mut progress = Progress {
            operation_id: Some("maintenance:fixture-original".into()),
            command_digest: Some(p::sha256_content_digest(b"fixture-command")),
            ..Progress::default()
        };
        progress.set(Stage::StopOriginalService, Some(p::BuiltinServiceId::Web));
        let error = p::Error("private-provider-key-and-path-which-must-not-be-recorded".into());
        record(root.path(), &progress, &error).unwrap();
        let destination = root.path().join(format!(
            "release-updater-failure-{}.json",
            std::process::id()
        ));
        let original = fs::read(&destination).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        assert_eq!(value["progress"]["stage"], "stop_original_service");
        assert_eq!(
            value["progress"]["operation_id"],
            "maintenance:fixture-original"
        );
        assert_eq!(value["progress"]["service"], "web");
        assert!(original.len() < 1024);
        assert!(!String::from_utf8(original.clone())
            .unwrap()
            .contains(&error.0));
        progress.set(Stage::StartTargetService, Some(p::BuiltinServiceId::Brain));
        assert!(record(root.path(), &progress, &error).is_err());
        assert_eq!(fs::read(&destination).unwrap(), original);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn updater_failure_never_creates_a_missing_log_directory() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing");
        assert!(record(&missing, &Progress::default(), &invalid()).is_err());
        assert!(!missing.exists());
        let progress = Progress {
            operation_id: Some("x".repeat(5000)),
            ..Progress::default()
        };
        assert!(record(root.path(), &progress, &invalid()).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
