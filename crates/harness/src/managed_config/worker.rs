//! Managed File placement is a readonly Host projection, never a local fallback.
use super::*;

impl ManagedBrainConfig {
    pub(super) fn configure_file_placement(
        &self,
        capabilities: &mut CapabilityRuntimeConfig,
    ) -> p::Result<()> {
        if self.worker_file_roots.is_empty() {
            return Ok(());
        }
        let binding = self.worker_binding.as_ref().ok_or_else(invalid)?;
        binding.validate()?;
        if !binding.supports(p::BackendKind::File) || self.worker_file_roots.len() > 64 {
            return Err(invalid());
        }
        let keys = self.content_key_directory()?;
        for (scope, root) in &self.worker_file_roots {
            p::validate_product_id(scope).map_err(|_| invalid())?;
            if !scope.starts_with("workspace:") {
                return Err(invalid());
            }
            check_managed_path(root)?;
            if !root.is_dir()
                || [&self.store_path, &self.artifact_root, &keys]
                    .into_iter()
                    .chain(self.models.iter().map(|model| &model.credential_path))
                    .any(|protected| protected.starts_with(root) || root.starts_with(protected))
            {
                return Err(invalid());
            }
        }
        if let Some(root) = self.worker_file_roots.get(&self.workspace.0) {
            capabilities.file.enabled = true;
            capabilities.file.roots = vec![root.to_string_lossy().replace('\\', "/")];
        }
        Ok(())
    }
}
