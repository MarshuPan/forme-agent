//! Network projection and LAN maintenance evidence without touching a real firewall.
use super::{
    tests::{fixture, patch},
    *,
};
use crate::installed::{HostCredentials, NativeServiceManager, ProductHostBroker};
use ed25519_dalek::Signer;
use std::sync::Arc;

const WEB: &str = "network-fixture-web-abcdefghijklmnopqrstuvwxyz";
const ADMIN: &str = "network-fixture-admin-abcdefghijklmnopqrstuvwxyz";
const AGENT: &str = "network-fixture-agent-abcdefghijklmnopqrstuvwxyz";
const INSTALL: &str = "install:config-test";
const NOW: i64 = 1_000_000;

#[derive(Clone, Default)]
struct Services {
    service_calls: Arc<Mutex<Vec<(p::BuiltinServiceId, p::NativeServiceAction)>>>,
    firewall_calls: Arc<Mutex<Vec<(p::ProductNetworkConfig, p::ProductNetworkConfig)>>>,
    reject_firewall: bool,
    bad_firewall_receipt: bool,
    omit_removed_rule: bool,
    unknown_start: bool,
    unsupported_firewall: bool,
    rejected_preflight: bool,
    unreachable_non_loopback: bool,
    untrusted_health: bool,
    wrong_health_nonce: bool,
    wrong_health_installation: bool,
    wrong_health_listener: bool,
    wrong_health_signature: bool,
}

impl NativeServiceManager for Services {
    fn supports_web_health(&self) -> bool {
        true
    }

    fn probe_web_listener(
        &self,
        network: &p::ProductNetworkConfig,
        expected_listener_digest: &p::SchemaDigest,
        nonce: &str,
    ) -> p::Result<p::ProductNetworkHealthObservation> {
        if self.unreachable_non_loopback && !network.bind.ip().is_loopback() {
            return Ok(p::ProductNetworkHealthObservation::Unreachable);
        }
        if self.untrusted_health {
            return Ok(p::ProductNetworkHealthObservation::Untrusted);
        }
        let mut proof = p::ProductNetworkHealthProof {
            schema_version: p::PRODUCT_NETWORK_HEALTH_SCHEMA_VERSION,
            installation_digest: p::product_network_installation_digest(INSTALL)?,
            listener_digest: expected_listener_digest.clone(),
            nonce: nonce.to_owned(),
            signature: Vec::new(),
        };
        if self.wrong_health_nonce {
            proof.nonce = "00".repeat(32);
        }
        if self.wrong_health_installation {
            proof.installation_digest =
                p::product_network_installation_digest("install:other-test")?;
        }
        if self.wrong_health_listener {
            proof.listener_digest = p::sha256_content_digest(b"wrong-listener");
        }
        proof.signature = tests::web_assertion_signing_key()
            .sign(proof.signing_digest()?.0.as_bytes())
            .to_bytes()
            .to_vec();
        if self.wrong_health_signature {
            proof.signature[0] ^= 1;
        }
        Ok(p::ProductNetworkHealthObservation::Verified(proof))
    }

    fn supports_firewall_reconciliation(&self) -> bool {
        !self.unsupported_firewall
    }
    fn validate_firewall_reconciliation(
        &self,
        _previous: &p::ProductNetworkConfig,
        _desired: &p::ProductNetworkConfig,
    ) -> p::Result<()> {
        if self.rejected_preflight {
            Err(p::Error(
                "fixture requires administrator firewall configuration".into(),
            ))
        } else {
            Ok(())
        }
    }
    fn inspect(&self, service: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
        Ok(p::NativeServiceStatus {
            schema_version: p::SchemaVersion(1),
            service,
            state: p::NativeServiceState::Stopped,
        })
    }

    fn apply(&self, change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
        self.service_calls
            .lock()
            .unwrap()
            .push((change.service, change.action));
        Ok(p::NativeReceipt {
            schema_version: p::SchemaVersion(1),
            service: change.service,
            requested: change.action,
            observed: match change.action {
                p::NativeServiceAction::Start if self.unknown_start => {
                    p::NativeServiceState::Unknown
                }
                p::NativeServiceAction::Start => p::NativeServiceState::Running,
                p::NativeServiceAction::Stop => p::NativeServiceState::Stopped,
            },
        })
    }

    fn reconcile_firewall(
        &self,
        previous: &p::ProductNetworkConfig,
        network: &p::ProductNetworkConfig,
    ) -> p::Result<p::NativeFirewallReceipt> {
        if self.reject_firewall {
            return Err(p::Error(
                "fixture firewall adapter rejected the rule".into(),
            ));
        }
        self.firewall_calls
            .lock()
            .unwrap()
            .push((previous.clone(), network.clone()));
        let removed_rule_refs = previous
            .firewall
            .as_ref()
            .filter(|old| network.firewall.as_ref() != Some(old))
            .map(|old| vec![old.rule_ref.clone()])
            .unwrap_or_default();
        Ok(p::NativeFirewallReceipt {
            schema_version: p::SchemaVersion(1),
            bind: if self.bad_firewall_receipt {
                "192.0.2.1:1".parse().unwrap()
            } else {
                network.bind
            },
            rule_ref: if self.bad_firewall_receipt {
                Some("firewall:wrong".into())
            } else {
                network
                    .firewall
                    .as_ref()
                    .map(|firewall| firewall.rule_ref.clone())
            },
            network_digest: if self.bad_firewall_receipt {
                p::sha256_content_digest(b"wrong-firewall-digest")
            } else {
                network.validation_digest()?
            },
            enabled: !network.bind.ip().is_loopback(),
            removed_rule_refs: if self.omit_removed_rule {
                Vec::new()
            } else {
                removed_rule_refs
            },
        })
    }
}

fn broker(
    root: &Path,
    config: Arc<Mutex<ProductConfigStore>>,
    services: Services,
) -> ProductHostBroker<Services> {
    ProductHostBroker::open_with_service_manager_and_config(
        &root.join("network-maintenance.jsonl"),
        INSTALL,
        HostCredentials::new(ADMIN.into(), Some(AGENT.into()))
            .unwrap()
            .with_web_configuration(Some(WEB.into()))
            .unwrap(),
        p::ProductTarget::WindowsX86_64,
        "0.1.0".into(),
        Some(config),
        services,
    )
    .unwrap()
}

fn lan_config(store: &ProductConfigStore) -> p::ProductNetworkConfig {
    let root = store.template.web_material_directory.clone().unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let identity = rcgen::generate_simple_self_signed(vec!["192.168.1.20".into()]).unwrap();
    std::fs::write(root.join("server.pem"), identity.cert.pem()).unwrap();
    std::fs::write(
        root.join("server.key"),
        identity.signing_key.serialize_pem(),
    )
    .unwrap();
    p::ProductNetworkConfig {
        schema_version: p::PRODUCT_NETWORK_SCHEMA_VERSION,
        bind: "192.168.1.20:8771".parse().unwrap(),
        allowed_hosts: vec!["192.168.1.20:8771".into()],
        tls: Some(p::ProductNetworkTls {
            schema_version: p::PRODUCT_NETWORK_SCHEMA_VERSION,
            certificate_path: root.join("server.pem"),
            private_key_path: root.join("server.key"),
        }),
        firewall: Some(p::ProductFirewallBinding {
            schema_version: p::PRODUCT_NETWORK_SCHEMA_VERSION,
            rule_ref: "firewall:forme-web-lan".into(),
        }),
    }
}

fn stage_lan(store: &Mutex<ProductConfigStore>) -> p::ProductNetworkConfig {
    let mut store = store.lock().unwrap();
    let network = lan_config(&store);
    let mut request = patch("save:network", 0);
    request.network = Some(network.clone());
    store.apply(request).unwrap();
    network
}

fn maintenance_request(
    host: &ProductHostBroker<Services>,
    auth: &crate::installed::HostAuth,
    config_revision: u64,
    validation_digest: p::SchemaDigest,
) -> p::ProductMaintenanceRequest {
    p::ProductMaintenanceRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: "network:activate".into(),
        expected_revision: host.maintenance_snapshot(auth).unwrap().product_revision,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        action: p::ProductMaintenanceAction::ConfigureLan {
            config_revision,
            validation_digest,
        },
        requested_deadline: NOW + 60_000,
    }
}

#[test]
fn saving_network_settings_keeps_the_old_web_projection_active() {
    let (_directory, _path, store) = fixture();
    let old = {
        let mut store = store.lock().unwrap();
        store.apply(patch("save:base", 0)).unwrap();
        let path = store.template.web_network_config_path.clone().unwrap();
        let old: p::ProductNetworkConfig = storage::json(&path, MAX_CONFIG_BYTES).unwrap();
        let network = lan_config(&store);
        let mut request = patch("save:network", 1);
        request.network = Some(network.clone());
        let response = store.apply(request).unwrap();
        assert_eq!(response.view.network, network);
        assert!(!response.view.network_active);
        assert!(response.view.pending_restart);
        old
    };
    let store = store.into_inner().unwrap();
    let path = store.template.web_network_config_path.clone().unwrap();
    let active: p::ProductNetworkConfig = storage::json(&path, MAX_CONFIG_BYTES).unwrap();
    assert_eq!(active, old);
}

#[test]
fn configure_lan_requires_a_bound_firewall_and_restarts_only_web() {
    let (directory, _path, store) = fixture();
    let network = stage_lan(&store);
    let config = Arc::new(store);
    let services = Services::default();
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let digest = config
        .lock()
        .unwrap()
        .network_validation_digest(1, &p::OwnerPrincipalRef("owner:test".into()))
        .unwrap();
    let result = host
        .owner_maintenance(&auth, maintenance_request(&host, &auth, 1, digest), NOW)
        .unwrap();
    assert_eq!(result.stage, p::MaintenanceStage::Committed);
    assert_eq!(
        services.firewall_calls.lock().unwrap().as_slice(),
        &[(
            p::ProductNetworkConfig::loopback("127.0.0.1:8771".parse().unwrap()),
            network,
        )]
    );
    assert_eq!(
        services.service_calls.lock().unwrap().as_slice(),
        &[
            (p::BuiltinServiceId::Web, p::NativeServiceAction::Stop),
            (p::BuiltinServiceId::Web, p::NativeServiceAction::Start),
        ]
    );
    assert!(config.lock().unwrap().snapshot().unwrap().network_active);
}

#[test]
fn ordinary_target_unreachable_rolls_back_to_verified_loopback() {
    let (directory, _path, store) = fixture();
    stage_lan(&store);
    let config = Arc::new(store);
    let services = Services {
        unreachable_non_loopback: true,
        ..Services::default()
    };
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let digest = config
        .lock()
        .unwrap()
        .network_validation_digest(1, &p::OwnerPrincipalRef("owner:test".into()))
        .unwrap();
    let result = host
        .owner_maintenance(&auth, maintenance_request(&host, &auth, 1, digest), NOW)
        .unwrap();
    let network_result = result.network_result.as_ref().unwrap();
    assert_eq!(network_result.outcome, p::ProductNetworkOutcome::RolledBack);
    assert_eq!(
        network_result.active_bind,
        "127.0.0.1:8771".parse().unwrap()
    );
    assert_eq!(
        config.lock().unwrap().active_network().unwrap().bind,
        "127.0.0.1:8771".parse().unwrap()
    );
    assert!(!config.lock().unwrap().snapshot().unwrap().network_active);
    assert_eq!(services.firewall_calls.lock().unwrap().len(), 1);
    assert_eq!(services.service_calls.lock().unwrap().len(), 4);
}

#[test]
fn untrusted_target_health_requires_recovery_without_fallback() {
    let (directory, _path, store) = fixture();
    stage_lan(&store);
    let config = Arc::new(store);
    let services = Services {
        untrusted_health: true,
        ..Services::default()
    };
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let digest = config
        .lock()
        .unwrap()
        .network_validation_digest(1, &p::OwnerPrincipalRef("owner:test".into()))
        .unwrap();
    let result = host
        .owner_maintenance(&auth, maintenance_request(&host, &auth, 1, digest), NOW)
        .unwrap();
    assert_eq!(result.stage, p::MaintenanceStage::RecoveryRequired);
    assert!(result.network_result.is_none());
    assert_eq!(services.firewall_calls.lock().unwrap().len(), 1);
    assert_eq!(services.service_calls.lock().unwrap().len(), 2);
}

#[test]
fn malformed_target_health_proofs_require_recovery_without_fallback() {
    for (label, services) in [
        (
            "nonce",
            Services {
                wrong_health_nonce: true,
                ..Services::default()
            },
        ),
        (
            "installation",
            Services {
                wrong_health_installation: true,
                ..Services::default()
            },
        ),
        (
            "listener",
            Services {
                wrong_health_listener: true,
                ..Services::default()
            },
        ),
        (
            "signature",
            Services {
                wrong_health_signature: true,
                ..Services::default()
            },
        ),
    ] {
        let (directory, _path, store) = fixture();
        stage_lan(&store);
        let config = Arc::new(store);
        let host = broker(directory.path(), config.clone(), services.clone());
        let auth = host.authenticate_web_configuration(WEB).unwrap();
        let digest = config
            .lock()
            .unwrap()
            .network_validation_digest(1, &p::OwnerPrincipalRef("owner:test".into()))
            .unwrap();
        let result = host
            .owner_maintenance(&auth, maintenance_request(&host, &auth, 1, digest), NOW)
            .unwrap();
        assert_eq!(
            result.stage,
            p::MaintenanceStage::RecoveryRequired,
            "{label}"
        );
        assert!(result.network_result.is_none(), "{label}");
        assert_eq!(services.service_calls.lock().unwrap().len(), 2, "{label}");
    }
}

#[test]
fn missing_or_unbound_firewall_never_starts_web() {
    let (directory, _path, store) = fixture();
    stage_lan(&store);
    let config = Arc::new(store);
    let services = Services {
        reject_firewall: true,
        ..Services::default()
    };
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let digest = config
        .lock()
        .unwrap()
        .network_validation_digest(1, &p::OwnerPrincipalRef("owner:test".into()))
        .unwrap();
    let result = host
        .owner_maintenance(&auth, maintenance_request(&host, &auth, 1, digest), NOW)
        .unwrap();
    assert_eq!(result.stage, p::MaintenanceStage::RecoveryRequired);
    assert!(services.service_calls.lock().unwrap().is_empty());
    assert!(!config.lock().unwrap().snapshot().unwrap().network_active);

    let (directory, _path, store) = fixture();
    stage_lan(&store);
    let config = Arc::new(store);
    let services = Services {
        bad_firewall_receipt: true,
        ..Services::default()
    };
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let digest = config
        .lock()
        .unwrap()
        .network_validation_digest(1, &p::OwnerPrincipalRef("owner:test".into()))
        .unwrap();
    let result = host
        .owner_maintenance(&auth, maintenance_request(&host, &auth, 1, digest), NOW)
        .unwrap();
    assert_eq!(result.stage, p::MaintenanceStage::RecoveryRequired);
    assert!(services.service_calls.lock().unwrap().is_empty());
    assert!(!config.lock().unwrap().snapshot().unwrap().network_active);
}

#[test]
fn stale_network_digest_is_rejected_before_native_io() {
    let (directory, _path, store) = fixture();
    stage_lan(&store);
    let config = Arc::new(store);
    let services = Services::default();
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let request = maintenance_request(
        &host,
        &auth,
        2,
        p::sha256_content_digest(b"stale-network-validation"),
    );
    assert!(host.owner_maintenance(&auth, request, NOW).is_err());
    assert!(services.firewall_calls.lock().unwrap().is_empty());
    assert!(services.service_calls.lock().unwrap().is_empty());
}

#[test]
fn interrupted_network_publication_recovers_the_exact_projection() {
    let (_directory, path, store) = fixture();
    store.lock().unwrap().apply(patch("save:base", 0)).unwrap();
    let mut store = store.into_inner().unwrap();
    let network = lan_config(&store);
    let mut state = store.state.clone();
    state.schema_version = p::SchemaVersion(6);
    state.network = Some(network.clone());
    state.validate(&store.template).unwrap();
    let reference = materialize::stage_with_network(&store.template, &state, &network).unwrap();
    store
        .journal
        .prepare(Intent {
            operation: "network:interrupted".into(),
            digest: p::canonical_digest(&"network-interrupted").unwrap(),
            snapshot: reference,
        })
        .unwrap();
    drop(store);

    let store = ProductConfigStore::open(&path).unwrap();
    let mut store = store.into_inner().unwrap();
    let view = store.snapshot().unwrap();
    assert_eq!(view.network, network);
    assert!(view.network_active);
    assert!(store.journal.pending().is_none());
}

#[test]
fn tampered_published_network_projection_blocks_reopen() {
    let (_directory, path, store) = fixture();
    store.lock().unwrap().apply(patch("save:base", 0)).unwrap();
    let network_path = store
        .lock()
        .unwrap()
        .template
        .web_network_config_path
        .clone()
        .unwrap();
    drop(store);
    std::fs::write(network_path, b"{}").unwrap();
    assert!(ProductConfigStore::open(&path).is_err());
}

#[test]
fn core_ack_does_not_acknowledge_a_staged_network() {
    let (_directory, _path, store) = fixture();
    stage_lan(&store);
    let mut store = store.into_inner().unwrap();
    store
        .acknowledge(p::ProductConfigAcknowledgement {
            schema_version: p::SchemaVersion(1),
            install_ref: "install:config-test".into(),
            revision: 1,
            data_identity: None,
        })
        .unwrap();
    let owner = p::OwnerPrincipalRef("owner:test".into());
    let report = store.validate_revision(1, &owner).unwrap();
    assert!(report.restart_required);
    assert!(!store.snapshot().unwrap().network_active);
    store
        .activate_network(
            "network:publish".into(),
            1,
            &owner,
            &report.network_validation_digest,
        )
        .unwrap();
    assert!(!store.validate_revision(1, &owner).unwrap().restart_required);
    assert!(store.snapshot().unwrap().network_active);
}

#[test]
fn tampering_while_open_blocks_validation_activation_and_model_save() {
    let (_directory, _path, store) = fixture();
    stage_lan(&store);
    let mut store = store.into_inner().unwrap();
    let owner = p::OwnerPrincipalRef("owner:test".into());
    let digest = store.network_validation_digest(1, &owner).unwrap();
    let path = store.template.web_network_config_path.clone().unwrap();
    std::fs::write(&path, b"{}").unwrap();
    assert!(store.validate_revision(1, &owner).is_err());
    assert!(store.network_validation_digest(1, &owner).is_err());
    assert!(store
        .activate_network("network:tampered".into(), 1, &owner, &digest)
        .is_err());
    assert!(store.apply(patch("save:after-tampering", 1)).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"{}");
    assert_eq!(store.state.revision, 1);
    assert!(store.journal.pending().is_none());
}

#[test]
fn certificate_replacement_invalidates_authorization_before_native_io() {
    let (directory, _path, store) = fixture();
    stage_lan(&store);
    let owner = p::OwnerPrincipalRef("owner:test".into());
    let old_digest = store
        .lock()
        .unwrap()
        .network_validation_digest(1, &owner)
        .unwrap();
    let config = Arc::new(store);
    let services = Services::default();
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    // The same protected paths now hold a different valid certificate/key pair.
    lan_config(&config.lock().unwrap());
    let new_digest = config
        .lock()
        .unwrap()
        .network_validation_digest(1, &owner)
        .unwrap();
    assert_ne!(old_digest, new_digest);
    let request = maintenance_request(&host, &auth, 1, old_digest);
    assert!(host.owner_maintenance(&auth, request, NOW).is_err());
    assert!(services.firewall_calls.lock().unwrap().is_empty());
    assert!(services.service_calls.lock().unwrap().is_empty());
}

#[test]
fn invalid_missing_and_mismatched_tls_never_pass_static_validation() {
    let (_directory, _path, store) = fixture();
    let network = stage_lan(&store);
    let mut store = store.into_inner().unwrap();
    let owner = p::OwnerPrincipalRef("owner:test".into());
    let tls = network.tls.unwrap();
    let original = std::fs::read(&tls.certificate_path).unwrap();
    std::fs::write(&tls.certificate_path, b"not a certificate").unwrap();
    assert!(store.validate_revision(1, &owner).is_err());
    std::fs::write(&tls.certificate_path, original).unwrap();
    let wrong = rcgen::generate_simple_self_signed(vec!["foreign.invalid".into()]).unwrap();
    std::fs::write(&tls.private_key_path, wrong.signing_key.serialize_pem()).unwrap();
    assert!(store.validate_revision(1, &owner).is_err());
    std::fs::remove_file(&tls.private_key_path).unwrap();
    assert!(store.network_validation_digest(1, &owner).is_err());
    assert!(!store.snapshot().unwrap().network_active);
}

fn apply_staged(
    host: &ProductHostBroker<Services>,
    config: &Mutex<ProductConfigStore>,
    operation: &str,
    revision: u64,
) -> p::MaintenanceOperation {
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let request = staged_request(host, config, operation, revision);
    host.owner_maintenance(&auth, request, NOW).unwrap()
}

fn staged_request(
    host: &ProductHostBroker<Services>,
    config: &Mutex<ProductConfigStore>,
    operation: &str,
    revision: u64,
) -> p::ProductMaintenanceRequest {
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let digest = config
        .lock()
        .unwrap()
        .network_validation_digest(revision, &p::OwnerPrincipalRef("owner:test".into()))
        .unwrap();
    let mut request = maintenance_request(host, &auth, revision, digest);
    request.operation_id = operation.into();
    request
}

#[test]
fn lan_replacement_and_return_to_loopback_reconcile_the_previous_rules() {
    let (directory, _path, store) = fixture();
    let original = stage_lan(&store);
    let config = Arc::new(store);
    let services = Services::default();
    let host = broker(directory.path(), config.clone(), services.clone());
    assert_eq!(
        apply_staged(&host, &config, "network:lan", 1).stage,
        p::MaintenanceStage::Committed
    );
    let mut replacement = original.clone();
    replacement.firewall.as_mut().unwrap().rule_ref = "firewall:forme-web-replacement".into();
    let mut update = patch("save:replacement", 1);
    update.network = Some(replacement.clone());
    config.lock().unwrap().apply(update).unwrap();
    assert_eq!(
        apply_staged(&host, &config, "network:replacement", 2).stage,
        p::MaintenanceStage::Committed
    );
    let loopback = p::ProductNetworkConfig::loopback("127.0.0.1:8771".parse().unwrap());
    let mut update = patch("save:loopback", 2);
    update.network = Some(loopback.clone());
    config.lock().unwrap().apply(update).unwrap();
    assert_eq!(
        apply_staged(&host, &config, "network:loopback", 3).stage,
        p::MaintenanceStage::Committed
    );
    let calls = services.firewall_calls.lock().unwrap();
    assert_eq!(calls[1], (original, replacement.clone()));
    assert_eq!(calls[2], (replacement, loopback.clone()));
    assert_eq!(calls.len(), 3);
    assert_eq!(config.lock().unwrap().active_network().unwrap(), loopback);
    assert_eq!(services.service_calls.lock().unwrap().len(), 6);
}

#[test]
fn unconfirmed_rule_removal_keeps_old_projection_and_never_restarts_or_retries() {
    let (directory, _path, store) = fixture();
    let original = stage_lan(&store);
    let config = Arc::new(store);
    let services = Services {
        omit_removed_rule: true,
        ..Services::default()
    };
    let host = broker(directory.path(), config.clone(), services.clone());
    assert_eq!(
        apply_staged(&host, &config, "network:initial", 1).stage,
        p::MaintenanceStage::Committed
    );
    let mut update = patch("save:loopback", 1);
    update.network = Some(p::ProductNetworkConfig::loopback(
        "127.0.0.1:8771".parse().unwrap(),
    ));
    config.lock().unwrap().apply(update).unwrap();
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let request = staged_request(&host, &config, "network:removal", 2);
    assert_eq!(
        host.owner_maintenance(&auth, request.clone(), NOW)
            .unwrap()
            .stage,
        p::MaintenanceStage::RecoveryRequired
    );
    assert_eq!(config.lock().unwrap().active_network().unwrap(), original);
    assert_eq!(services.service_calls.lock().unwrap().len(), 2);
    assert_eq!(
        host.owner_maintenance(&auth, request, NOW).unwrap().stage,
        p::MaintenanceStage::RecoveryRequired
    );
    assert_eq!(services.firewall_calls.lock().unwrap().len(), 2);
    assert_eq!(services.service_calls.lock().unwrap().len(), 2);
}

#[test]
fn web_start_unknown_is_not_a_success_receipt_and_is_not_replayed() {
    let (directory, _path, store) = fixture();
    stage_lan(&store);
    let config = Arc::new(store);
    let services = Services {
        unknown_start: true,
        ..Services::default()
    };
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let request = staged_request(&host, &config, "network:unknown", 1);
    let result = host.owner_maintenance(&auth, request.clone(), NOW).unwrap();
    assert_eq!(result.stage, p::MaintenanceStage::RecoveryRequired);
    assert!(result.pending_stage.is_some());
    assert!(config.lock().unwrap().snapshot().unwrap().network_active);
    assert_eq!(
        host.owner_maintenance(&auth, request, NOW).unwrap().stage,
        p::MaintenanceStage::RecoveryRequired
    );
    assert_eq!(services.firewall_calls.lock().unwrap().len(), 1);
    assert_eq!(services.service_calls.lock().unwrap().len(), 2);
}

#[test]
fn restore_and_legacy_model_save_preserve_the_staged_network_without_activating_it() {
    let (_directory, path, store) = fixture();
    let network = stage_lan(&store);
    let mut store = store.into_inner().unwrap();
    let original = store.active_network().unwrap();
    let binding = p::ProductDataBinding {
        schema_version: p::SchemaVersion(1),
        candidate_ref: "restored-network.db".into(),
        store_path: store
            .template
            .brain_store_path
            .parent()
            .unwrap()
            .join("data-operations/restored-network.db"),
        content_key_directory: store
            .template
            .brain_store_path
            .with_extension("content-keys"),
        owner_digest: p::sha256_content_digest(b"owner:test"),
        store_identity: p::sha256_content_digest(b"network:store"),
        manifest_digest: p::sha256_content_digest(b"network:manifest"),
        execution_fence_digest: Some(p::sha256_content_digest(b"network:fence")),
    };
    let request = p::ProductRestoreRequest {
        schema_version: p::SchemaVersion(1),
        install_ref: store.template.install_ref.clone(),
        operation_id: "restore:network".into(),
        command_digest: p::sha256_content_digest(b"restore:network"),
        expected_config_revision: 1,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        candidate_ref: binding.candidate_ref.clone(),
        candidate_digest: binding.manifest_digest.clone(),
    };
    let view = store
        .publish_restored_data(&request, binding.clone())
        .unwrap();
    assert_eq!(view.network, network);
    assert!(!view.network_active);
    assert_eq!(store.active_network().unwrap(), original);
    store.apply(patch("network:legacy-save", 2)).unwrap();
    drop(store);
    let mut store = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    let view = store.snapshot().unwrap();
    assert_eq!(view.network, network);
    assert!(!view.network_active);
    assert_eq!(store.active_network().unwrap(), original);
    assert_eq!(store.state.schema_version, p::SchemaVersion(6));
    assert_eq!(store.state.active_data, Some(binding));
}

#[test]
fn prepared_network_dispatch_is_one_shot_and_existing_post_only_queries() {
    let (directory, _path, store) = fixture();
    stage_lan(&store);
    let config = Arc::new(store);
    let services = Services::default();
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let request = staged_request(&host, &config, "network:deferred", 1);
    let (planned, ticket) = host
        .prepare_owner_network(&auth, request.clone(), NOW)
        .unwrap();
    assert_eq!(planned.stage, p::MaintenanceStage::Planned);
    assert!(services.firewall_calls.lock().unwrap().is_empty());
    assert!(services.service_calls.lock().unwrap().is_empty());
    let (original, duplicate) = host
        .prepare_owner_network(&auth, request.clone(), NOW)
        .unwrap();
    assert_eq!(original, planned);
    assert!(duplicate.is_none());
    let committed = host
        .execute_owner_network(ticket.unwrap(), NOW + 1)
        .unwrap();
    assert_eq!(committed.stage, p::MaintenanceStage::Committed);
    assert!(host
        .prepare_owner_network(&auth, request, NOW + 2)
        .unwrap()
        .1
        .is_none());
    assert_eq!(services.firewall_calls.lock().unwrap().len(), 1);
    assert_eq!(services.service_calls.lock().unwrap().len(), 2);
}

#[test]
fn unavailable_firewall_is_rejected_before_journal_admission() {
    let (directory, _path, store) = fixture();
    stage_lan(&store);
    let config = Arc::new(store);
    let services = Services {
        unsupported_firewall: true,
        ..Services::default()
    };
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let before = host.maintenance_snapshot(&auth).unwrap();
    assert!(!before
        .host
        .allowed_operations
        .contains(&"configure_lan".into()));
    let request = staged_request(&host, &config, "network:unsupported", 1);
    assert!(host.prepare_owner_network(&auth, request, NOW).is_err());
    let after = host.maintenance_snapshot(&auth).unwrap();
    assert_eq!(before.product_revision, after.product_revision);
    assert_eq!(before.operations, after.operations);
    assert!(services.service_calls.lock().unwrap().is_empty());
    assert!(services.firewall_calls.lock().unwrap().is_empty());
    assert!(!config.lock().unwrap().snapshot().unwrap().network_active);
}

#[test]
fn unconfigured_native_rule_is_refused_before_planning_or_web_restart() {
    let (directory, _path, store) = fixture();
    stage_lan(&store);
    let config = Arc::new(store);
    let services = Services {
        rejected_preflight: true,
        ..Services::default()
    };
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let before = host.maintenance_snapshot(&auth).unwrap();
    let request = staged_request(&host, &config, "network:manual-rule-missing", 1);
    assert!(host.prepare_owner_network(&auth, request, NOW).is_err());
    let after = host.maintenance_snapshot(&auth).unwrap();
    assert_eq!(before.product_revision, after.product_revision);
    assert!(after.operations.is_empty());
    assert!(services.firewall_calls.lock().unwrap().is_empty());
    assert!(services.service_calls.lock().unwrap().is_empty());
}

#[test]
fn a_lost_response_ticket_is_not_recreated_after_restart() {
    let (directory, _path, store) = fixture();
    stage_lan(&store);
    let config = Arc::new(store);
    let services = Services::default();
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let request = staged_request(&host, &config, "network:lost", 1);
    let (planned, ticket) = host
        .prepare_owner_network(&auth, request.clone(), NOW)
        .unwrap();
    drop(ticket);
    drop(host);
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let (observed, dispatch) = host.prepare_owner_network(&auth, request, NOW + 1).unwrap();
    assert_eq!(observed.operation_id, planned.operation_id);
    assert!(dispatch.is_none());
    assert!(services.service_calls.lock().unwrap().is_empty());
    assert!(services.firewall_calls.lock().unwrap().is_empty());
}

#[test]
fn prepared_ticket_rechecks_material_before_writing_an_effect_intent() {
    let (directory, _path, store) = fixture();
    let network = stage_lan(&store);
    let config = Arc::new(store);
    let services = Services::default();
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let request = staged_request(&host, &config, "network:ticket-stale", 1);
    let (_, ticket) = host.prepare_owner_network(&auth, request, NOW).unwrap();
    std::fs::write(&network.tls.unwrap().certificate_path, b"not a certificate").unwrap();
    assert!(host
        .execute_owner_network(ticket.unwrap(), NOW + 1)
        .is_err());
    let observed = host
        .owner_maintenance_operation(&auth, "network:ticket-stale")
        .unwrap();
    assert_eq!(observed.stage, p::MaintenanceStage::Planned);
    assert!(observed.pending_stage.is_none());
    assert!(services.firewall_calls.lock().unwrap().is_empty());
    assert!(services.service_calls.lock().unwrap().is_empty());
}

#[test]
fn legacy_installation_cannot_stage_an_unpublishable_network_change() {
    let (_directory, _path, store) = tests::fixture_with_network(false);
    let mut store = store.into_inner().unwrap();
    let mut update = patch("network:legacy", 0);
    update.network = Some(p::ProductNetworkConfig::loopback(
        "127.0.0.1:8999".parse().unwrap(),
    ));
    assert!(store.apply(update).is_err());
    assert_eq!(store.snapshot().unwrap().active_revision, 0);
    let mut unchanged = patch("network:legacy-unchanged", 0);
    unchanged.network = Some(p::ProductNetworkConfig::loopback(store.template.web_bind));
    store.apply(unchanged).unwrap();
    assert!(store.snapshot().unwrap().network_active);
}
