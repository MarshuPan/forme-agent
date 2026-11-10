//! Broker authentication and shared configuration binding; no SCM calls.
use super::*;
use crate::installed::{HostCredentials, HostManagement, ProductHostBroker, SystemServiceManager};
use std::sync::Arc;

#[test]
fn mcp_program_host_accepts_only_web_configuration_and_preserves_receipts() {
    let (_config_temp, _template, config) = crate::installed::product_config::tests::fixture();
    let fixture = Fixture::new();
    let config = Arc::new(config);
    let programs = Arc::new(
        InstalledManagedMcpPrograms::new(
            fixture.journal.clone(),
            "install:config-test".into(),
            p::AuthorityEpoch(3),
            config.clone(),
            fixture.root.clone(),
            vec![fixture.source.scope.clone()],
            vec![],
        )
        .unwrap(),
    );
    let credentials = HostCredentials::new("M".repeat(32), Some("A".repeat(32)))
        .unwrap()
        .with_web_configuration(Some("W".repeat(32)))
        .unwrap();
    let broker = ProductHostBroker::open_with_service_manager_and_config(
        &fixture._temp.path().join("host.jsonl"),
        "install:config-test",
        credentials,
        p::ProductTarget::WindowsX86_64,
        "fixture-release".into(),
        Some(config),
        SystemServiceManager,
    )
    .unwrap()
    .with_mcp_programs(programs)
    .unwrap();
    for wrong in ["M".repeat(32), "A".repeat(32), "X".repeat(32)] {
        assert!(broker.authenticate_web_configuration(&wrong).is_err());
    }
    let wrong_auths = [
        broker.authenticate(&"M".repeat(32)).unwrap(),
        broker.authenticate_agent(&"A".repeat(32)).unwrap(),
    ];
    for auth in &wrong_auths {
        assert!(broker.mcp_programs(auth).is_err());
        assert!(broker.mcp_program_operation(auth, "register:host").is_err());
    }
    assert!(!fixture.journal.exists());
    let auth = broker
        .authenticate_web_configuration(&"W".repeat(32))
        .unwrap();
    let catalog = broker.mcp_programs(&auth).unwrap();
    let command = p::ManagedMcpProgramCommand {
        schema_version: p::SchemaVersion(1),
        installation_ref: catalog.installation_ref,
        owner_ref: catalog.owner_ref,
        authority_epoch: catalog.authority_epoch,
        operation_id: "register:host".into(),
        expected_revision: catalog.revision,
        requested_deadline: 10_000,
        action: p::ManagedMcpProgramAction::Register {
            source_ref: catalog.choices[0].source_ref.clone(),
            source_digest: catalog.choices[0].source_digest.clone(),
        },
    };
    let before = fs::metadata(&fixture.journal).unwrap().len();
    for auth in &wrong_auths {
        assert!(broker
            .apply_mcp_program(auth, command.clone(), 1_000)
            .is_err());
    }
    let mut wrong = command.clone();
    wrong.owner_ref.0 = "owner:forged".into();
    assert!(broker.apply_mcp_program(&auth, wrong, 1_000).is_err());
    assert_eq!(fs::metadata(&fixture.journal).unwrap().len(), before);
    let result = broker
        .apply_mcp_program(&auth, command.clone(), 1_000)
        .unwrap();
    assert_eq!(
        broker
            .mcp_program_operation(&auth, &command.operation_id)
            .unwrap(),
        result
    );
    let revoked = broker
        .apply_mcp_program(&auth, revoke(&command, &result), 2_000)
        .unwrap();
    assert!(revoked.program.revoked);
    assert_eq!(
        broker.apply_mcp_program(&auth, command, 100_000).unwrap(),
        result
    );
    assert!(broker.mcp_programs(&auth).unwrap().programs[0].revoked);
}

#[test]
fn mcp_program_initialization_rejects_configuration_from_another_installation() {
    let (_config_temp, _template, config) = crate::installed::product_config::tests::fixture();
    let fixture = Fixture::new();
    let programs = InstalledManagedMcpPrograms::new(
        fixture.journal.clone(),
        "install:wrong".into(),
        p::AuthorityEpoch(3),
        Arc::new(config),
        fixture.root.clone(),
        vec![fixture.source.scope.clone()],
        vec![],
    )
    .unwrap();
    assert!(programs
        .with_registry(|registry| registry.catalog())
        .is_err());
    assert!(!fixture.journal.exists());
}
