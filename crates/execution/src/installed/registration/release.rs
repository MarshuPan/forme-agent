//! SCM configuration change only; the independent updater owns service lifecycle.
use super::*;
use crate::installed::ReleaseServiceImageChange;

pub(in crate::installed) fn verify_original(
    change: &ReleaseServiceImageChange,
) -> p::Result<p::SchemaDigest> {
    let expected = desired(
        change.service(),
        change.configuration_path(),
        change.source_image().to_owned(),
    )?;
    let service = open(change.service(), false)?;
    snapshot(&service, &expected).and_then(|config| config_digest(&config))
}

pub(in crate::installed) fn switch_and_verify(
    change: &ReleaseServiceImageChange,
    prepared_snapshot: &p::SchemaDigest,
) -> p::Result<p::SchemaDigest> {
    let expected = desired(
        change.service(),
        change.configuration_path(),
        change.source_image().to_owned(),
    )?;
    let service = open(change.service(), true)?;
    let original = snapshot(&service, &expected)?;
    if config_digest(&original)? != *prepared_snapshot {
        return Err(invalid());
    }
    let mut replacement = desired(
        change.service(),
        change.configuration_path(),
        change.target_image().to_owned(),
    )?;
    replacement.display_name = original.display_name.clone();
    // NULL account/password preserves the current identity, including its
    // credential. No service is deleted, recreated or assigned a new account.
    replacement.account_name = None;
    replacement.dependencies = original.dependencies.clone();
    service.change_config(&replacement).map_err(|_| invalid())?;
    let wanted = desired(
        change.service(),
        change.configuration_path(),
        change.target_image().to_owned(),
    )?;
    let actual = snapshot(&service, &wanted)?;
    if !preserved(&original, &actual) {
        return Err(invalid());
    }
    config_digest(&actual)
}

fn open(id: p::BuiltinServiceId, change: bool) -> p::Result<Service> {
    let manager = manager(ServiceManagerAccess::CONNECT)?;
    let mut access = ServiceAccess::QUERY_CONFIG | ServiceAccess::QUERY_STATUS;
    if change {
        access |= ServiceAccess::CHANGE_CONFIG;
    }
    manager
        .open_service(id.service_name(), access)
        .map_err(|_| invalid())
}

fn snapshot(service: &Service, expected: &ServiceInfo) -> p::Result<ServiceConfig> {
    stopped(service)?;
    verify(service, expected)?;
    let actual = service.query_config().map_err(|_| invalid())?;
    if !matches_config(&actual, expected) {
        return Err(invalid());
    }
    stopped(service)?;
    Ok(actual)
}

fn stopped(service: &Service) -> p::Result<()> {
    if service.query_status().map_err(|_| invalid())?.current_state != ServiceState::Stopped {
        return Err(invalid());
    }
    Ok(())
}

fn preserved(before: &ServiceConfig, after: &ServiceConfig) -> bool {
    before.service_type == after.service_type
        && before.start_type == after.start_type
        && before.error_control == after.error_control
        && before.account_name == after.account_name
        && before.dependencies == after.dependencies
        && before.display_name == after.display_name
        && before.load_order_group == after.load_order_group
        && before.tag_id == after.tag_id
}

fn config_digest(config: &ServiceConfig) -> p::Result<p::SchemaDigest> {
    // This query contains configuration only, never a service password. Windows
    // enums have no serde representation; the pinned wrapper's Debug form is a
    // local receipt encoding, not a wire authorization or executable command.
    p::canonical_digest(&("forme-native-service-image-v1", format!("{config:?}")))
}

fn invalid() -> p::Error {
    p::Error(
        "native service image changed or its outcome is unknown; inspect the original operation"
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> ServiceConfig {
        ServiceConfig {
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: "old --config fixed".into(),
            load_order_group: Some("kept-group".into()),
            tag_id: 7,
            dependencies: vec![ServiceDependency::Service("forme-host".into())],
            account_name: Some("NT SERVICE\\forme-web".into()),
            display_name: "existing display".into(),
        }
    }
    #[test]
    fn image_change_preserves_identity_startup_dependency_group_and_display() {
        let original = config();
        let mut target = config();
        target.executable_path = "new --config fixed".into();
        assert!(preserved(&original, &target));
        assert_ne!(
            config_digest(&original).unwrap(),
            config_digest(&target).unwrap()
        );
        let mut changed = target.clone();
        changed.account_name = None;
        assert!(!preserved(&original, &changed));
        changed = target.clone();
        changed.dependencies.clear();
        assert!(!preserved(&original, &changed));
        changed = target.clone();
        changed.tag_id += 1;
        assert!(!preserved(&original, &changed));
        changed = target.clone();
        changed.load_order_group = None;
        assert!(!preserved(&original, &changed));
        changed = target.clone();
        changed.display_name = "replaced".into();
        assert!(!preserved(&original, &changed));
        changed = target;
        changed.start_type = ServiceStartType::Disabled;
        assert!(!preserved(&original, &changed));
    }
}
