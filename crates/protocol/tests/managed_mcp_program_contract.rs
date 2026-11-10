use forme_protocol as p;
use serde_json::json;

fn command() -> p::ManagedMcpProgramCommand {
    p::ManagedMcpProgramCommand {
        schema_version: p::SchemaVersion(1),
        installation_ref: "install:one".into(),
        owner_ref: p::OwnerPrincipalRef("owner:one".into()),
        authority_epoch: p::AuthorityEpoch(1),
        operation_id: "register:one".into(),
        expected_revision: 1,
        requested_deadline: 301_000,
        action: p::ManagedMcpProgramAction::Register {
            source_ref: "source:one".into(),
            source_digest: p::sha256_content_digest(b"source"),
        },
    }
}

#[test]
fn registration_wire_rejects_paths_overrides_unknown_fields_and_versions() {
    let cmd = command();
    let value = serde_json::to_value(&cmd).unwrap();
    assert_eq!(
        serde_json::from_value::<p::ManagedMcpProgramCommand>(value.clone()).unwrap(),
        cmd
    );
    for (field, input) in [
        ("program", json!("C:/private/tool.exe")),
        ("args", json!(["--replace"])),
        ("credentials", json!(["secret"])),
    ] {
        let mut changed = value.clone();
        changed["action"][field] = input;
        assert!(serde_json::from_value::<p::ManagedMcpProgramCommand>(changed).is_err());
    }
    let mut changed = value;
    changed["principal"] = json!("owner:other");
    assert!(serde_json::from_value::<p::ManagedMcpProgramCommand>(changed).is_err());
    let mut changed = cmd.clone();
    changed.schema_version = p::SchemaVersion(2);
    assert!(changed.validate().is_err());
    let mut changed = cmd;
    changed.action = p::ManagedMcpProgramAction::Register {
        source_ref: "../tool.exe".into(),
        source_digest: p::sha256_content_digest(b"source"),
    };
    assert!(changed.validate().is_err());
}

#[test]
fn operation_digest_binds_identity_epoch_revision_deadline_and_action() {
    let cmd = command();
    let digest = cmd.digest().unwrap();
    for index in 0..7 {
        let mut changed = cmd.clone();
        match index {
            0 => changed.installation_ref = "install:other".into(),
            1 => changed.owner_ref.0 = "owner:other".into(),
            2 => changed.authority_epoch.0 = 2,
            3 => changed.operation_id = "register:other".into(),
            4 => changed.expected_revision += 1,
            5 => changed.requested_deadline -= 1,
            _ => {
                changed.action = p::ManagedMcpProgramAction::Revoke {
                    program_ref: "program:one".into(),
                    registration_digest: p::sha256_content_digest(b"registration"),
                }
            }
        }
        assert_ne!(changed.digest().unwrap(), digest);
    }
    assert!(cmd.validate_at(1_000).is_ok());
    for now in [0, 999, 301_000, i64::MAX] {
        assert!(cmd.validate_at(now).is_err());
    }
}

#[test]
fn program_view_and_catalog_cannot_claim_other_profiles_or_rebind_digests() {
    let digest = p::sha256_content_digest(b"registered");
    let program = p::ManagedMcpProgramView {
        schema_version: p::SchemaVersion(1),
        program_ref: format!("mcp-program:{}", &digest.0[7..]),
        registration_digest: digest,
        source_ref: "source:one".into(),
        scope: p::Scope("workspace:test".into()),
        sandbox_profile_ref: p::MANAGED_MCP_PROGRAM_PROFILE.into(),
        revoked: false,
    };
    program.validate().unwrap();
    let mut changed = program.clone();
    changed.registration_digest = p::sha256_content_digest(b"other");
    assert!(changed.validate().is_err());
    changed = program.clone();
    changed.sandbox_profile_ref = "unrestricted".into();
    assert!(changed.validate().is_err());
    let mut catalog = p::ManagedMcpProgramCatalog {
        schema_version: p::SchemaVersion(1),
        installation_ref: "install:one".into(),
        owner_ref: command().owner_ref,
        authority_epoch: p::AuthorityEpoch(1),
        revision: 2,
        choices: vec![],
        programs: vec![program.clone()],
    };
    catalog.validate().unwrap();
    catalog.programs.push(program);
    assert!(catalog.validate().is_err());
}
