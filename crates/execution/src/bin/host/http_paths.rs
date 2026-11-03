//! Exact credential-domain route classification; no wildcard dispatch grant.
pub(super) fn is_agent_path(path: &str) -> bool {
    path == "/v1/agent/workers/dispatch" || path.starts_with("/v1/agent/workers/")
}

pub(super) fn is_configuration_path(path: &str) -> bool {
    matches!(
        path,
        "/v1/host/configuration"
            | "/v1/host/configuration/v2"
            | "/v1/host/configuration/owner"
            | "/v1/host/configuration/validate"
            | "/v1/host/configuration/data/verify"
            | "/v1/host/configuration/activate"
            | "/v1/host/configuration/maintenance"
    ) || path.starts_with("/v1/host/configuration/operations/")
        || path.starts_with("/v1/host/configuration/maintenance/operations/")
        || path == "/v1/host/configuration/release-recovery"
        || path == "/v1/host/configuration/release-recovery/verify"
        || path == "/v1/host/configuration/release-recovery/operations"
        || path.starts_with("/v1/host/configuration/release-recovery/candidates/")
        || path.starts_with("/v1/host/configuration/release-recovery/operations/")
}

pub(super) fn is_configuration_status_path(path: &str) -> bool {
    path == "/v1/host/status"
}
