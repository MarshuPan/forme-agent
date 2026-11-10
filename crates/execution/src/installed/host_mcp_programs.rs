//! Authenticated program management has no AgentExecution endpoint.
use super::*;
impl<M: NativeServiceManager> ProductHostBroker<M> {
    #[cfg(windows)]
    pub fn with_mcp_programs(
        mut self,
        programs: Arc<super::super::InstalledManagedMcpPrograms>,
    ) -> p::Result<Self> {
        if !programs.matches_configuration(
            &self.install_id,
            self.product_config.as_ref().ok_or_else(unavailable)?,
        ) {
            return Err(unavailable());
        }
        self.mcp_programs = Some(programs);
        Ok(self)
    }
    pub fn mcp_programs(&self, auth: &HostAuth) -> p::Result<p::ManagedMcpProgramCatalog> {
        self.verify_web_configuration_auth(auth)?;
        #[cfg(windows)]
        {
            self.mcp_programs
                .as_ref()
                .ok_or_else(unavailable)?
                .with_registry(|registry| registry.catalog())
        }
        #[cfg(not(windows))]
        {
            Err(unavailable())
        }
    }
    pub fn mcp_program_operation(
        &self,
        auth: &HostAuth,
        operation: &str,
    ) -> p::Result<p::ManagedMcpProgramOperation> {
        self.verify_web_configuration_auth(auth)?;
        p::validate_product_id(operation)?;
        #[cfg(windows)]
        {
            self.mcp_programs
                .as_ref()
                .ok_or_else(unavailable)?
                .with_registry(|registry| registry.operation(operation))
        }
        #[cfg(not(windows))]
        {
            Err(unavailable())
        }
    }
    pub fn apply_mcp_program(
        &self,
        auth: &HostAuth,
        command: p::ManagedMcpProgramCommand,
        now: p::Timestamp,
    ) -> p::Result<p::ManagedMcpProgramOperation> {
        #[cfg(windows)]
        let window = super::super::managed_mcp_program::CommandWindow::new(now);
        self.verify_web_configuration_auth(auth)?;
        command.validate()?;
        #[cfg(windows)]
        {
            self.mcp_programs
                .as_ref()
                .ok_or_else(unavailable)?
                .with_registry(|registry| {
                    let allow_mutation = !self
                        .journal
                        .operations(0)?
                        .iter()
                        .any(|operation| !operation.stage.is_terminal());
                    registry.apply_in_window(command, window, allow_mutation)
                })
        }
        #[cfg(not(windows))]
        {
            let _ = (command, now);
            Err(unavailable())
        }
    }
}
fn unavailable() -> p::Error {
    p::Error("managed MCP program authority is unavailable".into())
}
