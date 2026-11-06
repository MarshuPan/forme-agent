//! Original isolated sources, keys and repair files; no machine installation.
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;

use super::*;
use crate::installed::{management_repair::tests::Fixture as RepairFixture, ProductJournal};

pub(super) fn hash(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}
pub(super) fn deadline() -> Instant {
    Instant::now() + std::time::Duration::from_secs(30)
}

pub(super) struct Fixture {
    pub journal: ProductJournal,
    pub repair: RepairFixture,
    pub root: PathBuf,
    pub trust: TrustConfig,
    pub source: SourceReview,
    pub report: p::SignedLegacyReleaseReview,
    pub identity: p::ProductReleaseDataIdentity,
    pub report_key: SigningKey,
    pub source_key: SigningKey,
}

impl Fixture {
    pub fn new() -> Self {
        let mut repair = RepairFixture::new();
        let installation = &repair.installation;
        let journal = ProductJournal::open(
            &installation.paths.host_state.join("product.jsonl"),
            &installation.install_id,
        )
        .unwrap();
        journal
            .record_installation_verification_with_active_release(
                installation.digest().unwrap(),
                installation.release_digest.clone(),
                Some((&installation.release_id, &installation.release_digest)),
            )
            .unwrap();
        let mut binding = repair.binding.clone();
        let operation = journal
            .plan(
                p::MaintenanceCommand {
                    schema_version: SCHEMA,
                    operation_id: binding.original_operation_id.clone(),
                    idempotency_key: "key:native-original".into(),
                    expected_revision: journal.revision().unwrap(),
                    actor_proof: p::MaintenanceProofRef {
                        schema_version: SCHEMA,
                        proof_ref: "proof:native".into(),
                    },
                    kind: p::MaintenanceKind::Upgrade {
                        release_id: "fixture-target".into(),
                        release_digest: hash("target"),
                        backup_ref: "backup:native".into(),
                    },
                    requested_deadline: 20_000,
                },
                p::MaintenanceActorClass::OwnerReauthenticated,
                p::MaintenanceReferences {
                    schema_version: SCHEMA,
                    original_release_ref: installation.release_id.clone(),
                    target_release_ref: "fixture-target".into(),
                    data_candidate_ref: None,
                    backup_ref: Some("backup:native".into()),
                },
                10_000,
            )
            .unwrap();
        let id = &binding.original_operation_id;
        journal
            .begin_stage(
                id,
                journal.revision().unwrap(),
                p::MaintenanceStage::Validated,
                p::MaintenanceActorClass::OwnerReauthenticated,
                10_001,
            )
            .unwrap();
        journal
            .finish_stage(
                id,
                journal.revision().unwrap(),
                p::MaintenanceStage::Validated,
                vec![hash("validated")],
                10_002,
            )
            .unwrap();
        let takeover = p::ProductReleaseTakeover {
            schema_version: SCHEMA,
            release: p::ProductReleaseRequest {
                schema_version: SCHEMA,
                install_ref: installation.install_id.clone(),
                operation_id: id.clone(),
                command_digest: operation.command_digest.clone(),
                change: p::ProductReleaseChange::Upgrade,
                original_release_ref: installation.release_id.clone(),
                original_release_digest: installation.release_digest.clone(),
                target_release_ref: "fixture-target".into(),
                target_release_digest: hash("target"),
                backup_ref: "backup:native".into(),
            },
            installation_digest: installation.digest().unwrap(),
            owner: binding.owner.clone(),
            expected_config_revision: binding.config_revision,
            expected_config_digest: binding.config_digest.clone(),
            deadline: 20_000,
            readiness_nonce: "b".repeat(64),
        };
        journal
            .prepare_release_takeover(journal.revision().unwrap(), takeover, 10_003)
            .unwrap();
        journal
            .begin_stage(
                id,
                journal.revision().unwrap(),
                p::MaintenanceStage::Quiescing,
                p::MaintenanceActorClass::OwnerReauthenticated,
                10_004,
            )
            .unwrap();
        let checkpoint = journal.checkpoint().unwrap();
        binding.original_command_digest = operation.command_digest;
        binding.journal_seq = checkpoint.0;
        binding.journal_head = checkpoint.1;
        binding.expires_at = super::super::super::timestamp().unwrap() + 90_000;
        repair.rebind_legacy(binding.clone());
        let identity = p::ProductReleaseDataIdentity {
            schema_version: SCHEMA,
            owner_digest: p::sha256_content_digest(binding.owner.0.as_bytes()),
            store_identity: hash("original store"),
            schema_digest: hash("original schema"),
            database_digest: hash("original database"),
            database_schema_version: 1,
            config_schema_version: 1,
            event_count: 13,
            last_stream_seq: 13,
        };
        let archive = archive(&repair, &journal, &identity);
        let archive_digest = p::sha256_content_digest(&archive);
        let origin_key = SigningKey::from_bytes(&[39; 32]);
        let takeover = journal
            .release_takeover(&binding.original_operation_id)
            .unwrap()
            .unwrap();
        let origin = json!({
            "schema_version":1,"source_ref":"origin:isolated-native",
            "install_ref":repair.installation.install_id,"installation_digest":repair.installation.digest().unwrap(),
            "owner":binding.owner,"original_operation_id":binding.original_operation_id,"original_command_digest":binding.original_command_digest,
            "original_release_ref":binding.original_release_ref,"original_release_digest":binding.original_release_digest,
            "target_release_ref":takeover.release.target_release_ref,"target_release_digest":takeover.release.target_release_digest,
            "config_revision":binding.config_revision,"config_digest":binding.config_digest,"archive_digest":archive_digest,"archive_revision":1,"recorded_at":100,
        });
        // Origin uses its declared Rust field order, not serde_json map order.
        let origin_digest =
            crate::installed::legacy_data_archive::origin_digest_for_test(origin.clone()).unwrap();
        let signature = origin_key
            .sign(origin_digest.0.as_bytes())
            .to_bytes()
            .to_vec();
        let origin =
            serde_json::to_vec(&json!({"schema_version":1,"origin":origin,"signature":signature}))
                .unwrap();
        let archive_root = repair
            .installation
            .paths
            .host_state
            .join("legacy-data-archives")
            .join(&archive_digest.0[7..]);
        fs::create_dir_all(&archive_root).unwrap();
        fs::write(archive_root.join("archive.json"), archive).unwrap();
        fs::write(archive_root.join("origin.json"), &origin).unwrap();
        let report_key = SigningKey::from_bytes(&[41; 32]);
        let source_key = SigningKey::from_bytes(&[42; 32]);
        let trust = TrustConfig {
            schema_version: SCHEMA,
            install_ref: repair.installation.install_id.clone(),
            original_operation_id: binding.original_operation_id.clone(),
            report_digest: hash("pending publication"),
            review_public_key: report_key.verifying_key().to_bytes(),
            source_review_digest: hash("pending publication"),
            source_public_key: source_key.verifying_key().to_bytes(),
            archive_digest,
            archive_origin_digest: p::sha256_content_digest(&origin),
            archive_origin_public_key: origin_key.verifying_key().to_bytes(),
        };
        let (configs, services) = repair.source_digests();
        let images: Vec<_> = repair
            .source_images()
            .into_iter()
            .map(|(binary, size, digest)| SourceImage {
                binary,
                size,
                digest,
            })
            .collect();
        let source = SourceReview {
            schema_version: SCHEMA,
            source_ref: "source:isolated-native".into(),
            install_ref: repair.installation.install_id.clone(),
            installation_digest: repair.installation.digest().unwrap(),
            owner: binding.owner.clone(),
            original_operation_id: binding.original_operation_id.clone(),
            original_command_digest: binding.original_command_digest.clone(),
            original_release_ref: binding.original_release_ref.clone(),
            original_release_digest: binding.original_release_digest.clone(),
            archive_origin_digest: trust.archive_origin_digest.clone(),
            source_recorded_at: 100,
            reviewed_at: 30_000,
            updater: UpdaterAudit {
                image_digest: images[0].digest.clone(),
                source_commit: "a".repeat(40),
                source_tree_digest: hash("source tree"),
                cargo_lock_digest: hash("lock"),
                toolchain_digest: hash("toolchain"),
                build_record_digest: hash("proof build-record"),
                code_audit_digest: hash("proof code-audit"),
                backup_order: AuditedOrder::DurableIntentBeforeSideEffect,
                switching_order: AuditedOrder::DurableIntentBeforeSideEffect,
            },
            images,
            config_files_digest: configs,
            service_configuration_digest: services,
            worker_state_digest: hash("independently archived worker watermark"),
            configuration_history_digest: hash("proof configuration-history"),
            worker_history_digest: hash("proof worker-history"),
            service_path_repair_digest: hash("proof service-path-repair"),
        };
        let evidence = p::LegacyReleaseEvidenceKind::ALL
            .into_iter()
            .map(|kind| {
                if kind == p::LegacyReleaseEvidenceKind::OriginalJournal {
                    return journal
                        .legacy_journal_evidence(
                            &binding,
                            super::super::super::timestamp().unwrap(),
                        )
                        .unwrap();
                }
                p::LegacyReleaseEvidence {
                    schema_version: SCHEMA,
                    kind,
                    artifact_digest: hash("pending observation"),
                    binding_digest: binding.digest().unwrap(),
                    observation_digest: hash("pending observation"),
                }
            })
            .collect();
        let report = p::SignedLegacyReleaseReview {
            schema_version: SCHEMA,
            review: p::LegacyReleaseReview {
                schema_version: SCHEMA,
                binding,
                evidence,
                status: p::LegacyReleaseReviewStatus::Verified,
                issues: vec![],
            },
            signature: vec![0; 64],
        };
        let root = repair
            .installation
            .paths
            .host_state
            .join("legacy-release-review");
        fs::create_dir_all(root.join("proofs")).unwrap();
        for (name, _) in source.proofs() {
            fs::write(root.join("proofs").join(name), format!("proof {name}")).unwrap();
        }
        let mut f = Self {
            journal,
            repair,
            root,
            trust,
            source,
            report,
            identity,
            report_key,
            source_key,
        };
        f.publish();
        f
    }

    pub fn publish(&mut self) {
        self.report.signature = self
            .report_key
            .sign(self.report.review_digest().unwrap().0.as_bytes())
            .to_bytes()
            .to_vec();
        let bytes = serde_json::to_vec(&self.report).unwrap();
        fs::write(self.root.join("report.json"), &bytes).unwrap();
        self.trust.report_digest = p::sha256_content_digest(&bytes);
        let signed = SignedSourceReview {
            schema_version: SCHEMA,
            signature: self
                .source_key
                .sign(self.source.digest().unwrap().0.as_bytes())
                .to_bytes()
                .to_vec(),
            review: self.source.clone(),
        };
        let bytes = serde_json::to_vec(&signed).unwrap();
        fs::write(self.root.join("sources.json"), &bytes).unwrap();
        self.trust.source_review_digest = p::sha256_content_digest(&bytes);
        fs::write(
            self.root.join("config.json"),
            serde_json::to_vec(&self.trust).unwrap(),
        )
        .unwrap();
    }

    pub fn materials(&self) -> p::Result<Materials> {
        Materials::open(self.repair.installation.clone(), &self.journal)?.ok_or_else(invalid)
    }

    pub fn inspection(&self) -> NativeInspection {
        let materials = Arc::new(self.materials().unwrap());
        let origin = LegacyDataArchiveTrust::from_pinned_origin(
            self.trust.archive_digest.clone(),
            self.trust.archive_origin_digest.clone(),
            self.trust.archive_origin_public_key,
        )
        .unwrap();
        let archive = LegacyDataArchiveEvidence::load(
            &self.repair.installation,
            &origin,
            &self.report.review.binding,
            &materials.context.takeover.release,
            materials.context.planned_at,
            deadline(),
        )
        .unwrap();
        NativeInspection {
            materials,
            archive,
            repair: self.repair.inspect().unwrap(),
            data: Box::new(DataGuard {
                identity: self.identity.clone(),
                held: Arc::new(AtomicBool::new(true)),
                releases: Arc::new(AtomicUsize::new(0)),
            }),
            identity: self.identity.clone(),
            worker_digest: self.source.worker_state_digest.clone(),
        }
    }
}

fn archive(
    repair: &RepairFixture,
    journal: &ProductJournal,
    identity: &p::ProductReleaseDataIdentity,
) -> Vec<u8> {
    let takeover = journal
        .release_takeover(&repair.binding.original_operation_id)
        .unwrap()
        .unwrap();
    serde_json::to_vec(&json!({
        "schema_version":1,"operation":"archive:isolated-native","outcome":"inbox_prepared_only","prepare_inbox":true,
        "release_id":takeover.release.target_release_ref,"release_digest":takeover.release.target_release_digest,
        "changes":["created-private-inbox-parent","created-private-preparation-directory","published-exact-signed-inbox"],
        "failure_stage":null,"provider_called":false,"provider_keys_read":false,"service_actions":0,"maintenance_commands_submitted":0,"program_images_changed":false,
        "database_schema_version":null,"runtime_compatibility_verified":false,
        "offline_identity":{"schema_version":1,"owner_digest":identity.owner_digest,"store_identity":identity.store_identity,"schema_digest":identity.schema_digest,
            "database_digest":identity.database_digest,"event_count":identity.event_count,"last_stream_seq":identity.last_stream_seq,"authenticated_content_rows":8},
        "product_revision":1,"config_schema_version":1,"actual_database_schema_required_during_upgrade":true,"copied_file_count":7,
        "services":(["forme-host","forme-web","forme-brain","forme-release-updater"].map(|name|json!({"Name":name,"State":"Stopped","StartName":if name=="forme-web"||name=="forme-brain"{format!("NT SERVICE\\{name}")}else{"LocalSystem".into()},"StartMode":"Manual","ProcessId":0,"PathName":format!("C:\\isolated-fixture\\{name}.exe")}))),
    })).unwrap()
}

pub(super) struct DataGuard {
    pub identity: p::ProductReleaseDataIdentity,
    pub held: Arc<AtomicBool>,
    pub releases: Arc<AtomicUsize>,
}
impl ReleaseRecoveryDataInspection for DataGuard {
    fn identity(&self) -> &p::ProductReleaseDataIdentity {
        &self.identity
    }
    fn ensure_held(&mut self, end: Instant) -> p::Result<()> {
        if self.held.load(Ordering::SeqCst) {
            ensure_time(end)
        } else {
            Err(invalid())
        }
    }
    fn release(&mut self) -> p::Result<()> {
        self.held.store(false, Ordering::SeqCst);
        self.releases.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
