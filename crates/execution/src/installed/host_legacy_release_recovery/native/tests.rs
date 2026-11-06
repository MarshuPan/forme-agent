//! Real files and original journal, controlled data/SCM observations only.
#![cfg(windows)]
#[path = "tests/fixture.rs"]
mod fixture;

use std::{
    fs,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};

use super::{objects::*, *};
use fixture::{deadline, hash, DataGuard, Fixture};

#[test]
fn native_source_context_survives_later_stages_and_replay_without_granting_eligibility() {
    let f = Fixture::new();
    let id = f.trust.original_operation_id.clone();
    f.journal
        .require_recovery(
            &id,
            f.journal.revision().unwrap(),
            hash("unresolved observation"),
            30_000,
        )
        .unwrap();
    assert_eq!(
        f.journal.legacy_source_context(&id).unwrap().planned_at,
        10_000
    );
    let Fixture {
        journal, repair, ..
    } = f;
    drop(journal);
    let reopened = crate::installed::ProductJournal::open(
        &repair.installation.paths.host_state.join("product.jsonl"),
        &repair.installation.install_id,
    )
    .unwrap();
    assert_eq!(
        reopened.legacy_source_context(&id).unwrap().planned_at,
        10_000
    );
    assert!(reopened
        .check_legacy_review(
            &p::SignedLegacyReleaseReview {
                schema_version: SCHEMA,
                review: p::LegacyReleaseReview {
                    schema_version: SCHEMA,
                    binding: repair.binding.clone(),
                    evidence: vec![],
                    status: p::LegacyReleaseReviewStatus::Unverifiable,
                    issues: vec!["issue:source-incomplete".into()]
                },
                signature: vec![0; 64],
            },
            super::super::timestamp().unwrap()
        )
        .is_err());
}

#[test]
fn native_managed_builder_mounts_provisioned_material_and_leaves_absence_unavailable() {
    for provisioned in [true, false] {
        let f = Fixture::new();
        if !provisioned {
            fs::rename(&f.root, f.root.with_file_name("saved-isolated-materials")).unwrap();
        }
        let Fixture {
            journal, repair, ..
        } = f;
        drop(journal);
        let installation = &repair.installation;
        let host = crate::installed::ProductHostBroker::open(
            &installation.paths.host_state.join("product.jsonl"),
            &installation.install_id,
            "owner-maintenance-isolated-abcdefghijklmnopqrstuvwxyz".into(),
            installation.target,
            installation.release_id.clone(),
        )
        .unwrap();
        let stager = Arc::new(
            crate::installed::ReleaseStager::open(installation.clone(), repair.trust.clone())
                .unwrap(),
        );
        let updater = Arc::new(
            crate::installed::InstalledReleaseUpdater::new(
                installation.clone(),
                installation.paths.config.join("host.json"),
                stager,
                Arc::new(crate::installed::SystemReleaseUpdaterPlatform),
            )
            .unwrap(),
        );
        let host = host
            .with_release_updater(updater)
            .unwrap()
            .with_native_legacy_review(repair.trust.clone())
            .unwrap();
        assert_eq!(host.legacy_release_reviewer.is_some(), provisioned);
    }
}

#[test]
fn native_materials_use_original_planned_time_and_independent_pinned_sources_without_writes() {
    let f = Fixture::new();
    let before = f.journal.legacy_source_bytes_for_test().unwrap();
    let materials = f.materials().unwrap();
    assert_eq!(materials.context.planned_at, 10_000);
    assert_eq!(materials.signed, f.report);
    materials.recheck(deadline()).unwrap();
    let collector = NativeCollector {
        materials: Arc::new(materials),
        release_trust: f.repair.trust.clone(),
    };
    assert_eq!(
        collector.load(&f.report.review.binding.review_id).unwrap(),
        f.report
    );
    assert_eq!(
        collector.load(&f.trust.original_operation_id).unwrap(),
        f.report
    );
    assert!(collector.load("review:foreign").is_err());
    assert_eq!(f.journal.legacy_source_bytes_for_test().unwrap(), before);
    assert!(!f.repair.installation.paths.brain.exists());
    assert!(!f.repair.installation.paths.keys.exists());
}

#[test]
fn native_absent_material_is_unavailable_and_never_provisions_trust() {
    let f = Fixture::new();
    // Move only the test-owned sources; the production loader must not recreate them.
    let saved = f.root.with_file_name("saved-isolated-review");
    fs::rename(&f.root, &saved).unwrap();
    assert!(Materials::open(f.repair.installation.clone(), &f.journal)
        .unwrap()
        .is_none());
    assert!(!f.root.exists());
    assert!(f
        .journal
        .legacy_source_context("operation:foreign")
        .is_err());
}

#[test]
fn native_closed_layout_rejects_missing_extra_oversized_and_linked_material() {
    let f = Fixture::new();
    let proof = f.root.join("proofs").join("build-record");
    let bytes = fs::read(&proof).unwrap();
    fs::remove_file(&proof).unwrap();
    assert!(f.materials().is_err());
    fs::write(&proof, &bytes).unwrap();
    fs::write(f.root.join("extra.json"), b"{}").unwrap();
    assert!(f.materials().is_err());
    fs::remove_file(f.root.join("extra.json")).unwrap();
    fs::write(&proof, vec![b'x'; 4 * 1024 * 1024 + 1]).unwrap();
    assert!(f.materials().is_err());
    fs::write(&proof, &bytes).unwrap();
    fs::hard_link(&proof, f.root.with_file_name("aliased-build-proof")).unwrap();
    assert!(f.materials().is_err());
}

#[test]
fn native_report_and_source_signatures_have_separate_keys_domains_and_raw_pins() {
    let mut f = Fixture::new();
    let mut report = f.report.clone();
    report.signature = SigningKey::from_bytes(&[99; 32])
        .sign(report.review_digest().unwrap().0.as_bytes())
        .to_bytes()
        .to_vec();
    let bytes = serde_json::to_vec(&report).unwrap();
    fs::write(f.root.join("report.json"), &bytes).unwrap();
    f.trust.report_digest = p::sha256_content_digest(&bytes);
    fs::write(
        f.root.join("config.json"),
        serde_json::to_vec(&f.trust).unwrap(),
    )
    .unwrap();
    assert!(f.materials().is_err());
    f.publish();
    let mut signed: SignedSourceReview =
        serde_json::from_slice(&fs::read(f.root.join("sources.json")).unwrap()).unwrap();
    signed.signature = f
        .report_key
        .sign(f.source.digest().unwrap().0.as_bytes())
        .to_bytes()
        .to_vec();
    let bytes = serde_json::to_vec(&signed).unwrap();
    fs::write(f.root.join("sources.json"), &bytes).unwrap();
    f.trust.source_review_digest = p::sha256_content_digest(&bytes);
    fs::write(
        f.root.join("config.json"),
        serde_json::to_vec(&f.trust).unwrap(),
    )
    .unwrap();
    assert!(f.materials().is_err());
    f.publish();
    signed.signature = f
        .source_key
        .sign(p::canonical_digest(&f.source).unwrap().0.as_bytes())
        .to_bytes()
        .to_vec();
    assert!(signed.verify(f.trust.source_public_key).is_err());
    fs::write(
        f.root.join("proofs/code-audit"),
        b"different parseable audit",
    )
    .unwrap();
    assert!(f.materials().is_err());
}

#[test]
fn native_resigning_does_not_override_source_identity_time_images_or_config_bindings() {
    let mut f = Fixture::new();
    let original = serde_json::to_value(&f.source).unwrap();
    let changes = [
        ("schema_version", json!(2)),
        ("install_ref", json!("foreign-install")),
        ("installation_digest", json!(hash("foreign manifest"))),
        ("owner", json!("owner:foreign")),
        ("original_operation_id", json!("upgrade:foreign")),
        ("original_command_digest", json!(hash("foreign command"))),
        ("original_release_ref", json!("foreign-release")),
        ("original_release_digest", json!(hash("foreign release"))),
        ("archive_origin_digest", json!(hash("foreign origin"))),
        ("source_recorded_at", json!(10_000)),
        ("reviewed_at", json!(9_999)),
        ("reviewed_at", json!(i64::MAX)),
        ("images", json!([])),
    ];
    for (field, value) in changes {
        let mut changed = original.clone();
        changed[field] = value;
        f.source = serde_json::from_value(changed).unwrap();
        f.publish();
        assert!(f.materials().is_err(), "accepted {field}");
    }
    f.source = serde_json::from_value(original).unwrap();
    f.source.images.swap(0, 1);
    f.publish();
    assert!(f.materials().is_err());
    f.source.images.swap(0, 1);
    f.source.updater.image_digest = hash("foreign executable");
    f.publish();
    assert!(f.materials().is_err());
}

#[test]
fn native_closed_source_formats_reject_self_supplied_keys_flags_and_missing_fields() {
    let f = Fixture::new();
    let original = serde_json::to_value(&f.source).unwrap();
    for name in ["public_key", "verified", "historical_success", "path"] {
        let mut value = original.clone();
        value[name] = json!(true);
        assert!(serde_json::from_value::<SourceReview>(value).is_err());
    }
    let mut value = original.clone();
    value.as_object_mut().unwrap().remove("worker_state_digest");
    assert!(serde_json::from_value::<SourceReview>(value).is_err());
    let mut value = original;
    value["updater"]["backup_order"] = json!("unknown");
    assert!(serde_json::from_value::<SourceReview>(value).is_err());
    let mut value = serde_json::to_value(&f.trust).unwrap();
    value["report_path"] = json!("C:\\foreign.json");
    assert!(serde_json::from_value::<TrustConfig>(value).is_err());
}

#[test]
fn native_combination_rebuilds_eight_evidences_and_preserves_journal_and_historical_null() {
    let f = Fixture::new();
    let before = f.journal.legacy_source_bytes_for_test().unwrap();
    let mut inspection = f.inspection();
    let observed = inspection.recheck(deadline()).unwrap();
    assert_eq!(observed.evidence.len(), 8);
    assert_eq!(
        observed
            .evidence
            .iter()
            .map(|item| item.kind)
            .collect::<Vec<_>>(),
        p::LegacyReleaseEvidenceKind::ALL
    );
    assert_eq!(
        observed.evidence[0],
        f.journal
            .legacy_journal_evidence(&f.report.review.binding, super::super::timestamp().unwrap())
            .unwrap()
    );
    assert_eq!(observed.evidence[1].artifact_digest, f.trust.archive_digest);
    assert_eq!(observed.worker_digest, f.source.worker_state_digest);
    assert_eq!(observed.updater_capabilities.journal_read_max, 3);
    assert_eq!(observed.updater_capabilities.updater_version, 3);
    inspection.ensure_held(deadline()).unwrap();
    assert_eq!(
        inspection.recheck(deadline()).unwrap().evidence,
        observed.evidence
    );
    Box::new(inspection).release().unwrap();
    let archive = f
        .repair
        .installation
        .paths
        .host_state
        .join("legacy-data-archives")
        .join(&f.trust.archive_digest.0[7..])
        .join("archive.json");
    let value: Value = serde_json::from_slice(&fs::read(archive).unwrap()).unwrap();
    assert_eq!(value["database_schema_version"], Value::Null);
    assert_eq!(f.journal.legacy_source_bytes_for_test().unwrap(), before);
}

#[test]
fn native_inspection_rejects_worker_data_or_custody_loss_without_reacquiring() {
    let f = Fixture::new();
    let mut inspection = f.inspection();
    inspection.worker_digest = hash("other worker");
    assert!(inspection.recheck(deadline()).is_err());
    inspection.worker_digest = f.source.worker_state_digest.clone();
    inspection.identity.database_digest = hash("drifted database");
    assert!(inspection.recheck(deadline()).is_err());
    inspection.identity = f.identity.clone();
    let held = Arc::new(AtomicBool::new(true));
    let releases = Arc::new(AtomicUsize::new(0));
    inspection.data = Box::new(DataGuard {
        identity: f.identity.clone(),
        held: held.clone(),
        releases: releases.clone(),
    });
    inspection.ensure_held(deadline()).unwrap();
    held.store(false, Ordering::SeqCst);
    assert!(inspection.ensure_held(deadline()).is_err());
    assert!(inspection.ensure_held(Instant::now()).is_err());
    Box::new(inspection).release().unwrap();
    assert_eq!(releases.load(Ordering::SeqCst), 1);
}

#[test]
fn native_historical_sources_cannot_substitute_repair_source_or_current_native_checks() {
    let mut f = Fixture::new();
    let mut images = f.source.image_bindings();
    images[0].2 = hash("foreign old host");
    let repair = ManagementReaderRepairEvidence::load(
        &f.repair.installation,
        &f.repair.trust,
        &f.report.review.binding,
        super::super::timestamp().unwrap(),
    )
    .unwrap();
    assert!(repair
        .check_legacy_sources(
            &images,
            &f.source.config_files_digest,
            &f.source.service_configuration_digest
        )
        .is_err());
    assert!(repair
        .check_legacy_sources(
            &f.source.image_bindings(),
            &hash("foreign config"),
            &f.source.service_configuration_digest
        )
        .is_err());
    drop(repair);
    let collector = NativeCollector {
        materials: Arc::new(f.materials().unwrap()),
        release_trust: f.repair.trust.clone(),
    };
    assert!(collector
        .inspect(&f.report.review, &hash("foreign worker"), deadline())
        .is_err());
    // Temp executable bytes and controlled SCM cannot pass the production
    // native reader. It must not start services or create the brain database.
    assert!(collector
        .inspect(&f.report.review, &f.source.worker_state_digest, deadline())
        .is_err());
    assert!(!f.repair.installation.paths.brain.exists());
    drop(collector);
    f.source.worker_state_digest = hash("different historical worker");
    f.publish();
    let collector = NativeCollector {
        materials: Arc::new(f.materials().unwrap()),
        release_trust: f.repair.trust.clone(),
    };
    assert!(collector
        .inspect(
            &f.report.review,
            &hash("independently archived worker watermark"),
            deadline()
        )
        .is_err());
}

#[test]
fn native_material_custody_blocks_changes_or_rejects_them_and_expired_recheck_fails() {
    let f = Fixture::new();
    let materials = f.materials().unwrap();
    assert!(materials.recheck(Instant::now()).is_err());
    let result = fs::write(f.root.join("sources.json"), b"{}");
    if result.is_ok() {
        assert!(materials.recheck(deadline()).is_err());
    } else {
        materials.recheck(deadline()).unwrap();
    }
    let alias = f.root.with_file_name("review-proof-hardlink");
    if fs::hard_link(f.root.join("proofs/build-record"), &alias).is_ok() {
        assert!(materials.recheck(deadline()).is_err());
    }
}
