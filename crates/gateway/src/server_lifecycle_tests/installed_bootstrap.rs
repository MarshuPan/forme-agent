//! Real encrypted source/key/lock checks; no claim or native service mutation.
use super::{installed_restore::RestoreFixture, *};
use crate::offline_data::{OfflineBootstrapInspection, OfflineCoreMaintenance};
use std::path::Path;

struct BootstrapFixture {
    source: RestoreFixture,
    manifest: PathBuf,
    install: p::InstallationManifest,
    candidate: PathBuf,
    request: p::BootstrapCandidateCatalogRequestV2,
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}

impl BootstrapFixture {
    fn new(backup: bool) -> Self {
        Self::with_authenticated_content(backup, true)
    }

    fn with_authenticated_content(backup: bool, seed_content: bool) -> Self {
        let source = RestoreFixture::new();
        if seed_content {
            let runtime = source.f.build();
            let owner = p::VerifiedPrincipal(source.f.config.owner.0.clone());
            let change = runtime
                .prepare_workspace_charter(
                    &owner,
                    p::WorkspaceCharterRecord {
                        schema_version: p::SchemaVersion(1),
                        workspace: source.f.config.workspace.clone(),
                        version: 0,
                        goals: vec![],
                        constraints: vec![p::Constraint("max-nodes:8".into())],
                        prohibitions: vec![],
                        done_contract: None,
                        review_cadence: None,
                        actor: p::Actor::Owner,
                        digest: p::sha256_content_digest(b"pending"),
                    },
                    0,
                )
                .unwrap();
            runtime
                .apply_workspace_charter(
                    &owner,
                    p::IdempotencyKey("bootstrap:seed-charter".into()),
                    change,
                )
                .unwrap();
            drop(runtime);
        }
        let mut install: p::InstallationManifest =
            serde_json::from_slice(&fs::read(&source.manifest).unwrap()).unwrap();
        install.install_id = "installation:bootstrap-target".into();
        let root = source.f.root.join("new-installation");
        install.paths = p::ProductPaths {
            schema_version: p::SchemaVersion(1),
            releases: root.join("releases"),
            config: root.join("config"),
            brain: root.join("brain"),
            web_auth: root.join("web-auth"),
            host_state: root.join("host"),
            keys: root.join("keys"),
            logs: root.join("logs"),
            backups: root.join("backups"),
            workspaces: root.join("workspaces"),
            ipc: root.join("ipc"),
        };
        for directory in [
            &install.paths.config,
            &install.paths.brain,
            &install.paths.backups,
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        let manifest = install.paths.config.join("installation.json");
        fs::write(&manifest, serde_json::to_vec(&install).unwrap()).unwrap();
        let historical = install.paths.backups.join("historical-brains");
        let backups = install.paths.backups.join("candidates");
        fs::create_dir(&historical).unwrap();
        fs::create_dir(&backups).unwrap();
        let candidate = if backup { &backups } else { &historical }.join("local-source");
        fs::create_dir(&candidate).unwrap();
        let mut core: serde_json::Value =
            serde_json::from_slice(&fs::read(&source.core).unwrap()).unwrap();
        core["brain"]["store_path"] = serde_json::to_value(candidate.join("forme.db")).unwrap();
        core["brain"]["artifact_root"] = serde_json::to_value(candidate.join("artifacts")).unwrap();
        fs::write(
            candidate.join("core.json"),
            serde_json::to_vec(&core).unwrap(),
        )
        .unwrap();
        fs::copy(&source.f.config.store_path, candidate.join("forme.db")).unwrap();
        copy_tree(
            &source.f.config.store_path.with_extension("content-keys"),
            &candidate.join("forme.content-keys"),
        );
        fs::create_dir(candidate.join("artifacts")).unwrap();
        if backup {
            let data = OfflineCoreMaintenance::open(&source.core, None).unwrap();
            data.backup(&candidate.join("backup.db")).unwrap();
        }
        if !seed_content {
            // Deliberately retain just authority metadata in this local fixture.
            // Normal runtime creation also persists protected seed records.
            let connection = rusqlite::Connection::open(candidate.join("forme.db")).unwrap();
            let tables: Vec<String> = connection
                .prepare("SELECT DISTINCT table_name FROM protected_rows")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            let trigger: String = connection
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name='events_immutable_delete'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            connection
                .execute_batch("DROP TRIGGER events_immutable_delete")
                .unwrap();
            for table in tables {
                assert!(table
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'));
                connection
                    .execute(&format!("DELETE FROM {table}"), [])
                    .unwrap();
            }
            connection.execute_batch(&trigger).unwrap();
        }
        let config = serde_json::json!({
            "schema_version":1,"install_ref":install.install_id,
            "journal_path":install.paths.host_state.join("bootstrap.journal"),
            "signing_key_path":install.paths.keys.join("bootstrap.key"),
            "web_credential_path":install.paths.web_auth.join("bootstrap-web.token"),
            "new_brain_directory":install.paths.brain,"candidate_ref":"candidate:new-brain",
            "historical_brain_directory":historical,"verified_backup_directory":backups,
        });
        fs::write(
            install.paths.config.join("bootstrap.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let request = p::BootstrapCandidateCatalogRequestV2 {
            schema_version: p::SchemaVersion(2),
            session: p::BootstrapSessionBindingV2 {
                schema_version: p::SchemaVersion(2),
                operation_id: "claim:inspection".into(),
                setup_digest: p::canonical_digest(&"setup").unwrap(),
            },
        };
        Self {
            source,
            manifest,
            install,
            candidate,
            request,
        }
    }

    fn inspect(&self) -> OfflineBootstrapInspection {
        OfflineBootstrapInspection::open(&self.manifest, &self.request, 1).unwrap()
    }
}

#[test]
fn bootstrap_catalog_verifies_actual_owner_protection_replay_and_holds_source_fence() {
    for backup in [false, true] {
        let f = BootstrapFixture::new(backup);
        let before_entries = tree_snapshot(&f.candidate);
        let before = fs::read(f.candidate.join("forme.db")).unwrap();
        let inspection = f.inspect();
        let catalog = inspection.catalog().unwrap();
        assert_eq!(catalog.candidates.len(), 2);
        let candidate = &catalog.candidates[1];
        assert_eq!(candidate.status, p::BootstrapCandidateStatus::Available);
        assert_eq!(candidate.owner_ref.as_ref(), Some(&f.source.f.config.owner));
        assert_eq!(
            candidate.owner_source,
            p::BootstrapOwnerSource::VerifiedFromCandidate
        );
        assert_eq!(
            candidate.protection_state,
            p::BootstrapProtectionState::Protected
        );
        assert_eq!(
            candidate.kind,
            if backup {
                p::BootstrapCandidateKind::VerifiedBackup
            } else {
                p::BootstrapCandidateKind::ExistingBrain
            }
        );
        let held_path = f
            .candidate
            .join(if backup { "backup.db" } else { "forme.db" });
        #[cfg(windows)]
        assert!(fs::OpenOptions::new().write(true).open(&held_path).is_err());
        #[cfg(not(windows))]
        assert!(fs::File::open(&held_path).unwrap().try_lock().is_err());
        let wire = serde_json::to_string(catalog).unwrap();
        let original_catalog = catalog.clone();
        assert!(!wire.contains(f.candidate.to_str().unwrap()));
        assert!(!wire.contains("local-source"));
        assert!(!wire.contains("password"));
        drop(inspection);
        assert_eq!(catalog_owned(&f), original_catalog);
        assert_eq!(fs::read(f.candidate.join("forme.db")).unwrap(), before);
        assert_eq!(tree_snapshot(&f.candidate), before_entries);
        assert!(!f
            .install
            .paths
            .host_state
            .join("bootstrap.journal")
            .exists());
        assert!(!f.install.paths.brain.join("forme.db").exists());
        assert_eq!(f.source.f.calls.load(Ordering::SeqCst), 0);
    }
}

fn tree_snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, p::SchemaDigest> {
    let mut result = std::collections::BTreeMap::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    p::sha256_content_digest(&fs::read(path).unwrap()),
                );
            }
        }
    }
    result
}

fn catalog_owned(f: &BootstrapFixture) -> p::BootstrapCandidateCatalogV2 {
    f.inspect().catalog().unwrap().clone()
}

#[test]
fn bootstrap_wrong_owner_corrupt_database_and_unknown_schema_never_become_empty_brain() {
    for case in ["owner", "corruption", "schema", "path", "key", "plaintext"] {
        let f = BootstrapFixture::new(false);
        let core = f.candidate.join("core.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&fs::read(&core).unwrap()).unwrap();
        match case {
            "owner" => config["brain"]["owner"] = serde_json::json!("owner:wrong"),
            "schema" => config["schema_version"] = serde_json::json!(99),
            "path" => {
                config["brain"]["store_path"] =
                    serde_json::to_value(&f.source.f.config.store_path).unwrap()
            }
            "key" => {
                fs::rename(
                    f.candidate.join("forme.content-keys"),
                    f.candidate.join("retained-original-keys"),
                )
                .unwrap();
                fs::create_dir(f.candidate.join("forme.content-keys")).unwrap();
            }
            "plaintext" => {
                let connection = rusqlite::Connection::open(f.candidate.join("forme.db")).unwrap();
                // Construct a historical plaintext fixture, bypassing only the
                // fixture's append-only trigger; discovery must never do this.
                let trigger: String = connection
                    .query_row(
                        "SELECT sql FROM sqlite_master WHERE name='events_immutable_update'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                connection
                    .execute_batch("DROP TRIGGER events_immutable_update")
                    .unwrap();
                assert!(
                    connection
                        .execute("UPDATE events SET payload=?1", [b"{}".as_slice()])
                        .unwrap()
                        > 0
                );
                connection.execute_batch(&trigger).unwrap();
            }
            _ => {
                let mut bytes = fs::read(f.candidate.join("forme.db")).unwrap();
                bytes[0] ^= 1;
                fs::write(f.candidate.join("forme.db"), bytes).unwrap();
            }
        }
        fs::write(&core, serde_json::to_vec(&config).unwrap()).unwrap();
        let before = tree_snapshot(&f.candidate);
        let inspection = f.inspect();
        let catalog = inspection.catalog().unwrap();
        assert_eq!(catalog.candidates.len(), 2);
        assert_ne!(
            catalog.candidates[1].status,
            p::BootstrapCandidateStatus::Available,
            "{case}"
        );
        assert!(catalog.candidates[1].owner_ref.is_none());
        assert!(catalog.candidates[1].issue_ref.is_some());
        assert!(!f.install.paths.brain.join("forme.db").exists());
        drop(inspection);
        assert_eq!(
            tree_snapshot(&f.candidate),
            before,
            "source changed for {case}"
        );
    }
}

#[test]
fn bootstrap_owner_digest_without_authenticated_content_is_unverifiable() {
    let f = BootstrapFixture::with_authenticated_content(false, false);
    let data = OfflineCoreMaintenance::open(&f.candidate.join("core.json"), None).unwrap();
    assert_eq!(data.identity().authenticated_content_rows, 0);
    drop(data);
    let before = tree_snapshot(&f.candidate);
    let inspection = f.inspect();
    let candidate = &inspection.catalog().unwrap().candidates[1];
    assert_eq!(candidate.status, p::BootstrapCandidateStatus::Unverifiable);
    assert!(candidate.owner_ref.is_none());
    drop(inspection);
    assert_eq!(tree_snapshot(&f.candidate), before);
}

#[test]
fn bootstrap_aliases_and_busy_sources_remain_visible_and_unverifiable() {
    let f = BootstrapFixture::new(false);
    fs::hard_link(f.candidate.join("forme.db"), f.candidate.join("alias.db")).unwrap();
    let inspection = f.inspect();
    assert_eq!(
        inspection.catalog().unwrap().candidates[1].status,
        p::BootstrapCandidateStatus::Unverifiable
    );
    assert!(inspection.catalog().unwrap().candidates[1]
        .owner_ref
        .is_none());
}

#[test]
fn bootstrap_existing_fence_is_respected_without_recreating_or_rebinding_it() {
    let f = BootstrapFixture::new(false);
    let fence = f.candidate.join("forme.access-lock");
    fs::write(&fence, []).unwrap();
    let before = tree_snapshot(&f.candidate);
    let inspection = f.inspect();
    assert_eq!(
        inspection.catalog().unwrap().candidates[1].status,
        p::BootstrapCandidateStatus::Available
    );
    assert!(OfflineCoreMaintenance::open(&f.candidate.join("core.json"), None).is_err());
    drop(inspection);
    assert_eq!(tree_snapshot(&f.candidate), before);
    let alias = f.candidate.join("aliased-fence");
    fs::hard_link(&fence, alias).unwrap();
    assert_ne!(
        f.inspect().catalog().unwrap().candidates[1].status,
        p::BootstrapCandidateStatus::Available
    );
}

#[test]
fn bootstrap_epoch_digest_and_source_identity_are_bound_to_real_observation() {
    let f = BootstrapFixture::new(false);
    let first = catalog_owned(&f);
    let next = OfflineBootstrapInspection::open(&f.manifest, &f.request, 2)
        .unwrap()
        .catalog()
        .unwrap()
        .clone();
    assert_ne!(first.catalog_digest, next.catalog_digest);
    assert_ne!(
        first.candidates[1].candidate_digest,
        next.candidates[1].candidate_digest
    );
    assert_eq!(
        first.candidates[1].source_observation_digest,
        next.candidates[1].source_observation_digest
    );
    assert!(OfflineBootstrapInspection::open(&f.manifest, &f.request, 0).is_err());
    let alias = f.install.paths.config.join("manifest-alias.json");
    fs::copy(&f.manifest, &alias).unwrap();
    assert!(OfflineBootstrapInspection::open(&alias, &f.request, 1).is_err());
}
