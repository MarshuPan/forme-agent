//! No wildcard forwarding; the browser cannot provide a program or credentials.
use super::*;
const ROOT: &str = "/v1/host/configuration/mcp/programs";
impl LocalConfigurationClient {
    pub(in crate::web) fn mcp_programs(&self) -> p::Result<p::ManagedMcpProgramCatalog> {
        let catalog: p::ManagedMcpProgramCatalog = self.request("GET", ROOT, None::<&()>)?;
        catalog.validate()?;
        Ok(catalog)
    }
    pub(in crate::web) fn apply_mcp_program(
        &self,
        command: p::ManagedMcpProgramCommand,
    ) -> p::Result<p::ManagedMcpProgramOperation> {
        let digest = command.digest()?;
        let operation: p::ManagedMcpProgramOperation =
            self.request("POST", &format!("{ROOT}/commands"), Some(&command))?;
        operation.validate()?;
        if operation.operation_id != command.operation_id || operation.command_digest != digest {
            return Err(invalid());
        }
        Ok(operation)
    }
    pub(in crate::web) fn mcp_program_operation(
        &self,
        id: &str,
    ) -> p::Result<p::ManagedMcpProgramOperation> {
        p::validate_product_id(id)?;
        let operation: p::ManagedMcpProgramOperation =
            self.request("GET", &format!("{ROOT}/operations/{id}"), None::<&()>)?;
        operation.validate()?;
        if operation.operation_id != id {
            return Err(invalid());
        }
        Ok(operation)
    }
}
pub(super) fn allows_route(method: &str, path: &str) -> bool {
    matches!(
        (method, path),
        ("GET", ROOT) | ("POST", "/v1/host/configuration/mcp/programs/commands")
    ) || (method == "GET"
        && path
            .strip_prefix("/v1/host/configuration/mcp/programs/operations/")
            .is_some_and(|id| p::validate_product_id(id).is_ok()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn program_routes_preserve_method_domain_and_exact_operation_references() {
        assert!(allows_route("GET", ROOT));
        assert!(allows_route("POST", &format!("{ROOT}/commands")));
        assert!(allows_route(
            "GET",
            &format!("{ROOT}/operations/register:one")
        ));
        for path in [
            format!("{ROOT}/execute"),
            format!("{ROOT}/sources"),
            format!("{ROOT}/operations/a/b"),
            format!("{ROOT}/operations/a?x=1"),
            "/v1/agent/workers/dispatch".into(),
        ] {
            assert!(!allows_route("GET", &path));
            assert!(!allows_route("POST", &path));
        }
        assert!(!allows_route("POST", ROOT));
        assert!(!allows_route("GET", &format!("{ROOT}/commands")));
    }
}
