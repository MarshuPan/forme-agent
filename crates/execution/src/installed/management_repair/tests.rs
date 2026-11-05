//! Original isolated fixtures, independent signatures and actual file custody.
use std::{collections::BTreeMap, fs, path::Path, time::Duration};

use ed25519_dalek::{Signer, SigningKey};
use serde::Serialize;

use super::{objects::*, *};

fn digest(value: &str) -> p::SchemaDigest {
    p::canonical_digest(&value).unwrap()
}

fn native_fixture(_: &p::InstallationManifest) -> p::Result<NativeObservation> {
    Ok(NativeObservation {
        services: digest("original isolated service configuration"),
        capabilities: p::ProductReleaseRecoveryCapabilities {
            schema_version: SCHEMA,
            journal_read_max: 3,
            updater_version: 3,
        },
    })
}

fn old_updater(installation: &p::InstallationManifest) -> p::Result<NativeObservation> {
    let mut observed = native_fixture(installation)?;
    observed.capabilities.journal_read_max = 2;
    observed.capabilities.updater_version = 2;
    Ok(observed)
}

fn wrong_services(installation: &p::InstallationManifest) -> p::Result<NativeObservation> {
    let mut observed = native_fixture(installation)?;
    observed.services = digest("changed isolated service registration");
    Ok(observed)
}

fn drifting_services(installation: &p::InstallationManifest) -> p::Result<NativeObservation> {
    if installation
        .paths
        .host_state
        .join("service-drift.fixture")
        .exists()
    {
        wrong_services(installation)
    } else {
        native_fixture(installation)
    }
}

fn write_json(path: &Path, value: &impl Serialize) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn receipt(plan: &RepairPlan) -> RepairReceipt {
    RepairReceipt {
        schema_version: SCHEMA,
        repair_id: plan.repair_id.clone(),
        plan_digest: plan.digest().unwrap(),
        distribution_digest: plan.distribution_digest.clone(),
        install_ref: plan.install_ref.clone(),
        installation_digest: plan.installation_digest.clone(),
        owner: plan.owner.clone(),
        original_operation_id: plan.original_operation_id.clone(),
        original_command_digest: plan.original_command_digest.clone(),
        original_release_ref: plan.original_release_ref.clone(),
        original_release_digest: plan.original_release_digest.clone(),
        journal_seq: plan.journal_seq,
        journal_head: plan.journal_head.clone(),
        config_files_digest: plan.config_files_digest.clone(),
        service_configuration_digest: plan.service_configuration_digest.clone(),
        installed_images_digest: plan.images_digest().unwrap(),
        journal_read_max: plan.journal_read_max,
        updater_version: plan.updater_version,
        committed_at: 2_000,
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    installation: p::InstallationManifest,
    binding: p::LegacyReleaseReviewBinding,
    trust: ReleaseStagingConfig,
    key: SigningKey,
    plan: RepairPlan,
    root: PathBuf,
    repair: PathBuf,
    distribution: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path();
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
            install_id: "repair-fixture-installation".into(),
            target,
            release_id: "fixture-original".into(),
            release_digest: digest("original immutable release"),
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
        let binding = p::LegacyReleaseReviewBinding {
            schema_version: SCHEMA,
            review_id: "repair-fixture-review".into(),
            install_ref: installation.install_id.clone(),
            owner: p::OwnerPrincipalRef("owner:repair-fixture".into()),
            original_operation_id: "repair-original-upgrade".into(),
            original_command_digest: digest("original uncompleted command"),
            original_release_ref: installation.release_id.clone(),
            original_release_digest: installation.release_digest.clone(),
            config_revision: 1,
            config_digest: digest("configuration binding"),
            journal_seq: 28,
            journal_head: digest("unaltered original journal checkpoint"),
            expires_at: 100_000,
        };
        let repair_id = "repair:isolated-management";
        let repair_root = installation.paths.host_state.join("management-repair");
        let directory = p::sha256_content_digest(repair_id.as_bytes());
        let repair = repair_root.join(&directory.0[7..]);
        let distribution = repair.join("distribution");
        let content = distribution.join("content");
        let mut files = Vec::new();
        let mut images = Vec::new();
        for image in RepairImage::ALL {
            let before = format!("original isolated executable {}", image.binary());
            let after = format!("new isolated distribution executable {}", image.binary());
            let path = content.join("bin").join(image.binary());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &after).unwrap();
            files.push(p::ReleaseFile {
                schema_version: SCHEMA,
                relative_path: format!("bin/{}", image.binary()),
                size: after.len() as u64,
                digest: p::sha256_content_digest(after.as_bytes()),
            });
            let installed = if image.may_change() { &after } else { &before };
            let path = installation.paths.releases.join("bin").join(image.binary());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, installed).unwrap();
            images.push(RepairImageBinding {
                schema_version: SCHEMA,
                image,
                source_size: before.len() as u64,
                source_digest: p::sha256_content_digest(before.as_bytes()),
                target_size: installed.len() as u64,
                target_digest: p::sha256_content_digest(installed.as_bytes()),
            });
        }
        fs::create_dir(content.join("web")).unwrap();
        let ui = b"<main>isolated repair distribution</main>";
        fs::write(content.join("web/index.html"), ui).unwrap();
        let ui_file = p::ReleaseFile {
            schema_version: SCHEMA,
            relative_path: "web/index.html".into(),
            size: ui.len() as u64,
            digest: p::sha256_content_digest(ui),
        };
        files.push(ui_file.clone());
        let range = p::SchemaReadWriteRange {
            schema_version: SCHEMA,
            read_min: 1,
            read_max: 1,
            write_version: 1,
        };
        let manifest = p::ReleaseManifest {
            schema_version: SCHEMA,
            release_id: "fixture-new-distribution".into(),
            target,
            files,
            ui_digest: p::canonical_digest(&vec![ui_file]).unwrap(),
            managed_dependencies: vec![],
            config_range: range.clone(),
            data_range: range,
            minimum_upgrader: 3,
            signature_ref: "isolated-repair-signing-key".into(),
        };
        let key = SigningKey::from_bytes(&[0x57; 32]);
        write_json(&distribution.join("manifest.json"), &manifest);
        fs::write(
            distribution.join("signature.ed25519"),
            key.sign(manifest.digest().unwrap().0.as_bytes()).to_bytes(),
        )
        .unwrap();
        let mut configurations = Vec::new();
        for name in CONFIG_FILES {
            let path = installation.paths.config.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if name == "installation.json" {
                write_json(&path, &installation);
            } else {
                fs::write(&path, format!("isolated configuration {name}")).unwrap();
            }
            configurations.push((name, file_digest(&path).unwrap()));
        }
        let plan = RepairPlan {
            schema_version: SCHEMA,
            repair_id: repair_id.into(),
            distribution_digest: manifest.digest().unwrap(),
            install_ref: installation.install_id.clone(),
            installation_digest: installation.digest().unwrap(),
            owner: binding.owner.clone(),
            original_operation_id: binding.original_operation_id.clone(),
            original_command_digest: binding.original_command_digest.clone(),
            original_release_ref: binding.original_release_ref.clone(),
            original_release_digest: binding.original_release_digest.clone(),
            journal_seq: binding.journal_seq,
            journal_head: binding.journal_head.clone(),
            config_files_digest: p::canonical_digest(&(
                "forme-release-recovery-configs-v1",
                configurations,
            ))
            .unwrap(),
            service_configuration_digest: digest("original isolated service configuration"),
            images,
            journal_read_max: 3,
            updater_version: 3,
            authorized_at: 1_000,
            authorization_deadline: 3_000,
        };
        let trust = ReleaseStagingConfig {
            schema_version: SCHEMA,
            install_ref: installation.install_id.clone(),
            pinned_keys: BTreeMap::from([(manifest.signature_ref, key.verifying_key().to_bytes())]),
        };
        let fixture = Self {
            _root: root,
            installation,
            binding,
            trust,
            key,
            plan,
            root: repair_root,
            repair,
            distribution,
        };
        fixture.publish(&fixture.plan);
        fixture
    }

    fn publish(&self, plan: &RepairPlan) {
        self.publish_receipt(plan, receipt(plan));
    }

    fn publish_receipt(&self, plan: &RepairPlan, receipt: RepairReceipt) {
        write_json(
            &self.repair.join("plan.json"),
            &SignedPlan {
                schema_version: SCHEMA,
                signature: self
                    .key
                    .sign(plan.digest().unwrap().0.as_bytes())
                    .to_bytes()
                    .to_vec(),
                plan: plan.clone(),
            },
        );
        write_json(
            &self.root.join("receipt.json"),
            &SignedReceipt {
                schema_version: SCHEMA,
                signature: self
                    .key
                    .sign(receipt.digest().unwrap().0.as_bytes())
                    .to_bytes()
                    .to_vec(),
                receipt,
            },
        );
    }

    fn load(&self) -> p::Result<ManagementReaderRepairEvidence> {
        ManagementReaderRepairEvidence::load(&self.installation, &self.trust, &self.binding, 5_000)
    }

    fn inspect(&self) -> p::Result<ManagementReaderRepairInspection> {
        self.load()?
            .inspect_with(Instant::now() + Duration::from_secs(30), native_fixture)
    }
}

#[test]
fn signed_repair_binds_full_distribution_and_preserves_business_images_and_state() {
    let fixture = Fixture::new();
    let original_installation =
        fs::read(fixture.installation.paths.config.join("installation.json")).unwrap();
    let evidence = fixture.load().unwrap();
    assert_eq!(
        evidence.receipt_digest(),
        &receipt(&fixture.plan).digest().unwrap()
    );
    let inspection = evidence
        .inspect_with(Instant::now() + Duration::from_secs(30), native_fixture)
        .unwrap();
    assert!(inspection
        .recheck(Instant::now() + Duration::from_secs(30))
        .is_ok());
    assert_eq!(
        fs::read(fixture.installation.paths.config.join("installation.json")).unwrap(),
        original_installation
    );
    assert!(!fixture
        .installation
        .paths
        .host_state
        .join("host.journal")
        .exists());
    assert!(!fixture.installation.paths.brain.exists());
    assert!(!fixture.installation.paths.keys.exists());
}

#[test]
fn durable_receipt_survives_authorization_expiry_but_requires_commit_inside_window() {
    let fixture = Fixture::new();
    assert!(fixture.load().is_ok());
    for committed_at in [999, 3_001, 5_001, -1] {
        let mut receipt = receipt(&fixture.plan);
        receipt.committed_at = committed_at;
        fixture.publish_receipt(&fixture.plan, receipt);
        assert!(fixture.load().is_err());
    }
}

#[test]
fn correctly_resigned_foreign_installation_owner_operation_release_and_checkpoint_are_rejected() {
    let fixture = Fixture::new();
    let changes: [fn(&mut RepairPlan); 8] = [
        |plan| plan.install_ref = "other-installation".into(),
        |plan| plan.installation_digest = digest("other installation"),
        |plan| plan.owner = p::OwnerPrincipalRef("owner:other".into()),
        |plan| plan.original_operation_id = "other-operation".into(),
        |plan| plan.original_command_digest = digest("other command"),
        |plan| plan.original_release_digest = digest("other release"),
        |plan| plan.journal_seq += 1,
        |plan| plan.journal_head = digest("other journal head"),
    ];
    for change in changes {
        let mut plan = fixture.plan.clone();
        change(&mut plan);
        fixture.publish(&plan);
        assert!(fixture.load().is_err());
    }
}

#[test]
fn only_three_management_images_may_change_even_with_valid_independent_signatures() {
    let fixture = Fixture::new();
    for image in [RepairImage::Brain, RepairImage::Cli, RepairImage::Executord] {
        let mut plan = fixture.plan.clone();
        let entry = plan
            .images
            .iter_mut()
            .find(|entry| entry.image == image)
            .unwrap();
        entry.target_digest = digest("unauthorized business replacement");
        assert!(plan.validate().is_err());
    }
    for mutation in [0, 1, 2, 3] {
        let mut plan = fixture.plan.clone();
        match mutation {
            0 => plan.images.swap(0, 1),
            1 => {
                plan.images.pop();
            }
            2 => plan.images.push(plan.images[0].clone()),
            _ => plan.images[0].image = RepairImage::Brain,
        }
        assert!(plan.validate().is_err());
    }
}

#[test]
fn every_receipt_binding_is_checked_even_when_resigned_by_the_trusted_authority() {
    let fixture = Fixture::new();
    let changes: [fn(&mut RepairReceipt); 15] = [
        |value| value.repair_id = "other-repair".into(),
        |value| value.plan_digest = digest("other plan"),
        |value| value.distribution_digest = digest("other distribution"),
        |value| value.install_ref = "other-installation".into(),
        |value| value.installation_digest = digest("other installation"),
        |value| value.owner = p::OwnerPrincipalRef("owner:other".into()),
        |value| value.original_operation_id = "other-operation".into(),
        |value| value.original_command_digest = digest("other command"),
        |value| value.original_release_ref = "other-release".into(),
        |value| value.original_release_digest = digest("other release"),
        |value| value.journal_seq += 1,
        |value| value.journal_head = digest("other journal"),
        |value| value.config_files_digest = digest("other configuration"),
        |value| value.service_configuration_digest = digest("other services"),
        |value| value.installed_images_digest = digest("other images"),
    ];
    for change in changes {
        let mut value = receipt(&fixture.plan);
        change(&mut value);
        fixture.publish_receipt(&fixture.plan, value);
        assert!(fixture.load().is_err());
    }
}

#[test]
fn independent_signature_domains_trust_roots_and_package_binding_cannot_be_substituted() {
    let fixture = Fixture::new();
    let wrong = ReleaseStagingConfig {
        pinned_keys: BTreeMap::from([(
            "isolated-repair-signing-key".into(),
            SigningKey::from_bytes(&[0x58; 32])
                .verifying_key()
                .to_bytes(),
        )]),
        ..fixture.trust.clone()
    };
    assert!(ManagementReaderRepairEvidence::load(
        &fixture.installation,
        &wrong,
        &fixture.binding,
        5_000
    )
    .is_err());
    let mut plan: SignedPlan =
        serde_json::from_slice(&fs::read(fixture.repair.join("plan.json")).unwrap()).unwrap();
    plan.signature = fixture
        .key
        .sign(fixture.plan.distribution_digest.0.as_bytes())
        .to_bytes()
        .to_vec();
    write_json(&fixture.repair.join("plan.json"), &plan);
    assert!(fixture.load().is_err());
    fixture.publish(&fixture.plan);
    let mut other = fixture.plan.clone();
    other.distribution_digest = digest("different but signed package");
    fixture.publish(&other);
    assert!(fixture.load().is_err());
}

#[test]
fn valid_signatures_cannot_claim_a_different_management_target() {
    let fixture = Fixture::new();
    for image in [RepairImage::Host, RepairImage::Web, RepairImage::Hostctl] {
        let mut plan = fixture.plan.clone();
        plan.images
            .iter_mut()
            .find(|entry| entry.image == image)
            .unwrap()
            .target_digest = digest("wrong signed target");
        fixture.publish(&plan);
        assert!(fixture.load().is_err());
    }
}

#[test]
fn closed_layout_closed_json_and_fixed_hashed_reference_reject_extra_inputs() {
    let fixture = Fixture::new();
    for path in [
        fixture.root.join("extra.json"),
        fixture.repair.join("other-plan.json"),
        fixture.distribution.join("public-key.json"),
        fixture.distribution.join("content/extra.txt"),
    ] {
        fs::write(&path, b"untrusted extra").unwrap();
        assert!(fixture.load().is_err());
        fs::remove_file(path).unwrap();
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.root.join("receipt.json")).unwrap()).unwrap();
    value["public_key"] = serde_json::json!([1, 2, 3]);
    write_json(&fixture.root.join("receipt.json"), &value);
    assert!(fixture.load().is_err());
    fixture.publish(&fixture.plan);
    value = serde_json::from_slice(&fs::read(fixture.repair.join("plan.json")).unwrap()).unwrap();
    value["plan"]["repair_id"] = serde_json::json!("../outside");
    write_json(&fixture.repair.join("plan.json"), &value);
    assert!(fixture.load().is_err());
}

#[test]
fn actual_images_configuration_services_and_exact_capabilities_are_required() {
    let fixture = Fixture::new();
    assert!(fixture
        .load()
        .unwrap()
        .inspect_with(Instant::now() + Duration::from_secs(30), old_updater)
        .is_err());
    assert!(fixture
        .load()
        .unwrap()
        .inspect_with(Instant::now() + Duration::from_secs(30), wrong_services)
        .is_err());
    let config = fixture.installation.paths.config.join("host.json");
    let original = fs::read(&config).unwrap();
    fs::write(&config, b"configuration changed after repair").unwrap();
    assert!(fixture.inspect().is_err());
    fs::write(&config, original).unwrap();
    for image in RepairImage::ALL {
        let path = fixture
            .installation
            .paths
            .releases
            .join("bin")
            .join(image.binary());
        let bytes = fs::read(&path).unwrap();
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        fs::write(&path, changed).unwrap();
        assert!(fixture.inspect().is_err());
        fs::write(&path, bytes).unwrap();
    }
}

#[test]
fn expired_inspection_and_unproved_real_platform_never_become_verified() {
    let fixture = Fixture::new();
    assert!(fixture
        .load()
        .unwrap()
        .inspect_with(Instant::now(), native_fixture)
        .is_err());
    assert!(fixture
        .load()
        .unwrap()
        .inspect_current(Instant::now() + Duration::from_secs(10))
        .is_err());
}

#[test]
fn inspection_rechecks_actual_services_and_closed_artifact_layout_before_commit() {
    let fixture = Fixture::new();
    let inspection = fixture
        .load()
        .unwrap()
        .inspect_with(Instant::now() + Duration::from_secs(30), drifting_services)
        .unwrap();
    let drift = fixture
        .installation
        .paths
        .host_state
        .join("service-drift.fixture");
    fs::write(&drift, b"isolated registration drift").unwrap();
    assert!(inspection
        .recheck(Instant::now() + Duration::from_secs(30))
        .is_err());
    fs::remove_file(drift).unwrap();
    fs::write(fixture.root.join("late-extra.fixture"), b"late artifact").unwrap();
    assert!(inspection
        .recheck(Instant::now() + Duration::from_secs(30))
        .is_err());
}

#[test]
fn linked_distribution_or_installed_image_is_rejected_under_real_file_custody() {
    let fixture = Fixture::new();
    let content = fixture.distribution.join("content/bin/forme-hostd.exe");
    let alias = fixture._root.path().join("distribution-alias.exe");
    fs::hard_link(&content, &alias).unwrap();
    assert!(fixture.load().is_err());
    fs::remove_file(alias).unwrap();
    let image = fixture
        .installation
        .paths
        .releases
        .join("bin/forme-hostd.exe");
    fs::hard_link(&image, fixture._root.path().join("image-alias.exe")).unwrap();
    assert!(fixture.inspect().is_err());
}

#[cfg(windows)]
#[test]
fn windows_inspection_denies_writing_or_replacing_original_files_until_guard_release() {
    let fixture = Fixture::new();
    let inspection = fixture.inspect().unwrap();
    for path in [
        fixture
            .installation
            .paths
            .releases
            .join("bin/forme-hostd.exe"),
        fixture.installation.paths.config.join("host.json"),
        fixture.repair.join("plan.json"),
        fixture.root.join("receipt.json"),
    ] {
        assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
        assert!(fs::rename(&path, path.with_extension("moved")).is_err());
    }
    drop(inspection);
    assert!(fs::OpenOptions::new()
        .write(true)
        .open(fixture.root.join("receipt.json"))
        .is_ok());
}
