//! Real child transport tests with a repository-owned fault fixture, not SCM/IP.
use forme_execution::installed::{
    InstalledReleaseDataWorker, ReleaseUpdaterPlatform, SystemReleaseUpdaterPlatform,
};
use forme_protocol as p;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Fixture {
    _root: tempfile::TempDir,
    installation: p::InstallationManifest,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        let paths = p::ProductPaths {
            schema_version: p::SchemaVersion(1),
            releases: path.join("releases"),
            config: path.join("config"),
            brain: path.join("brain"),
            web_auth: path.join("web-auth"),
            host_state: path.join("host-state"),
            keys: path.join("keys"),
            logs: path.join("logs"),
            backups: path.join("backups"),
            workspaces: path.join("workspaces"),
            ipc: path.join("ipc"),
        };
        let target = if cfg!(windows) {
            p::ProductTarget::WindowsX86_64
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            p::ProductTarget::MacosArm64
        } else if cfg!(target_os = "macos") {
            p::ProductTarget::MacosX86_64
        } else if cfg!(target_arch = "aarch64") {
            p::ProductTarget::Ubuntu2404Arm64
        } else {
            p::ProductTarget::Ubuntu2404X86_64
        };
        let installation = p::InstallationManifest {
            schema_version: p::SchemaVersion(1),
            install_id: "install:data-transport".into(),
            target,
            release_id: "0.1.0".into(),
            release_digest: p::canonical_digest(&"original").unwrap(),
            paths,
            services: [
                p::BuiltinServiceId::Host,
                p::BuiltinServiceId::Web,
                p::BuiltinServiceId::Brain,
            ]
            .into_iter()
            .map(|service| p::ServiceIdentityBinding {
                schema_version: p::SchemaVersion(1),
                service,
                account: service.required_account(target).into(),
            })
            .collect(),
            created_at: 1,
        };
        for directory in [
            &installation.paths.config,
            &installation.paths.host_state,
            &installation.paths.releases.join("bin"),
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        fs::write(
            installation.paths.config.join("installation.json"),
            serde_json::to_vec(&installation).unwrap(),
        )
        .unwrap();
        let binary = if cfg!(windows) {
            "forme-hostctl.exe"
        } else {
            "forme-hostctl"
        };
        fs::copy(
            env!("CARGO_BIN_EXE_forme-host-storage-fixture"),
            installation.paths.releases.join("bin").join(binary),
        )
        .unwrap();
        Self {
            _root: root,
            installation,
        }
    }
    fn worker(&self) -> InstalledReleaseDataWorker {
        InstalledReleaseDataWorker::new(self.installation.clone()).unwrap()
    }
    fn marker(&self, name: &str) -> PathBuf {
        self.installation.paths.host_state.join(name)
    }
    fn request(&self, mode: &str) -> p::ProductReleaseDataRequest {
        p::ProductReleaseDataRequest {
            schema_version: p::SchemaVersion(1),
            expected_config_revision: 1,
            owner: p::OwnerPrincipalRef("owner:fixture".into()),
            release: p::ProductReleaseRequest {
                schema_version: p::SchemaVersion(1),
                install_ref: self.installation.install_id.clone(),
                operation_id: "operation:pipe".into(),
                command_digest: p::canonical_digest(&"command").unwrap(),
                change: p::ProductReleaseChange::Upgrade,
                original_release_ref: "0.1.0".into(),
                original_release_digest: self.installation.release_digest.clone(),
                target_release_ref: mode.into(),
                target_release_digest: p::canonical_digest(&"target").unwrap(),
                backup_ref: "backup:pipe".into(),
            },
        }
    }
}

#[test]
fn release_recovery_inspection_uses_fixed_child_and_releases_without_backup() {
    let f = Fixture::new();
    let mut inspection = SystemReleaseUpdaterPlatform
        .inspect_recovery_data(
            f.worker(),
            f.request("0.2.0"),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(inspection.identity().database_schema_version, 13);
    assert!(f.marker("data-worker-started").exists());
    assert!(!f.marker("data-worker-backup").exists());
    inspection
        .ensure_held(Instant::now() + Duration::from_secs(5))
        .unwrap();
    inspection.release().unwrap();
    assert!(inspection
        .ensure_held(Instant::now() + Duration::from_secs(5))
        .is_err());
    assert!(inspection.release().is_err());
    assert!(!f.marker("data-worker-backup").exists());
}

#[test]
fn release_data_hold_rejects_a_cleanly_exited_child_without_respawn_or_backup() {
    let f = Fixture::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut process = f
        .worker()
        .start(f.request("exit-after-inspect"), deadline)
        .unwrap();
    loop {
        if process.ensure_read_only_hold(deadline).is_err() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "clean exit must invalidate the hold"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(process.ensure_read_only_hold(deadline).is_err());
    assert!(process.step(p::ProductReleaseDataStep::Backup).is_err());
    assert!(process.step(p::ProductReleaseDataStep::Release).is_err());
    assert!(!f.marker("data-worker-backup").exists());
}

#[test]
fn release_data_hold_rechecks_the_existing_child_and_cannot_survive_its_deadline() {
    let f = Fixture::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut process = f.worker().start(f.request("0.2.0"), deadline).unwrap();
    let identity = process.identity().clone();
    for _ in 0..3 {
        process.ensure_read_only_hold(deadline).unwrap();
        assert_eq!(process.identity(), &identity);
    }
    assert!(!f.marker("data-worker-backup").exists());
    assert!(process.ensure_read_only_hold(Instant::now()).is_err());
    assert!(process.ensure_read_only_hold(deadline).is_err());
    assert!(process.step(p::ProductReleaseDataStep::Backup).is_err());
    assert!(!f.marker("data-worker-backup").exists());
}

#[test]
fn release_data_child_preserves_binding_and_consumes_backup_once() {
    let f = Fixture::new();
    let mut process = f
        .worker()
        .start(f.request("0.2.0"), Instant::now() + Duration::from_secs(10))
        .unwrap();
    assert_eq!(process.identity().database_schema_version, 13);
    process.step(p::ProductReleaseDataStep::Backup).unwrap();
    assert!(process.step(p::ProductReleaseDataStep::Backup).is_err());
    assert_eq!(fs::read(f.marker("data-worker-backup")).unwrap(), b"1");
    process.step(p::ProductReleaseDataStep::Release).unwrap();
    assert!(process.step(p::ProductReleaseDataStep::Release).is_err());
}

#[test]
fn release_data_child_deadline_kills_waiting_process_and_does_not_retry_unknown_backup() {
    for mode in ["stall-before-inspect", "stall-on-backup"] {
        let f = Fixture::new();
        let started = Instant::now();
        let result = f
            .worker()
            // A new Windows executable can spend seconds in native image/AV
            // startup under the full suite. Keep a finite end-to-end deadline
            // with enough startup margin; neither stalled phase may bypass it.
            .start(f.request(mode), started + Duration::from_secs(10));
        if mode == "stall-on-backup" {
            let mut process = result.unwrap();
            assert!(process.step(p::ProductReleaseDataStep::Backup).is_err());
            assert!(process.step(p::ProductReleaseDataStep::Backup).is_err());
            assert_eq!(fs::read(f.marker("data-worker-backup")).unwrap(), b"1");
        } else {
            assert!(result.is_err());
        }
        assert!(started.elapsed() < Duration::from_secs(18));
        // A replaced executable is also impossible while a Windows child is
        // still live. Keep both copies inside this test-owned installation.
        let binary = f
            .installation
            .paths
            .releases
            .join("bin")
            .join(if cfg!(windows) {
                "forme-hostctl.exe"
            } else {
                "forme-hostctl"
            });
        fs::rename(&binary, binary.with_extension("retained-fixture")).unwrap();
    }
}

#[test]
fn release_data_child_rejects_bad_owner_oversize_nonzero_exit_and_replaced_installation() {
    for mode in ["wrong-owner", "oversized-frame", "nonzero-release"] {
        let f = Fixture::new();
        let result = f
            .worker()
            .start(f.request(mode), Instant::now() + Duration::from_secs(5));
        if mode == "nonzero-release" {
            let mut process = result.unwrap();
            assert!(process.step(p::ProductReleaseDataStep::Release).is_err());
        } else {
            assert!(result.is_err());
        }
    }
    let f = Fixture::new();
    let worker = f.worker();
    let mut replaced = f.installation.clone();
    replaced.created_at += 1;
    fs::write(
        replaced.paths.config.join("installation.json"),
        serde_json::to_vec(&replaced).unwrap(),
    )
    .unwrap();
    assert!(worker
        .start(f.request("0.2.0"), Instant::now() + Duration::from_secs(5))
        .is_err());
    assert!(!f.marker("data-worker-started").exists());
}
