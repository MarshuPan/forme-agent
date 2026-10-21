use super::*;

fn info() -> ServiceInfo {
    desired(
        p::BuiltinServiceId::Web,
        Path::new(r"C:\forme\config\web.json"),
        PathBuf::from(r"C:\Program Files\forme\bin\forme-webd.exe"),
    )
    .unwrap()
}

fn existing(info: &ServiceInfo) -> ServiceConfig {
    ServiceConfig {
        service_type: info.service_type,
        start_type: info.start_type,
        error_control: info.error_control,
        executable_path: PathBuf::from(format!(
            "\"{}\" --config {}",
            info.executable_path.display(),
            info.launch_arguments[1].to_string_lossy()
        )),
        load_order_group: None,
        tag_id: 0,
        dependencies: info.dependencies.clone(),
        account_name: info.account_name.clone(),
        display_name: info.display_name.clone(),
    }
}

#[test]
fn native_registration_rejects_foreign_command_account_or_startup() {
    let wanted = info();
    let original = existing(&wanted);
    assert!(matches_config(&original, &wanted));
    for command in [
        r"C:\Program Files\forme\bin\forme-webd.exe --config C:\forme\config\web.json",
        r#""C:\Program Files\forme\bin\forme-webd.exe" --config C:\forme\config\other.json"#,
        r#""C:\Program Files\forme\bin\forme-webd.exe" --config C:\forme\config\web.json --development-process-workers"#,
    ] {
        let mut changed = original.clone();
        changed.executable_path = command.into();
        assert!(!matches_config(&changed, &wanted));
    }
    let mut changed = original.clone();
    changed.account_name = Some("LocalSystem".into());
    assert!(!matches_config(&changed, &wanted));
    changed = original.clone();
    changed.dependencies.clear();
    assert!(!matches_config(&changed, &wanted));
    changed = original;
    changed.start_type = ServiceStartType::Disabled;
    assert!(!matches_config(&changed, &wanted));
}

#[test]
fn native_registration_has_fixed_identity_and_finite_recovery() {
    let web = info();
    assert_eq!(
        web.account_name.unwrap(),
        OsString::from("NT SERVICE\\forme-web")
    );
    let host = desired(
        p::BuiltinServiceId::Host,
        Path::new(r"C:\forme\host.json"),
        PathBuf::from(r"C:\forme\bin\forme-hostd.exe"),
    )
    .unwrap();
    assert!(host.account_name.is_none());
    assert!(host.dependencies.is_empty());
    let brain = desired(
        p::BuiltinServiceId::Brain,
        Path::new(r"C:\forme\core.json"),
        PathBuf::from(r"C:\forme\bin\forme-gatewayd.exe"),
    )
    .unwrap();
    assert_eq!(brain.start_type, ServiceStartType::OnDemand);
    let actions = recovery().actions.unwrap();
    assert_eq!(actions.len(), 4);
    assert_eq!(actions.last().unwrap().action_type, ServiceActionType::None);
    for path in [
        Path::new("relative.json"),
        Path::new("C:\\forme\\bad\".json"),
    ] {
        assert!(validate_path(path).is_err());
    }
}
