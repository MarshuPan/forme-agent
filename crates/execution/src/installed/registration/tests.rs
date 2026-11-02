use super::*;

fn info(root: &Path) -> ServiceInfo {
    desired(
        p::BuiltinServiceId::Web,
        &root.join("config/web.json"),
        root.join("program files/bin/forme-webd.exe"),
    )
    .unwrap()
}

fn existing(info: &ServiceInfo) -> ServiceConfig {
    ServiceConfig {
        service_type: info.service_type,
        start_type: info.start_type,
        error_control: info.error_control,
        executable_path: PathBuf::from(format!(
            "\"{}\" --config \"{}\"",
            info.executable_path.display(),
            info.launch_arguments[1].to_string_lossy()
        )),
        load_order_group: None,
        tag_id: 0,
        dependencies: info.dependencies.clone(),
        account_name: info
            .account_name
            .clone()
            .or_else(|| Some("LocalSystem".into())),
        display_name: info.display_name.clone(),
    }
}

#[test]
fn native_registration_rejects_foreign_command_account_or_startup() {
    // A machine with a real, protected installation must remain testable by a
    // non-administrator. Pure registration fixtures never inspect that install.
    let root = tempfile::tempdir().unwrap();
    let wanted = info(root.path());
    let original = existing(&wanted);
    assert!(matches_config(&original, &wanted));
    let image = wanted.executable_path.display();
    let config = wanted.launch_arguments[1].to_string_lossy();
    for command in [
        format!("{image} --config \"{config}\""),
        format!(
            "\"{image}\" --config \"{}\"",
            root.path().join("config/other.json").display()
        ),
        format!("\"{image}\" --config \"{config}\" --development-process-workers"),
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
fn native_registration_accepts_path_separators_but_rejects_changed_arguments() {
    let root = tempfile::tempdir().unwrap();
    for id in [
        p::BuiltinServiceId::Host,
        p::BuiltinServiceId::Web,
        p::BuiltinServiceId::Brain,
    ] {
        let wanted = desired(
            id,
            &root.path().join("config/core/core.json"),
            root.path()
                .join(format!("program files/bin/{}.exe", id.binary_name())),
        )
        .unwrap();
        let baseline = existing(&wanted);
        assert!(matches_config(&baseline, &wanted));
        for separator in ["/", "\\"] {
            let mut actual = baseline.clone();
            let command = baseline.executable_path.to_str().unwrap();
            actual.executable_path = command.replace(['/', '\\'], separator).into();
            assert!(matches_config(&actual, &wanted));
            for changed in [
                format!("{} --development-config", actual.executable_path.display()),
                actual
                    .executable_path
                    .to_str()
                    .unwrap()
                    .replace("--config", "--release-updater --config"),
                actual
                    .executable_path
                    .to_str()
                    .unwrap()
                    .replace("core.json", "other.json"),
            ] {
                let mut foreign = actual.clone();
                foreign.executable_path = changed.into();
                assert!(!matches_config(&foreign, &wanted));
            }
        }
    }
}

#[test]
fn native_registration_command_matching_preserves_updater_mode_and_paths() {
    let expected =
        r#""C:\ForMe\bin/forme-hostd.exe" --release-updater --config "C:\ForMe\config/host.json""#;
    let canonical = expected.replace('/', "\\");
    assert!(service_commands_match(expected, &canonical));
    assert!(service_commands_match(&canonical, expected));
    for changed in [
        canonical.replace("--release-updater ", ""),
        canonical.replace(" --config ", " --config= "),
        canonical.replace("host.json", "other.json"),
        canonical.replace("forme-hostd.exe", "foreign.exe"),
        canonical.replace("ForMe\\bin", "ForMe\\other\\..\\bin"),
        format!("{canonical} --development-config"),
    ] {
        assert!(!service_commands_match(expected, &changed));
    }
}

#[test]
fn native_registration_has_fixed_identity_and_finite_recovery() {
    let root = tempfile::tempdir().unwrap();
    let policy = p::ServiceRecoveryPolicy::product_default();
    policy.validate().unwrap();
    assert!(policy.is_product_default());
    assert_eq!(
        policy.decide(0).unwrap(),
        p::ServiceRecoveryDecision::RestartAfter(p::DurationMs(5_000))
    );
    assert_eq!(
        policy.decide(2).unwrap(),
        p::ServiceRecoveryDecision::RestartAfter(p::DurationMs(120_000))
    );
    assert_eq!(
        policy.decide(3).unwrap(),
        p::ServiceRecoveryDecision::RequireDiagnosis
    );
    let web = info(root.path());
    assert_eq!(
        web.account_name.unwrap(),
        OsString::from("NT SERVICE\\forme-web")
    );
    let host = desired(
        p::BuiltinServiceId::Host,
        &root.path().join("host.json"),
        root.path().join("bin/forme-hostd.exe"),
    )
    .unwrap();
    assert!(host.account_name.is_none());
    assert!(host.dependencies.is_empty());
    let brain = desired(
        p::BuiltinServiceId::Brain,
        &root.path().join("core.json"),
        root.path().join("bin/forme-gatewayd.exe"),
    )
    .unwrap();
    assert_eq!(brain.start_type, ServiceStartType::OnDemand);
    let actions = recovery().actions.unwrap();
    assert_eq!(actions.len(), 4);
    assert_eq!(actions.last().unwrap().action_type, ServiceActionType::None);
    assert_eq!(
        actions[..3]
            .iter()
            .map(|action| action.delay)
            .collect::<Vec<_>>(),
        [
            Duration::from_secs(5),
            Duration::from_secs(30),
            Duration::from_secs(120)
        ]
    );
    for path in [
        Path::new("relative.json"),
        Path::new("C:\\forme\\bad\".json"),
    ] {
        assert!(validate_path(path).is_err());
    }
}

#[test]
fn service_sid_policy_is_closed_to_the_installed_service_set() {
    for name in [
        "forme-host",
        "forme-web",
        "forme-brain",
        "forme-release-updater",
    ] {
        assert_eq!(
            expected_service_sid_type(std::ffi::OsStr::new(name)).unwrap(),
            ServiceSidType::Unrestricted
        );
    }
    assert!(expected_service_sid_type(std::ffi::OsStr::new("other-service")).is_err());
}

#[test]
fn recovery_policy_rejects_unbounded_or_non_monotonic_schedules() {
    let mut policy = p::ServiceRecoveryPolicy::product_default();
    policy.restart_delays[1] = policy.restart_delays[0];
    assert!(policy.validate().is_err());
    policy = p::ServiceRecoveryPolicy::product_default();
    policy.restart_delays.push(p::DurationMs(600_001));
    assert!(policy.validate().is_err());
    policy = p::ServiceRecoveryPolicy::product_default();
    policy.reset_after = p::DurationMs(7 * 24 * 60 * 60 * 1_000 + 1);
    assert!(policy.validate().is_err());
}
