use forme_protocol as p;
fn patch() -> p::ProductComputerSettingsPatch {
    p::ProductComputerSettingsPatch {
        schema_version: p::SchemaVersion(1),
        operation_id: "save:computer-golden".into(),
        expected_revision: 7,
        owner: p::OwnerPrincipalRef("owner:settings".into()),
        computer: p::ProductComputerPlacementSettings {
            schema_version: p::SchemaVersion(1),
            enabled: true,
            scope: p::Scope("workspace:default".into()),
            surface_refs: vec!["surface:windows-default".into()],
        },
    }
}
#[test]
fn desktop_settings_digest_matches_browser_and_changes_with_every_authority_field() {
    let original = patch();
    assert_eq!(
        original.digest().unwrap().0,
        "sha256:8a414b981e750c75018841f0fa0be00fd8de9308ef1a21fa5541e1c0c71b6072"
    );
    for index in 0..5 {
        let mut changed = original.clone();
        match index {
            0 => changed.operation_id = "save:another".into(),
            1 => changed.expected_revision += 1,
            2 => changed.owner.0 = "owner:another".into(),
            3 => changed.computer.enabled = false,
            _ => changed.computer.surface_refs = vec!["surface:other".into()],
        }
        assert_ne!(changed.digest().unwrap(), original.digest().unwrap());
    }
}
#[test]
fn desktop_settings_rejects_schema_foreign_fields_and_invalid_placement() {
    let encoded = serde_json::to_value(patch()).unwrap();
    for field in [
        "publisher_keys",
        "credentials",
        "models",
        "principal",
        "path",
    ] {
        let mut wrong = encoded.clone();
        wrong[field] = serde_json::json!("forged");
        assert!(serde_json::from_value::<p::ProductComputerSettingsPatch>(wrong).is_err());
    }
    let mut wrong = patch();
    wrong.schema_version = p::SchemaVersion(2);
    assert!(wrong.validate().is_err());
    let mut wrong = patch();
    wrong.computer.surface_refs.clear();
    assert!(wrong.validate().is_err());
}
