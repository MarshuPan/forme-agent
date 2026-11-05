//! Real encrypted worker side effects under EOF and confirmation substitution.
use super::*;
use forme_gateway::offline_data::{OfflineBootstrapDataWorker, OfflineBootstrapInspection};

fn intent(f: &Fixture) -> p::BootstrapDataIntentV2 {
    let request = request();
    let inspection = OfflineBootstrapInspection::open(&f.manifest_path, &request, 1).unwrap();
    let candidate = inspection
        .catalog()
        .unwrap()
        .candidates
        .iter()
        .find(|c| c.kind == p::BootstrapCandidateKind::ExistingBrain)
        .unwrap()
        .clone();
    p::BootstrapDataIntentV2 {
        schema_version: p::SchemaVersion(2),
        install_ref: f.install.install_id.clone(),
        request: p::BootstrapClaimRequestV2 {
            schema_version: p::SchemaVersion(2),
            session: request.session,
            candidate_ref: candidate.candidate_ref.clone(),
            expected_candidate_digest: candidate.candidate_digest.clone(),
            account_commitment: p::canonical_digest(&"original fixture account").unwrap(),
        },
        candidate,
    }
}

fn receipt(
    intent: &p::BootstrapDataIntentV2,
    data: &p::BootstrapDataPreparedResultV2,
) -> p::BootstrapClaimReceiptV2 {
    p::BootstrapClaimReceiptV2 {
        schema_version: p::SchemaVersion(2),
        install_ref: intent.install_ref.clone(),
        operation_id: intent.request.session.operation_id.clone(),
        candidate_ref: intent.candidate.candidate_ref.clone(),
        candidate_kind: intent.candidate.kind,
        candidate_digest: intent.candidate.candidate_digest.clone(),
        owner_source: intent.candidate.owner_source,
        owner_ref: intent.candidate.owner_ref.clone().unwrap(),
        owner_binding_digest: intent.candidate.owner_binding_digest.clone().unwrap(),
        account_commitment: intent.request.account_commitment.clone(),
        target_binding_digest: data.target_digest().unwrap(),
        migration_outcome: p::BootstrapMigrationOutcome::Recovered,
        expires_at: 999_999,
    }
}

#[test]
fn bootstrap_worker_eof_keeps_original_intent_prepared_data_and_refuses_a_second_copy() {
    let f = Fixture::new();
    let source = source::seed(&f);
    let original = fs::read(source.join("forme.db")).unwrap();
    let intent = intent(&f);
    let args = vec![
        "maintenance-bootstrap-data".into(),
        "--installation-manifest".into(),
        f.manifest_path.to_str().unwrap().into(),
    ];
    let mut frame = serde_json::to_vec(&p::BootstrapDataWorkerRequestV2::Prepare {
        schema_version: p::SchemaVersion(2),
        intent: intent.clone(),
    })
    .unwrap();
    frame.push(b'\n');
    let output = f.worker(&args, &frame);
    assert!(!output.status.success());
    let p::BootstrapDataWorkerResponseV2::Prepared { data, .. } =
        serde_json::from_slice(&output.stdout).unwrap()
    else {
        panic!("missing actual prepared receipt")
    };
    data.validate_for(&intent).unwrap();
    let root = f
        .install
        .paths
        .host_state
        .join("bootstrap-data")
        .join(intent.target_ref().unwrap());
    let before_intent = fs::read(root.join("intent.json")).unwrap();
    let before_prepared = fs::read(root.join("prepared.json")).unwrap();
    let before_target = fs::read(&data.binding.store_path).unwrap();
    assert!(!root.join("confirmation.json").exists());
    let retried = f.worker(&args, &frame);
    assert!(!retried.status.success() && retried.stdout.is_empty());
    assert_eq!(fs::read(root.join("intent.json")).unwrap(), before_intent);
    assert_eq!(
        fs::read(root.join("prepared.json")).unwrap(),
        before_prepared
    );
    assert_eq!(fs::read(&data.binding.store_path).unwrap(), before_target);
    assert_eq!(fs::read(source.join("forme.db")).unwrap(), original);
    assert!(!f.install.paths.config.join("core/core.json").exists());
    assert!(!f
        .install
        .paths
        .host_state
        .join("bootstrap.journal")
        .exists());
}

#[test]
fn bootstrap_worker_confirmation_rechecks_receipt_snapshot_and_exact_target_before_release() {
    let f = Fixture::new();
    source::seed(&f);
    let intent = intent(&f);
    let mut worker = OfflineBootstrapDataWorker::prepare(&f.manifest_path, intent.clone()).unwrap();
    let p::BootstrapDataWorkerResponseV2::Prepared { data, .. } = worker.prepared().unwrap() else {
        panic!("missing prepared result")
    };
    let receipt = receipt(&intent, &data);
    let release = p::BootstrapDataWorkerRequestV2::Release {
        schema_version: p::SchemaVersion(2),
        operation_id: receipt.operation_id.clone(),
    };
    assert!(worker.step(release.clone()).is_err());
    let template: p::ProductConfigTemplate = serde_json::from_slice(
        &fs::read(f.install.paths.config.join("product-template.json")).unwrap(),
    )
    .unwrap();
    let journal_root = f
        .install
        .paths
        .host_state
        .join("bootstrap-data")
        .join(intent.target_ref().unwrap());
    for case in 0..8 {
        let snapshot_ref = format!("{case:032x}");
        let mut state = serde_json::json!({ "schema_version":1, "install_ref": intent.install_ref, "owner": receipt.owner_ref,
            "revision":0, "models":[], "credentials":{}, "active_data":data.binding });
        let mut supplied = receipt.clone();
        match case {
            0 => state["owner"] = "owner:substituted".into(),
            1 => state["revision"] = 1.into(),
            2 => state["models"] = serde_json::json!(["unapproved model"]),
            3 => state["active_data"]["store_path"] = "substituted.db".into(),
            4 => supplied.account_commitment = p::canonical_digest(&"substituted account").unwrap(),
            5 => state["credentials"] = serde_json::json!({"model":"unexpected"}),
            6 => {
                supplied.target_binding_digest = p::canonical_digest(&"substituted target").unwrap()
            }
            _ => {}
        }
        let path = template
            .core_config_path
            .with_extension("revisions")
            .join(&snapshot_ref)
            .join("state.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes = serde_json::to_vec(&state).unwrap();
        fs::write(&path, &bytes).unwrap();
        let request = p::BootstrapDataWorkerRequestV2::Confirm {
            schema_version: p::SchemaVersion(2),
            receipt: supplied,
            snapshot_ref,
            config_binding_digest: p::sha256_content_digest(&bytes),
        };
        if case == 7 {
            let p::BootstrapDataWorkerResponseV2::Confirmed { confirmation, .. } =
                worker.step(request).unwrap()
            else {
                panic!("missing confirmation")
            };
            confirmation.validate_against_receipt(&receipt).unwrap();
            assert_eq!(confirmation.verification_digest, data.verification_digest);
        } else {
            assert!(worker.step(request).is_err(), "substitution case {case}");
            assert!(!journal_root.join("confirmation.json").exists());
        }
    }
    assert!(matches!(
        worker.step(release).unwrap(),
        p::BootstrapDataWorkerResponseV2::Released { .. }
    ));
    assert!(journal_root.join("confirmation.json").is_file());
    assert!(!template.core_config_path.exists());
    assert!(!template.configuration_ack_path.exists());
}
