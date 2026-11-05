//! Original temporary files and independent keys; no installed-user fixtures.
use std::{fs, path::PathBuf, time::Duration};

use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};

use super::{objects::*, *};

fn digest(value: &str) -> p::SchemaDigest {
    p::canonical_digest(&value).unwrap()
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

struct Fixture {
    _temp: tempfile::TempDir,
    installation: p::InstallationManifest,
    binding: p::LegacyReleaseReviewBinding,
    original: p::ProductReleaseRequest,
    key: SigningKey,
    origin: ArchiveOrigin,
    trust: LegacyDataArchiveTrust,
    bytes: Vec<u8>,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        let paths = p::ProductPaths {
            schema_version: SCHEMA,
            releases: base.join("releases"),
            config: base.join("config"),
            brain: base.join("brain"),
            web_auth: base.join("web-auth"),
            host_state: base.join("host-state"),
            keys: base.join("keys"),
            logs: base.join("logs"),
            backups: base.join("backups"),
            workspaces: base.join("workspaces"),
            ipc: base.join("ipc"),
        };
        let target = p::ProductTarget::WindowsX86_64;
        let installation = p::InstallationManifest {
            schema_version: SCHEMA,
            install_id: "archive-fixture-installation".into(),
            target,
            release_id: "archive-original".into(),
            release_digest: digest("fixture original release"),
            paths,
            services: [
                p::BuiltinServiceId::Host,
                p::BuiltinServiceId::Web,
                p::BuiltinServiceId::Brain,
            ]
            .into_iter()
            .map(|service| p::ServiceIdentityBinding {
                schema_version: SCHEMA,
                service,
                account: service.required_account(target).into(),
            })
            .collect(),
            created_at: 0,
        };
        let owner = p::OwnerPrincipalRef("owner:archive-isolated".into());
        let original = p::ProductReleaseRequest {
            schema_version: SCHEMA,
            install_ref: installation.install_id.clone(),
            operation_id: "upgrade:archive-isolated".into(),
            command_digest: digest("fixture command"),
            change: p::ProductReleaseChange::Upgrade,
            original_release_ref: installation.release_id.clone(),
            original_release_digest: installation.release_digest.clone(),
            target_release_ref: "archive-target".into(),
            target_release_digest: digest("fixture target release"),
            backup_ref: "backup:archive-isolated".into(),
        };
        let binding = p::LegacyReleaseReviewBinding {
            schema_version: SCHEMA,
            review_id: "review:archive-isolated".into(),
            install_ref: installation.install_id.clone(),
            owner: owner.clone(),
            original_operation_id: original.operation_id.clone(),
            original_command_digest: original.command_digest.clone(),
            original_release_ref: installation.release_id.clone(),
            original_release_digest: installation.release_digest.clone(),
            config_revision: 1,
            config_digest: digest("fixture unchanged configuration"),
            journal_seq: 23,
            journal_head: digest("fixture full original journal"),
            expires_at: 99_000,
        };
        let archive = DataArchive {
            schema_version: SCHEMA,
            operation: "fixture:offline-inbox".into(),
            outcome: "inbox_prepared_only".into(),
            prepare_inbox: true,
            release_id: original.target_release_ref.clone(),
            release_digest: original.target_release_digest.clone(),
            changes: [
                "created-private-inbox-parent",
                "created-private-preparation-directory",
                "published-exact-signed-inbox",
            ]
            .map(str::to_owned)
            .to_vec(),
            failure_stage: None,
            provider_called: false,
            provider_keys_read: false,
            service_actions: 0,
            maintenance_commands_submitted: 0,
            program_images_changed: false,
            database_schema_version: None,
            runtime_compatibility_verified: false,
            offline_identity: ArchivedIdentity {
                schema_version: SCHEMA,
                owner_digest: p::sha256_content_digest(owner.0.as_bytes()),
                store_identity: digest("fixture store identity"),
                schema_digest: digest("fixture database schema"),
                database_digest: digest("fixture database bytes"),
                event_count: 13,
                last_stream_seq: 13,
                authenticated_content_rows: 8,
            },
            product_revision: 11,
            config_schema_version: 1,
            actual_database_schema_required_during_upgrade: true,
            services: [
                "forme-host",
                "forme-web",
                "forme-brain",
                "forme-release-updater",
            ]
            .into_iter()
            .map(|name| ArchivedService {
                name: name.into(),
                state: "Stopped".into(),
                start_name: match name {
                    "forme-web" | "forme-brain" => format!("NT SERVICE\\{name}"),
                    _ => "LocalSystem".into(),
                },
                start_mode: "Manual".into(),
                process_id: 0,
                path_name: format!("C:\\isolated-archive-fixture\\{name}.exe"),
            })
            .collect(),
            copied_file_count: 7,
        };
        let bytes = serde_json::to_vec(&archive).unwrap();
        let archive_digest = p::sha256_content_digest(&bytes);
        let origin = ArchiveOrigin {
            schema_version: SCHEMA,
            source_ref: "source:independent-archive-fixture".into(),
            install_ref: installation.install_id.clone(),
            installation_digest: installation.digest().unwrap(),
            owner,
            original_operation_id: original.operation_id.clone(),
            original_command_digest: original.command_digest.clone(),
            original_release_ref: installation.release_id.clone(),
            original_release_digest: installation.release_digest.clone(),
            target_release_ref: original.target_release_ref.clone(),
            target_release_digest: original.target_release_digest.clone(),
            config_revision: binding.config_revision,
            config_digest: binding.config_digest.clone(),
            archive_digest: archive_digest.clone(),
            archive_revision: archive.product_revision,
            recorded_at: 100,
        };
        let key = SigningKey::from_bytes(&[43; 32]);
        let trust = LegacyDataArchiveTrust::from_pinned_origin(
            archive_digest,
            digest("not yet published"),
            key.verifying_key().to_bytes(),
        )
        .unwrap();
        let mut f = Self {
            _temp: temp,
            installation,
            binding,
            original,
            key,
            origin,
            trust,
            bytes,
            root: PathBuf::new(),
        };
        f.publish();
        f
    }

    fn publish(&mut self) {
        self.origin.archive_digest = p::sha256_content_digest(&self.bytes);
        self.root = self
            .installation
            .paths
            .host_state
            .join("legacy-data-archives")
            .join(&self.origin.archive_digest.0[7..]);
        fs::create_dir_all(&self.root).unwrap();
        fs::write(self.root.join("archive.json"), &self.bytes).unwrap();
        let signed = SignedArchiveOrigin {
            schema_version: SCHEMA,
            origin: self.origin.clone(),
            signature: self
                .key
                .sign(self.origin.digest().unwrap().0.as_bytes())
                .to_bytes()
                .to_vec(),
        };
        let bytes = serde_json::to_vec(&signed).unwrap();
        fs::write(self.root.join("origin.json"), &bytes).unwrap();
        self.trust = LegacyDataArchiveTrust::from_pinned_origin(
            self.origin.archive_digest.clone(),
            p::sha256_content_digest(&bytes),
            self.key.verifying_key().to_bytes(),
        )
        .unwrap();
    }

    fn edit_archive(&mut self, edit: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_slice(&self.bytes).unwrap();
        edit(&mut value);
        self.bytes = serde_json::to_vec(&value).unwrap();
        self.publish();
    }

    fn load(&self) -> p::Result<LegacyDataArchiveEvidence> {
        LegacyDataArchiveEvidence::load(
            &self.installation,
            &self.trust,
            &self.binding,
            &self.original,
            200,
            deadline(),
        )
    }

    fn current(&self) -> p::ProductReleaseDataIdentity {
        let archive: DataArchive = serde_json::from_slice(&self.bytes).unwrap();
        let old = archive.offline_identity;
        p::ProductReleaseDataIdentity {
            schema_version: SCHEMA,
            config_schema_version: 1,
            database_schema_version: 4,
            owner_digest: old.owner_digest,
            store_identity: old.store_identity,
            schema_digest: old.schema_digest,
            database_digest: old.database_digest,
            event_count: old.event_count,
            last_stream_seq: old.last_stream_seq,
        }
    }
}

#[test]
fn signed_origin_and_current_identity_produce_only_one_bound_evidence_without_writes() {
    let f = Fixture::new();
    let before = fs::read(f.root.join("origin.json")).unwrap();
    let evidence = f
        .load()
        .unwrap()
        .compare_current(&f.current(), deadline())
        .unwrap();
    assert_eq!(
        evidence.kind,
        p::LegacyReleaseEvidenceKind::ArchivedDataIdentity
    );
    assert_eq!(evidence.artifact_digest, f.trust.archive_digest);
    evidence.validate_for(&f.binding.digest().unwrap()).unwrap();
    assert_eq!(fs::read(f.root.join("archive.json")).unwrap(), f.bytes);
    assert_eq!(fs::read(f.root.join("origin.json")).unwrap(), before);
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 2);
    assert!(!f
        .installation
        .paths
        .host_state
        .join("host.journal")
        .exists());
    assert!(!f.installation.paths.brain.exists());
    let safe = serde_json::to_string(&evidence).unwrap();
    assert!(!safe.contains("isolated-archive-fixture"));
    assert!(!safe.contains("owner:archive-isolated"));
}

#[test]
fn missing_origin_and_unpinned_or_self_replaced_origin_are_rejected() {
    let f = Fixture::new();
    fs::remove_file(f.root.join("origin.json")).unwrap();
    assert!(f.load().is_err());
    let mut f = Fixture::new();
    f.trust.origin_digest = digest("a different independently recorded source");
    assert!(f.load().is_err());
    let mut f = Fixture::new();
    let old_trust = f.trust.origin_digest.clone();
    f.origin.source_ref = "source:replacement".into();
    f.publish();
    f.trust.origin_digest = old_trust;
    assert!(f.load().is_err());
}

#[test]
fn changed_bytes_are_rejected_even_when_the_file_still_parses() {
    let f = Fixture::new();
    let mut bytes = f.bytes.clone();
    bytes.push(b' ');
    fs::write(f.root.join("archive.json"), bytes).unwrap();
    assert!(f.load().is_err());
}

#[test]
fn signature_requires_independent_key_and_correct_domain() {
    let mut f = Fixture::new();
    f.trust.key = SigningKey::from_bytes(&[47; 32]).verifying_key();
    assert!(f.load().is_err());
    let mut f = Fixture::new();
    let mut value: SignedArchiveOrigin =
        serde_json::from_slice(&fs::read(f.root.join("origin.json")).unwrap()).unwrap();
    value.signature = f
        .key
        .sign(p::canonical_digest(&f.origin).unwrap().0.as_bytes())
        .to_bytes()
        .to_vec();
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::write(f.root.join("origin.json"), &bytes).unwrap();
    f.trust.origin_digest = p::sha256_content_digest(&bytes);
    assert!(f.load().is_err());
}

#[test]
fn origin_cannot_be_from_another_install_owner_operation_release_or_config() {
    for case in 0..12 {
        let mut f = Fixture::new();
        match case {
            0 => f.origin.install_ref = "another-installation".into(),
            1 => f.origin.installation_digest = digest("another installation manifest"),
            2 => f.origin.owner = p::OwnerPrincipalRef("owner:someone-else".into()),
            3 => f.origin.original_operation_id = "another-operation".into(),
            4 => f.origin.original_command_digest = digest("another command"),
            5 => f.origin.original_release_ref = "another-release".into(),
            6 => f.origin.original_release_digest = digest("another release bytes"),
            7 => f.origin.target_release_ref = "another-target".into(),
            8 => f.origin.target_release_digest = digest("another target bytes"),
            9 => f.origin.config_revision += 1,
            10 => f.origin.config_digest = digest("a changed configuration"),
            _ => f.origin.schema_version = p::SchemaVersion(2),
        }
        f.publish();
        assert!(f.load().is_err(), "source binding case {case}");
    }
}

#[test]
fn origin_time_and_archive_watermark_must_precede_original_operation() {
    for case in 0..5 {
        let mut f = Fixture::new();
        match case {
            0 => f.origin.recorded_at = -1,
            1 => f.origin.recorded_at = 200,
            2 => f.origin.recorded_at = 201,
            3 => f.origin.archive_revision = 0,
            _ => f.origin.archive_revision = f.binding.journal_seq,
        }
        f.publish();
        assert!(f.load().is_err(), "temporal source case {case}");
    }
}

#[test]
fn explicit_unknown_historical_schema_stays_null_and_current_schema_enters_observation() {
    let f = Fixture::new();
    let loaded = f.load().unwrap();
    let mut current = f.current();
    let first = loaded.compare_current(&current, deadline()).unwrap();
    current.database_schema_version = 5;
    let second = loaded.compare_current(&current, deadline()).unwrap();
    assert_ne!(first.observation_digest, second.observation_digest);
    let archive: Value =
        serde_json::from_slice(&fs::read(f.root.join("archive.json")).unwrap()).unwrap();
    assert!(archive["database_schema_version"].is_null());
    current.database_schema_version = 0;
    assert!(loaded.compare_current(&current, deadline()).is_err());
}

#[test]
fn missing_and_zero_historical_schema_cannot_be_inferred() {
    let mut f = Fixture::new();
    f.edit_archive(|v| {
        v.as_object_mut().unwrap().remove("database_schema_version");
    });
    assert!(f.load().is_err());
    let mut f = Fixture::new();
    f.edit_archive(|v| v["database_schema_version"] = json!(0));
    assert!(f.load().is_err());
    let mut f = Fixture::new();
    f.edit_archive(|v| v["database_schema_version"] = json!(4));
    let loaded = f.load().unwrap();
    let mut current = f.current();
    loaded.compare_current(&current, deadline()).unwrap();
    current.database_schema_version = 5;
    assert!(loaded.compare_current(&current, deadline()).is_err());
}

#[test]
fn identity_drift_is_rejected_field_by_field() {
    let f = Fixture::new();
    let loaded = f.load().unwrap();
    for case in 0..8 {
        let mut current = f.current();
        match case {
            0 => current.owner_digest = digest("another owner"),
            1 => current.store_identity = digest("another store"),
            2 => current.schema_digest = digest("different schema bytes"),
            3 => current.database_digest = digest("changed database bytes"),
            4 => current.event_count += 1,
            5 => current.last_stream_seq += 1,
            6 => current.config_schema_version += 1,
            _ => current.schema_version = p::SchemaVersion(2),
        }
        assert!(
            loaded.compare_current(&current, deadline()).is_err(),
            "identity case {case}"
        );
    }
}

#[test]
fn archived_owner_uses_the_store_raw_utf8_digest_not_canonical_json() {
    // Independently calculated SHA-256 golden for the store's owner encoding.
    // Both fixture and comparator using JSON hashing would otherwise agree.
    let mut f = Fixture::new();
    f.edit_archive(|v| {
        v["offline_identity"]["owner_digest"] =
            json!("sha256:8976989464b67ba0de13007afb9be9021ee804fb14615f8a3a0e3a32275053d8")
    });
    f.load()
        .unwrap()
        .compare_current(&f.current(), deadline())
        .unwrap();
    let json_digest = p::canonical_digest(&f.binding.owner).unwrap();
    f.edit_archive(|v| v["offline_identity"]["owner_digest"] = json!(json_digest));
    assert!(f.load().is_err());
}

#[test]
fn a_signed_archive_still_requires_matching_semantics() {
    for (field, value) in [
        ("outcome", json!("upgrade_succeeded")),
        ("prepare_inbox", json!(false)),
        ("release_id", json!("different-target")),
        ("release_digest", json!(digest("different target bytes"))),
        ("failure_stage", json!("copy")),
        ("provider_called", json!(true)),
        ("provider_keys_read", json!(true)),
        ("service_actions", json!(1)),
        ("maintenance_commands_submitted", json!(1)),
        ("program_images_changed", json!(true)),
        ("runtime_compatibility_verified", json!(true)),
        ("config_schema_version", json!(0)),
        ("product_revision", json!(12)),
        (
            "actual_database_schema_required_during_upgrade",
            json!(false),
        ),
        ("copied_file_count", json!(0)),
        ("changes", json!(["published-exact-signed-inbox"])),
    ] {
        let mut f = Fixture::new();
        f.edit_archive(|v| v[field] = value);
        assert!(f.load().is_err(), "semantic field {field}");
    }
}

#[test]
fn closed_formats_reject_unknown_duplicate_wrong_type_and_missing_fields() {
    for case in 0..7 {
        let mut f = Fixture::new();
        match case {
            0 => f.edit_archive(|v| v["verified"] = json!(true)),
            1 => f.edit_archive(|v| v["offline_identity"]["path"] = json!("C:\\untrusted")),
            2 => f.edit_archive(|v| v["services"][0]["Trusted"] = json!(true)),
            3 => f.edit_archive(|v| v["provider_called"] = json!("false")),
            4 => f.edit_archive(|v| {
                v.as_object_mut().unwrap().remove("failure_stage");
            }),
            5 => f.edit_archive(|v| {
                v["offline_identity"]["owner_digest"] = json!(digest("wrong owner"))
            }),
            _ => {
                f.bytes.pop();
                f.bytes.extend_from_slice(b",\"schema_version\":1}");
                f.publish();
            }
        }
        assert!(f.load().is_err(), "closed archive case {case}");
    }
    let mut f = Fixture::new();
    let mut origin: Value =
        serde_json::from_slice(&fs::read(f.root.join("origin.json")).unwrap()).unwrap();
    origin["public_key"] = json!(f.key.verifying_key().to_bytes());
    let bytes = serde_json::to_vec(&origin).unwrap();
    fs::write(f.root.join("origin.json"), &bytes).unwrap();
    f.trust.origin_digest = p::sha256_content_digest(&bytes);
    assert!(f.load().is_err());
}

#[test]
fn duplicate_inconsistent_or_incomplete_historical_services_are_rejected() {
    for case in 0..6 {
        let mut f = Fixture::new();
        f.edit_archive(|v| match case {
            0 => v["services"][0] = v["services"][1].clone(),
            1 => v["services"][2]["ProcessId"] = json!(42),
            2 => v["services"][2]["StartName"] = json!("LocalSystem"),
            3 => v["services"][3]["State"] = json!("Running"),
            4 => v["services"][0]["PathName"] = json!("bad\npath"),
            _ => {
                v["services"].as_array_mut().unwrap().pop();
            }
        });
        assert!(f.load().is_err(), "service archive case {case}");
    }
}

#[test]
fn extra_files_missing_files_and_hardlink_aliases_are_rejected() {
    let f = Fixture::new();
    fs::write(f.root.join("extra.json"), b"{}").unwrap();
    assert!(f.load().is_err());
    let f = Fixture::new();
    fs::remove_file(f.root.join("archive.json")).unwrap();
    assert!(f.load().is_err());
    let f = Fixture::new();
    fs::hard_link(
        f.root.join("archive.json"),
        f._temp.path().join("aliased-archive.json"),
    )
    .unwrap();
    assert!(f.load().is_err());
}

#[test]
fn archive_and_origin_size_limits_are_enforced_before_decoding() {
    let f = Fixture::new();
    fs::write(
        f.root.join("origin.json"),
        vec![b' '; ORIGIN_LIMIT as usize + 1],
    )
    .unwrap();
    assert!(f.load().is_err());
    let f = Fixture::new();
    fs::write(
        f.root.join("archive.json"),
        vec![b' '; ARCHIVE_LIMIT as usize + 1],
    )
    .unwrap();
    assert!(f.load().is_err());
}

#[test]
fn file_changes_are_either_blocked_by_custody_or_detected_on_recheck() {
    let f = Fixture::new();
    let loaded = f.load().unwrap();
    let replacement = fs::write(f.root.join("archive.json"), b"{}");
    #[cfg(windows)]
    assert!(replacement.is_err());
    if replacement.is_ok() {
        assert!(loaded.recheck(deadline()).is_err());
        assert!(loaded.compare_current(&f.current(), deadline()).is_err());
    }
    fs::write(f.root.join("injected.json"), b"{}").unwrap();
    assert!(loaded.recheck(deadline()).is_err());
}

#[test]
fn expired_monotonic_deadline_cannot_load_or_compare() {
    let f = Fixture::new();
    let expired = Instant::now() - Duration::from_millis(1);
    assert!(LegacyDataArchiveEvidence::load(
        &f.installation,
        &f.trust,
        &f.binding,
        &f.original,
        200,
        expired
    )
    .is_err());
    assert!(f
        .load()
        .unwrap()
        .compare_current(&f.current(), expired)
        .is_err());
}
