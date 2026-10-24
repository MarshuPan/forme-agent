//! Native switching algorithm evidence with controlled platform observations.
use super::*;
use forme_execution::installed::{ReleaseServiceImageChange, ReleaseServiceImageManager};
use std::time::{Duration, Instant};

fn prepare(f: &Fixture) -> (ProductJournal, p::ProductReleaseRequest) {
    let journal = f.stage();
    let request = f.upgrade_request("native-service-switch", journal.revision().unwrap());
    let command = request.command.clone();
    let operation = journal
        .plan(
            command.clone(),
            p::MaintenanceActorClass::OwnerReauthenticated,
            request.references,
            now_millis(),
        )
        .unwrap();
    for stage in [
        p::MaintenanceStage::Validated,
        p::MaintenanceStage::Quiescing,
        p::MaintenanceStage::BackedUp,
    ] {
        journal
            .begin_stage(
                &command.operation_id,
                journal.revision().unwrap(),
                stage,
                p::MaintenanceActorClass::OwnerReauthenticated,
                now_millis(),
            )
            .unwrap();
        journal
            .finish_stage(
                &command.operation_id,
                journal.revision().unwrap(),
                stage,
                vec![p::canonical_digest(&stage).unwrap()],
                now_millis(),
            )
            .unwrap();
    }
    journal
        .begin_stage(
            &command.operation_id,
            journal.revision().unwrap(),
            p::MaintenanceStage::Switching,
            p::MaintenanceActorClass::OwnerReauthenticated,
            now_millis(),
        )
        .unwrap();
    for id in [
        p::BuiltinServiceId::Host,
        p::BuiltinServiceId::Web,
        p::BuiltinServiceId::Brain,
    ] {
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let program = f
            .installation
            .paths
            .releases
            .join("bin")
            .join(format!("{}{suffix}", id.binary_name()));
        fs::create_dir_all(program.parent().unwrap()).unwrap();
        fs::write(program, format!("old-{}", id.binary_name())).unwrap();
        let config = f.installation.paths.config.join(match id {
            p::BuiltinServiceId::Host => "host.json",
            p::BuiltinServiceId::Web => "web.json",
            p::BuiltinServiceId::Brain => "core/core.json",
        });
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(config, b"immutable config fixture").unwrap();
    }
    (
        journal,
        p::ProductReleaseRequest {
            schema_version: p::SchemaVersion(1),
            install_ref: f.installation.install_id.clone(),
            operation_id: command.operation_id,
            command_digest: operation.command_digest,
            change: p::ProductReleaseChange::Upgrade,
            original_release_ref: f.installation.release_id.clone(),
            original_release_digest: f.installation.release_digest.clone(),
            target_release_ref: RELEASE.into(),
            target_release_digest: f.release.digest().unwrap(),
            backup_ref: "backup:release-fixture".into(),
        },
    )
}

#[derive(Default)]
struct ObservedPlatform {
    verified: Mutex<Vec<p::BuiltinServiceId>>,
    changed: Mutex<Vec<p::BuiltinServiceId>>,
    fail_inspection: Option<p::BuiltinServiceId>,
    fail_after_change: Option<p::BuiltinServiceId>,
}
impl ReleaseServiceImageManager for ObservedPlatform {
    fn verify_original(&self, change: &ReleaseServiceImageChange) -> p::Result<p::SchemaDigest> {
        self.verified.lock().unwrap().push(change.service());
        if self.fail_inspection == Some(change.service()) {
            return Err(p::Error(
                "fixture service is running or misconfigured".into(),
            ));
        }
        p::canonical_digest(&(change.service(), "actual original configuration"))
    }
    fn switch_and_verify(
        &self,
        change: &ReleaseServiceImageChange,
        original: &p::SchemaDigest,
    ) -> p::Result<p::SchemaDigest> {
        assert_eq!(
            self.verified.lock().unwrap().len(),
            3,
            "all registrations must be inspected before the first effect"
        );
        assert_eq!(
            *original,
            p::canonical_digest(&(change.service(), "actual original configuration")).unwrap()
        );
        assert!(change.source_image().is_absolute());
        assert!(change.target_image().is_absolute());
        assert!(change.configuration_path().is_absolute());
        self.changed.lock().unwrap().push(change.service());
        if self.fail_after_change == Some(change.service()) {
            return Err(p::Error(
                "fixture lost native receipt after changing image".into(),
            ));
        }
        p::canonical_digest(&(change.service(), "actual target configuration"))
    }
}

#[test]
fn service_image_switch_preflights_all_three_and_durably_consumes_the_original_intent() {
    let f = Fixture::new();
    let (journal, request) = prepare(&f);
    let before = journal.revision().unwrap();
    let plan = f
        .stager()
        .prepare_service_switch(&journal, request.clone())
        .unwrap();
    let platform = ObservedPlatform::default();
    let receipt = plan
        .execute(
            &journal,
            &platform,
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    receipt.validate_for(&request).unwrap();
    assert_eq!(receipt.images.len(), 3);
    assert_eq!(
        *platform.changed.lock().unwrap(),
        [
            p::BuiltinServiceId::Web,
            p::BuiltinServiceId::Brain,
            p::BuiltinServiceId::Host
        ]
    );
    assert_eq!(journal.revision().unwrap(), before + 1);
    assert_eq!(
        journal
            .operation(&request.operation_id)
            .unwrap()
            .pending_stage,
        Some(p::MaintenanceStage::Switching)
    );
    assert_eq!(
        journal.active_release().unwrap(),
        Some((
            f.installation.release_id.clone(),
            f.installation.release_digest.clone()
        ))
    );
    assert!(f
        .stager()
        .prepare_service_switch(&journal, request.clone())
        .is_err());
    drop(journal);
    let reopened = ProductJournal::open(&f.journal(), &f.installation.install_id).unwrap();
    assert!(f
        .stager()
        .prepare_service_switch(&reopened, request)
        .is_err());
    assert_eq!(reopened.revision().unwrap(), before + 1);
}

#[test]
fn failed_service_preflight_has_zero_effects_and_zero_claims() {
    let f = Fixture::new();
    let (journal, request) = prepare(&f);
    let before = journal.revision().unwrap();
    let platform = ObservedPlatform {
        fail_inspection: Some(p::BuiltinServiceId::Host),
        ..Default::default()
    };
    let plan = f
        .stager()
        .prepare_service_switch(&journal, request.clone())
        .unwrap();
    assert!(plan
        .execute(
            &journal,
            &platform,
            Instant::now() + Duration::from_secs(10)
        )
        .is_err());
    assert!(platform.changed.lock().unwrap().is_empty());
    assert_eq!(journal.revision().unwrap(), before);
    assert!(f.stager().prepare_service_switch(&journal, request).is_ok());
}

#[test]
fn lost_native_switch_receipt_survives_restart_without_repeating_partial_effects() {
    let f = Fixture::new();
    let (journal, request) = prepare(&f);
    let before = journal.revision().unwrap();
    let platform = ObservedPlatform {
        fail_after_change: Some(p::BuiltinServiceId::Brain),
        ..Default::default()
    };
    let plan = f
        .stager()
        .prepare_service_switch(&journal, request.clone())
        .unwrap();
    assert!(plan
        .execute(
            &journal,
            &platform,
            Instant::now() + Duration::from_secs(10)
        )
        .is_err());
    assert_eq!(
        *platform.changed.lock().unwrap(),
        [p::BuiltinServiceId::Web, p::BuiltinServiceId::Brain]
    );
    assert_eq!(journal.revision().unwrap(), before + 1);
    drop(journal);
    let reopened = ProductJournal::open(&f.journal(), &f.installation.install_id).unwrap();
    assert!(f
        .stager()
        .prepare_service_switch(&reopened, request)
        .is_err());
    assert_eq!(
        reopened
            .operation("native-service-switch")
            .unwrap()
            .pending_stage,
        Some(p::MaintenanceStage::Switching)
    );
}

#[test]
fn stale_expired_or_changed_configuration_plans_never_reach_native_effects() {
    for fault in ["revision", "deadline", "config", "source", "target"] {
        let f = Fixture::new();
        let (journal, request) = prepare(&f);
        let plan = f
            .stager()
            .prepare_service_switch(&journal, request.clone())
            .unwrap();
        match fault {
            "revision" => journal
                .require_recovery(
                    &request.operation_id,
                    journal.revision().unwrap(),
                    p::canonical_digest(&"fixture interrupted operation").unwrap(),
                    now_millis(),
                )
                .unwrap(),
            "config" => {
                fs::write(f.installation.paths.config.join("web.json"), b"changed").unwrap()
            }
            "source" => fs::write(
                f.installation
                    .paths
                    .releases
                    .join("bin")
                    .join(if cfg!(windows) {
                        "forme-webd.exe"
                    } else {
                        "forme-webd"
                    }),
                b"changed",
            )
            .unwrap(),
            "target" => fs::write(
                f.destination().join(&f.release.files[0].relative_path),
                b"changed",
            )
            .unwrap(),
            _ => {}
        }
        let before = journal.revision().unwrap();
        let platform = ObservedPlatform::default();
        let deadline = if fault == "deadline" {
            Instant::now()
        } else {
            Instant::now() + Duration::from_secs(10)
        };
        assert!(
            plan.execute(&journal, &platform, deadline).is_err(),
            "fault={fault}"
        );
        assert!(platform.changed.lock().unwrap().is_empty());
        assert!(platform.verified.lock().unwrap().is_empty());
        assert_eq!(journal.revision().unwrap(), before);
    }
}
