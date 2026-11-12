//! Exact credential-domain route classification; no wildcard dispatch grant.
pub(super) fn is_agent_path(path: &str) -> bool {
    path == "/v1/agent/workers/dispatch" || path.starts_with("/v1/agent/workers/")
}

pub(super) fn is_configuration_path(path: &str) -> bool {
    matches!(
        path,
        "/v1/host/configuration"
            | "/v1/host/configuration/v2"
            | "/v1/host/configuration/v3"
            | "/v1/host/configuration/owner"
            | "/v1/host/configuration/validate"
            | "/v1/host/configuration/data/verify"
            | "/v1/host/configuration/activate"
            | "/v1/host/configuration/maintenance"
            | "/v1/host/configuration/desktop/settings"
            | "/v1/host/configuration/desktop/sessions"
            | "/v1/host/configuration/desktop/consents"
            | "/v1/host/configuration/desktop/revocations"
    ) || super::super::mcp_program_routes::is_path(path)
        || super::super::mcp_settings_routes::is_path(path)
        || super::super::plugin_settings_routes::is_path(path)
        || super::super::channel_settings_routes::is_path(path)
        || path
            .strip_prefix("/v1/host/configuration/desktop/consents/")
            .is_some_and(|id| forme_protocol::validate_product_id(id).is_ok())
        || path
            .strip_prefix("/v1/host/configuration/desktop/settings/operations/")
            .is_some_and(|id| forme_protocol::validate_product_id(id).is_ok())
        || path.starts_with("/v1/host/configuration/operations/")
        || path.starts_with("/v1/host/configuration/maintenance/operations/")
        || path == "/v1/host/configuration/release-recovery"
        || path == "/v1/host/configuration/release-recovery/verify"
        || path == "/v1/host/configuration/release-recovery/operations"
        || path.starts_with("/v1/host/configuration/release-recovery/candidates/")
        || path.starts_with("/v1/host/configuration/release-recovery/operations/")
        || path == "/v1/host/configuration/legacy-release-recovery"
        || path == "/v1/host/configuration/legacy-release-recovery/verify"
        || path == "/v1/host/configuration/legacy-release-recovery/operations"
        || path.starts_with("/v1/host/configuration/legacy-release-recovery/review/")
        || path.starts_with("/v1/host/configuration/legacy-release-recovery/operations/")
}

pub(super) fn is_configuration_status_path(path: &str) -> bool {
    path == "/v1/host/status"
}

pub(super) fn worker_id<'a>(
    path: &'a str,
    prefix: &str,
    suffix: &str,
) -> forme_protocol::Result<&'a str> {
    let value = path
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(suffix))
        .filter(|value| !value.is_empty() && !value.contains('/'))
        .ok_or_else(|| forme_protocol::Error("worker route job id is invalid".into()))?;
    forme_protocol::validate_product_id(value)
        .map_err(|_| forme_protocol::Error("worker route job id is invalid".into()))?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_recovery_paths_are_configuration_only_without_a_management_wildcard() {
        let root = "/v1/host/configuration/legacy-release-recovery";
        for suffix in [
            "",
            "/verify",
            "/operations",
            "/review/original:upgrade",
            "/operations/legacy:decision",
        ] {
            let path = format!("{root}{suffix}");
            assert!(is_configuration_path(&path));
            assert!(!is_agent_path(&path));
            assert!(!is_configuration_status_path(&path));
        }
        for suffix in ["/execute", "/sign", "/force", "/report", "/verify/extra"] {
            assert!(!is_configuration_path(&format!("{root}{suffix}")));
        }
        for path in [
            "/v1/agent/workers/dispatch",
            "/v1/host/plan",
            "/v1/host/operations/legacy/execute",
        ] {
            assert!(!is_configuration_path(path));
        }
    }
}
