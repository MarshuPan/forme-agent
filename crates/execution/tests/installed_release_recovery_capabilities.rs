//! Actual fixed-mode child; no SCM, installation or journal is opened.
use forme_protocol as p;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn release_recovery_capability_mode_is_closed_and_never_loads_host_configuration() {
    let root = tempfile::tempdir().unwrap();
    let journal = root.path().join("must-not-open.journal");
    for request in [
        "{\"schema_version\":1}\n",
        "{\"schema_version\":2}\n",
        "{\"schema_version\":1,\"program\":\"foreign\"}\n",
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forme-hostd"));
        command
            .arg("--release-recovery-capabilities")
            .env_clear()
            .env("FORME_HOST_JOURNAL_PATH", &journal)
            .current_dir(root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        if request == "{\"schema_version\":1}\n" {
            assert!(output.status.success());
            let view: p::ProductReleaseRecoveryCapabilities =
                serde_json::from_slice(&output.stdout).unwrap();
            view.validate().unwrap();
            assert_eq!(view.journal_read_max, 2);
            assert_eq!(view.updater_version, 2);
        } else {
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
        }
        assert!(!journal.exists());
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }
    let incompatible = p::ProductReleaseRecoveryCapabilities {
        schema_version: p::SchemaVersion(1),
        journal_read_max: 1,
        updater_version: 1,
    };
    assert!(incompatible.validate().is_err());
}
