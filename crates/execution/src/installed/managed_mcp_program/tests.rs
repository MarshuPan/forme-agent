//! Real Windows files and restart replay; no installed-product acceptance claim.
use super::*;
use std::fs;

mod host;

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    journal: PathBuf,
    source: ManagedMcpProgramSource,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("programs");
        fs::create_dir(&root).unwrap();
        let program = root.join("fixture.exe");
        fs::write(&program, b"inert registration fixture; never executed").unwrap();
        let source = ManagedMcpProgramSource {
            schema_version: p::SchemaVersion(1),
            source_ref: "source:fixture".into(),
            scope: p::Scope("workspace:fixture".into()),
            sandbox_profile_ref: p::MANAGED_MCP_PROGRAM_PROFILE.into(),
            expected_program_digest: crate::installed::file_digest(&program).unwrap(),
            program,
            args: vec!["--fixed-argument".into()],
        };
        let journal = temp.path().join("programs.jsonl");
        let fixture = Self {
            _temp: temp,
            root,
            journal,
            source,
        };
        fixture.write_sources(std::slice::from_ref(&fixture.source));
        fixture
    }
    fn write_sources(&self, sources: &[ManagedMcpProgramSource]) {
        fs::write(
            self.root.join("sources.json"),
            serde_json::to_vec(&serde_json::json!({"schema_version":1,"sources":sources})).unwrap(),
        )
        .unwrap();
    }
    fn open(&self) -> p::Result<ManagedMcpProgramRegistry> {
        ManagedMcpProgramRegistry::open(
            &self.journal,
            "install:fixture".into(),
            owner(),
            p::AuthorityEpoch(3),
            &self.root,
            std::slice::from_ref(&self.source.scope),
            &[],
        )
    }
    fn register_command(
        &self,
        registry: &ManagedMcpProgramRegistry,
    ) -> p::ManagedMcpProgramCommand {
        let catalog = registry.catalog().unwrap();
        p::ManagedMcpProgramCommand {
            schema_version: p::SchemaVersion(1),
            installation_ref: catalog.installation_ref,
            owner_ref: catalog.owner_ref,
            authority_epoch: catalog.authority_epoch,
            operation_id: "register:fixture".into(),
            expected_revision: catalog.revision,
            requested_deadline: 10_000,
            action: p::ManagedMcpProgramAction::Register {
                source_ref: catalog.choices[0].source_ref.clone(),
                source_digest: catalog.choices[0].source_digest.clone(),
            },
        }
    }
    fn register(&self, registry: &ManagedMcpProgramRegistry) -> p::ManagedMcpProgramOperation {
        registry
            .apply(self.register_command(registry), 1_000)
            .unwrap()
    }
    fn lease(
        &self,
        registry: &ManagedMcpProgramRegistry,
        view: &p::ManagedMcpProgramView,
    ) -> p::Result<ManagedMcpProgramLease> {
        registry.acquire(
            &owner(),
            p::AuthorityEpoch(3),
            &view.program_ref,
            &view.registration_digest,
            &self.source.scope,
        )
    }
}
fn owner() -> p::OwnerPrincipalRef {
    p::OwnerPrincipalRef("owner:fixture".into())
}
fn revoke(
    command: &p::ManagedMcpProgramCommand,
    operation: &p::ManagedMcpProgramOperation,
) -> p::ManagedMcpProgramCommand {
    let mut command = command.clone();
    command.operation_id = "revoke:fixture".into();
    command.expected_revision = operation.revision;
    command.action = p::ManagedMcpProgramAction::Revoke {
        program_ref: operation.program.program_ref.clone(),
        registration_digest: operation.program.registration_digest.clone(),
    };
    command
}

#[test]
fn registry_custody_blocks_real_write_delete_and_ancestor_replacement_until_lease_drops() {
    let fixture = Fixture::new();
    let registry = fixture.open().unwrap();
    let operation = fixture.register(&registry);
    let lease = fixture.lease(&registry, &operation.program).unwrap();
    assert_eq!(lease.launch().unwrap().args, fixture.source.args);
    assert_eq!(lease.view(), &operation.program);
    assert!(fs::write(&fixture.source.program, b"replacement").is_err());
    assert!(fs::remove_file(&fixture.source.program).is_err());
    assert!(fs::rename(&fixture.root, fixture.root.with_extension("moved")).is_err());
    assert!(fs::write(fixture.root.join("sources.json"), b"{}").is_err());
    assert_eq!(registry.with_active(&lease, || Ok(17)).unwrap(), 17);
    drop(lease);
    fs::write(&fixture.source.program, b"replacement").unwrap();
    assert!(fixture.lease(&registry, &operation.program).is_err());
}

#[test]
fn registry_same_bytes_new_file_identity_invalidates_original_registration() {
    let fixture = Fixture::new();
    let registry = fixture.open().unwrap();
    let command = fixture.register_command(&registry);
    let operation = registry.apply(command.clone(), 1_000).unwrap();
    let before = registry.catalog().unwrap().choices[0].source_digest.clone();
    let renamed = fixture.root.join("original.exe");
    fs::rename(&fixture.source.program, renamed).unwrap();
    fs::write(
        &fixture.source.program,
        b"inert registration fixture; never executed",
    )
    .unwrap();
    assert_ne!(registry.catalog().unwrap().choices[0].source_digest, before);
    assert!(fixture.lease(&registry, &operation.program).is_err());
    assert_eq!(registry.apply(command, 100_000).unwrap(), operation);
}

#[test]
fn registry_revoke_is_durable_blocks_existing_lease_and_replay_never_revives_it() {
    let fixture = Fixture::new();
    let registry = fixture.open().unwrap();
    let command = fixture.register_command(&registry);
    let operation = registry.apply(command.clone(), 1_000).unwrap();
    let lease = fixture.lease(&registry, &operation.program).unwrap();
    let revocation = revoke(&command, &operation);
    let result = registry.apply(revocation.clone(), 2_000).unwrap();
    assert!(result.program.revoked);
    let mut called = false;
    assert!(registry
        .with_active(&lease, || {
            called = true;
            Ok(())
        })
        .is_err());
    assert!(!called);
    assert!(fixture.lease(&registry, &operation.program).is_err());
    assert_eq!(registry.apply(command.clone(), 100_000).unwrap(), operation);
    assert_eq!(registry.apply(revocation.clone(), 100_000).unwrap(), result);
    drop(lease);
    drop(registry);
    let reopened = fixture.open().unwrap();
    assert!(reopened.catalog().unwrap().programs[0].revoked);
    assert_eq!(
        reopened.operation(&operation.operation_id).unwrap(),
        operation
    );
    assert_eq!(
        reopened.operation(&revocation.operation_id).unwrap(),
        result
    );
    assert!(fixture.lease(&reopened, &operation.program).is_err());
    let mut new_command = fixture.register_command(&reopened);
    new_command.operation_id = "register:new".into();
    let new_operation = reopened.apply(new_command, 3_000).unwrap();
    assert_ne!(
        new_operation.program.program_ref,
        operation.program.program_ref
    );
}

#[test]
fn registry_identity_cas_deadline_drift_and_operation_collision_are_zero_append() {
    let fixture = Fixture::new();
    let registry = fixture.open().unwrap();
    let command = fixture.register_command(&registry);
    let before = fs::metadata(&fixture.journal).unwrap().len();
    for index in 0..7 {
        let mut changed = command.clone();
        match index {
            0 => changed.owner_ref.0 = "owner:other".into(),
            1 => changed.installation_ref = "install:other".into(),
            2 => changed.authority_epoch.0 += 1,
            3 => changed.expected_revision += 1,
            4 => changed.requested_deadline = 999,
            5 => changed.requested_deadline = 1_000_000,
            _ => {
                changed.action = p::ManagedMcpProgramAction::Register {
                    source_ref: fixture.source.source_ref.clone(),
                    source_digest: p::sha256_content_digest(b"different"),
                }
            }
        }
        assert!(registry.apply(changed, 1_000).is_err());
        assert_eq!(fs::metadata(&fixture.journal).unwrap().len(), before);
    }
    let registered = registry.apply(command.clone(), 1_000).unwrap();
    let length = fs::metadata(&fixture.journal).unwrap().len();
    let mut collision = command;
    collision.requested_deadline += 1;
    assert!(registry.apply(collision, 1_000).is_err());
    assert_eq!(fs::metadata(&fixture.journal).unwrap().len(), length);
    for (principal, epoch, scope) in [
        (
            p::OwnerPrincipalRef("owner:other".into()),
            p::AuthorityEpoch(3),
            fixture.source.scope.clone(),
        ),
        (owner(), p::AuthorityEpoch(4), fixture.source.scope.clone()),
        (
            owner(),
            p::AuthorityEpoch(3),
            p::Scope("workspace:other".into()),
        ),
    ] {
        assert!(registry
            .acquire(
                &principal,
                epoch,
                &registered.program.program_ref,
                &registered.program.registration_digest,
                &scope
            )
            .is_err());
    }
}

#[test]
fn registry_replay_requires_original_owner_epoch_complete_hash_chain_and_single_writer() {
    let fixture = Fixture::new();
    let registry = fixture.open().unwrap();
    let operation = fixture.register(&registry);
    assert!(fixture.open().is_err());
    drop(registry);
    let original = fs::read(&fixture.journal).unwrap();
    for (principal, epoch) in [
        (p::OwnerPrincipalRef("owner:other".into()), 3),
        (owner(), 4),
    ] {
        assert!(ManagedMcpProgramRegistry::open(
            &fixture.journal,
            "install:fixture".into(),
            principal,
            p::AuthorityEpoch(epoch),
            &fixture.root,
            std::slice::from_ref(&fixture.source.scope),
            &[]
        )
        .is_err());
        assert_eq!(fs::read(&fixture.journal).unwrap(), original);
    }
    let reopened = fixture.open().unwrap();
    assert_eq!(
        reopened.operation(&operation.operation_id).unwrap(),
        operation
    );
    drop(reopened);
    for bytes in [
        original[..original.len() - 1].to_vec(),
        [original.as_slice(), b"{}"].concat(),
        original
            .iter()
            .map(|b| if *b == b'3' { b'4' } else { *b })
            .collect(),
    ] {
        fs::write(&fixture.journal, &bytes).unwrap();
        assert!(fixture.open().is_err());
        assert_eq!(fs::read(&fixture.journal).unwrap(), bytes);
    }
}

#[test]
fn registry_source_hardlinks_wrong_digest_escape_and_overrides_are_rejected() {
    let fixture = Fixture::new();
    fs::hard_link(&fixture.source.program, fixture.root.join("alias.exe")).unwrap();
    let registry = fixture.open().unwrap();
    assert!(registry.catalog().is_err());
    drop(registry);
    fs::remove_file(fixture.root.join("alias.exe")).unwrap();
    for index in 0..5 {
        let mut source = fixture.source.clone();
        match index {
            0 => source.expected_program_digest = p::sha256_content_digest(b"wrong"),
            1 => {
                source.program = fixture.root.parent().unwrap().join("outside.exe");
                fs::write(&source.program, b"outside").unwrap();
            }
            2 => source.sandbox_profile_ref = "unrestricted".into(),
            3 => source.args = vec!["--injected\nargument".into()],
            _ => source.scope = p::Scope("workspace:other".into()),
        }
        fixture.write_sources(&[source]);
        if let Ok(registry) = fixture.open() {
            assert!(registry.catalog().is_err());
        }
    }
}

#[test]
fn registry_reopen_preserves_revocation_access_when_program_is_missing_or_definition_changed() {
    let fixture = Fixture::new();
    let registry = fixture.open().unwrap();
    let command = fixture.register_command(&registry);
    let operation = registry.apply(command.clone(), 1_000).unwrap();
    drop(registry);
    fs::remove_file(&fixture.source.program).unwrap();
    let registry = fixture.open().unwrap();
    assert!(fixture.lease(&registry, &operation.program).is_err());
    let result = registry.apply(revoke(&command, &operation), 2_000).unwrap();
    assert!(result.program.revoked);
    drop(registry);
    fs::write(
        &fixture.source.program,
        b"inert registration fixture; never executed",
    )
    .unwrap();
    let mut changed = fixture.source.clone();
    changed.args.push("--new".into());
    fixture.write_sources(&[changed]);
    let reopened = fixture.open().unwrap();
    assert!(fixture.lease(&reopened, &operation.program).is_err());
    assert!(reopened.catalog().unwrap().programs[0].revoked);
}

#[test]
fn registry_elapsed_admission_window_cannot_renew_or_append_after_waits() {
    let fixture = Fixture::new();
    let registry = fixture.open().unwrap();
    let command = fixture.register_command(&registry);
    let before = fs::metadata(&fixture.journal).unwrap().len();
    let expired_window = CommandWindow {
        received_at: 1_000,
        started: std::time::Instant::now() - std::time::Duration::from_secs(10),
    };
    assert!(registry
        .apply_in_window(command.clone(), expired_window, true)
        .is_err());
    assert!(registry
        .apply_in_window(command.clone(), CommandWindow::new(1_000), false)
        .is_err());
    assert_eq!(fs::metadata(&fixture.journal).unwrap().len(), before);
    let result = registry.apply(command.clone(), 1_000).unwrap();
    let before = fs::metadata(&fixture.journal).unwrap().len();
    assert_eq!(
        registry
            .apply_in_window(command, CommandWindow::new(100_000), false)
            .unwrap(),
        result
    );
    assert_eq!(fs::metadata(&fixture.journal).unwrap().len(), before);
}

#[test]
fn registry_rejects_network_device_and_relative_sources_without_opening_them() {
    for path in [
        r"\\server\share\fixture.exe",
        r"\\?\UNC\server\share\fixture.exe",
        r"\\.\C:\fixture.exe",
        r"C:fixture.exe",
        "fixture.exe",
    ] {
        let fixture = Fixture::new();
        let mut source = fixture.source.clone();
        source.program = path.into();
        fixture.write_sources(&[source]);
        assert!(fixture.open().is_err(), "{path}");
        assert!(!fixture.journal.exists());
    }
}

#[test]
fn registry_missing_empty_or_replaced_original_log_never_reinitializes_revoked_identity() {
    for case in 0..4 {
        let fixture = Fixture::new();
        let registry = fixture.open().unwrap();
        let command = fixture.register_command(&registry);
        let operation = registry.apply(command.clone(), 1_000).unwrap();
        registry.apply(revoke(&command, &operation), 2_000).unwrap();
        drop(registry);
        let original = fs::read(&fixture.journal).unwrap();
        match case {
            0 => fs::write(&fixture.journal, []).unwrap(),
            1 => fs::remove_file(&fixture.journal).unwrap(),
            2 => {
                fs::rename(&fixture.journal, fixture.journal.with_extension("original")).unwrap();
                fs::write(&fixture.journal, &original).unwrap();
            }
            _ => fs::remove_file(fixture.journal.with_extension("binding.json")).unwrap(),
        }
        let before = fs::read(&fixture.journal).ok();
        assert!(fixture.open().is_err(), "case {case}");
        assert_eq!(fs::read(&fixture.journal).ok(), before);
    }
}

#[test]
fn registry_pins_log_ancestor_and_binding_until_writer_is_closed() {
    let fixture = Fixture::new();
    let registry = fixture.open().unwrap();
    let parent = fixture.journal.parent().unwrap();
    assert!(fs::rename(parent, parent.with_extension("moved")).is_err());
    assert!(fs::remove_file(fixture.journal.with_extension("binding.json")).is_err());
    drop(registry);
    fs::remove_file(fixture.journal.with_extension("binding.json")).unwrap();
}
