//! Crate-private restore publication; ordinary browser patches never accept paths.
use super::*;

impl ProductConfigStore {
    pub(crate) fn restore_admission_pending(
        &self,
        id: &str,
        digest: &p::SchemaDigest,
    ) -> p::Result<bool> {
        let path = self
            .template
            .core_config_path
            .with_extension("work-admission.json");
        let admission: p::ProductWorkAdmission = storage::json(&path, 8192)?;
        admission.validate()?;
        if admission.install_ref != self.template.install_ref {
            return Err(invalid());
        }
        if admission.operation_id != id {
            return Ok(false);
        }
        if admission.command_digest != *digest {
            return Err(invalid());
        }
        Ok(!admission.released)
    }

    pub(crate) fn release_committed_restore(
        &mut self,
        command: &p::MaintenanceCommand,
        digest: &p::SchemaDigest,
    ) -> p::Result<()> {
        let p::MaintenanceKind::CommitRestore {
            candidate_ref,
            candidate_digest,
            owner_digest,
            config_revision,
        } = &command.kind
        else {
            return Err(invalid());
        };
        self.snapshot()?;
        let binding = self.state.active_data.clone().ok_or_else(invalid)?;
        let owner = self.state.owner.clone().ok_or_else(invalid)?;
        if binding.candidate_ref != *candidate_ref
            || binding.manifest_digest != *candidate_digest
            || binding.owner_digest != *owner_digest
        {
            return Err(invalid());
        }
        let request = p::ProductRestoreRequest {
            schema_version: p::SchemaVersion(1),
            install_ref: self.template.install_ref.clone(),
            operation_id: command.operation_id.clone(),
            command_digest: digest.clone(),
            expected_config_revision: *config_revision,
            owner,
            candidate_ref: candidate_ref.clone(),
            candidate_digest: candidate_digest.clone(),
        };
        self.restore_work_admission(&request, &binding, true)
    }

    pub(crate) fn restore_work_admission(
        &self,
        request: &p::ProductRestoreRequest,
        binding: &p::ProductDataBinding,
        released: bool,
    ) -> p::Result<()> {
        let revision = request
            .expected_config_revision
            .checked_add(1)
            .ok_or_else(invalid)?;
        if released
            && (self.state.active_data.as_ref() != Some(binding)
                || self.state.revision < revision
                || self.journal.acknowledged() != Some(self.state.revision))
        {
            return Err(invalid());
        }
        let admission = p::ProductWorkAdmission {
            schema_version: p::SchemaVersion(1),
            install_ref: request.install_ref.clone(),
            operation_id: request.operation_id.clone(),
            command_digest: request.command_digest.clone(),
            config_revision: revision,
            owner_digest: binding.owner_digest.clone(),
            execution_fence_digest: binding.execution_fence_digest.clone().ok_or_else(invalid)?,
            released,
        };
        admission.validate()?;
        storage::publish(
            &self
                .template
                .core_config_path
                .with_extension("work-admission.json"),
            &storage::encode(&admission)?,
        )
    }

    pub(crate) fn prepare_restore_request(
        &mut self,
        command: &p::MaintenanceCommand,
        command_digest: &p::SchemaDigest,
    ) -> p::Result<p::ProductRestoreRequest> {
        let p::MaintenanceKind::CommitRestore {
            candidate_ref,
            candidate_digest,
            owner_digest,
            config_revision,
        } = &command.kind
        else {
            return Err(invalid());
        };
        self.recover()?;
        self.verify_published()?;
        let owner = self.state.owner.clone().ok_or_else(invalid)?;
        self.check_revision(*config_revision, &owner, true)?;
        if self
            .state
            .active_data
            .as_ref()
            .is_some_and(|binding| binding.candidate_ref == *candidate_ref)
        {
            return Err(p::Error(
                "restore candidate is already the active brain".into(),
            ));
        }
        if p::sha256_content_digest(owner.0.as_bytes()) != *owner_digest || *config_revision == 0 {
            return Err(invalid());
        }
        let request = p::ProductRestoreRequest {
            schema_version: p::SchemaVersion(1),
            install_ref: self.template.install_ref.clone(),
            operation_id: command.operation_id.clone(),
            command_digest: command_digest.clone(),
            expected_config_revision: *config_revision,
            owner,
            candidate_ref: candidate_ref.clone(),
            candidate_digest: candidate_digest.clone(),
        };
        request.validate()?;
        Ok(request)
    }

    pub(crate) fn publish_restored_data(
        &mut self,
        request: &p::ProductRestoreRequest,
        binding: p::ProductDataBinding,
    ) -> p::Result<p::ProductConfigView> {
        request.validate()?;
        binding.validate()?;
        self.check_revision(request.expected_config_revision, &request.owner, true)?;
        let expected_path = self
            .template
            .brain_store_path
            .parent()
            .ok_or_else(invalid)?
            .join("data-operations")
            .join(&request.candidate_ref);
        if request.install_ref != self.template.install_ref
            || binding.store_path != expected_path
            || binding.candidate_ref != request.candidate_ref
            || binding.content_key_directory
                != self
                    .template
                    .brain_store_path
                    .with_extension("content-keys")
            || binding.owner_digest != p::sha256_content_digest(request.owner.0.as_bytes())
            || binding.manifest_digest != request.candidate_digest
            || binding.execution_fence_digest.is_none()
            || self
                .state
                .active_data
                .as_ref()
                .is_some_and(|old| old.store_identity != binding.store_identity)
        {
            return Err(invalid());
        }
        let mut state = self.state.clone();
        state.schema_version = p::SchemaVersion(if state.schema_version.0 >= 10 {
            state.schema_version.0
        } else if state.worker.is_some() {
            9
        } else if state.proxy.is_some() {
            8
        } else if state.release.is_some() {
            7
        } else if state.network.is_some() {
            6
        } else if state.capabilities.is_some() {
            5
        } else if state.brain.is_some() {
            4
        } else {
            3
        });
        state.revision = state.revision.checked_add(1).ok_or_else(invalid)?;
        state.active_data = Some(binding);
        let digest = p::canonical_digest(&(
            "forme-restore-configuration-v1",
            request,
            &state.active_data,
        ))?;
        let operation = format!("restore:{}", &digest.0[7..]);
        self.commit_state(operation, digest, state)?;
        self.snapshot()
    }
}
