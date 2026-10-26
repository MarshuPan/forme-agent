//! Host-only release configuration and its independent work-admission fence.
use super::*;

#[cfg(test)]
mod tests;

pub(super) fn validate_state(
    state: &ConfigState,
    template: &p::ProductConfigTemplate,
) -> p::Result<()> {
    // Schema 7 introduced the release binding; schema 8 adds the model proxy
    // while preserving the ability to carry that binding. A proxy-only state
    // is therefore valid at schema 8 without a release binding.
    if (state.schema_version == p::SchemaVersion(7) && state.release.is_none())
        || (state.schema_version.0 < 7 && state.release.is_some())
    {
        return Err(invalid());
    }
    if let Some(binding) = &state.release {
        binding.validate()?;
        if binding.install_ref != template.install_ref
            || binding.config_revision > state.revision
            || state
                .owner
                .as_ref()
                .map(|o| p::sha256_content_digest(o.0.as_bytes()))
                != Some(binding.owner_digest.clone())
            || state.active_data.as_ref().is_some_and(|data| {
                data.owner_digest != binding.owner_digest
                    || data.store_identity != binding.store_identity
            })
        {
            return Err(invalid());
        }
    }
    Ok(())
}

impl ProductConfigStore {
    pub(crate) fn release_configuration_identity(
        &mut self,
    ) -> p::Result<(p::OwnerPrincipalRef, u64, p::SchemaDigest)> {
        self.recover()?;
        self.verify_published()?;
        Ok((
            self.state.owner.clone().ok_or_else(invalid)?,
            self.state.revision,
            materialize::bytes_digest(&storage::read(
                &self.template.core_config_path,
                MAX_CONFIG_BYTES,
            )?),
        ))
    }

    pub(crate) fn publish_release_binding(
        &mut self,
        takeover: &p::ProductReleaseTakeover,
        binding: p::ProductReleaseBinding,
        at: p::Timestamp,
    ) -> p::Result<()> {
        binding.validate_for(takeover)?;
        if at < 0
            || at >= takeover.deadline
            || self.release_configuration_identity()?
                != (
                    takeover.owner.clone(),
                    takeover.expected_config_revision,
                    takeover.expected_config_digest.clone(),
                )
        {
            return Err(invalid());
        }
        let digest = p::canonical_digest(&("forme-release-configuration-v1", takeover, &binding))?;
        let mut state = self.state.clone();
        state.schema_version = p::SchemaVersion(if state.worker.is_some() {
            9
        } else if state.proxy.is_some() {
            8
        } else {
            7
        });
        state.revision = binding.config_revision;
        state.release = Some(binding.clone());
        state.validate(&self.template)?;
        // Keep the next runtime closed even if configuration publication fails.
        self.write_release_admission(&binding, false, at)?;
        self.commit_state(format!("release:{}", &digest.0[7..]), digest, state)
    }

    pub(crate) fn verify_release_readiness(
        &self,
        binding: &p::ProductReleaseBinding,
        revision: u64,
    ) -> p::Result<p::ProductReleaseReadiness> {
        let readiness: p::ProductReleaseReadiness = storage::json(
            &self
                .template
                .configuration_ack_path
                .with_extension("release-readiness.json"),
            8192,
        )?;
        readiness.validate()?;
        if readiness.binding != *binding || readiness.config_revision != revision {
            return Err(invalid());
        }
        Ok(readiness)
    }

    pub(crate) fn release_binding(&self) -> Option<&p::ProductReleaseBinding> {
        self.state.release.as_ref()
    }

    pub(crate) fn live_release_readiness(
        &mut self,
        binding: &p::ProductReleaseBinding,
        deadline: std::time::Instant,
    ) -> p::Result<p::ProductReleaseReadiness> {
        self.recover()?;
        self.verify_published()?;
        self.sync_acknowledgement()?;
        if self.state.release.as_ref() != Some(binding)
            || self.journal.acknowledged() != Some(self.state.revision)
        {
            return Err(invalid());
        }
        let saved = self.verify_release_readiness(binding, self.state.revision)?;
        let credential =
            zeroize::Zeroizing::new(storage::secret(&self.template.core_owner_credential_path)?);
        let live = crate::installed::release_probe::probe(
            self.template.core_bind,
            &credential,
            binding,
            self.state.revision,
            deadline,
        )?;
        if live != saved {
            return Err(invalid());
        }
        Ok(live)
    }

    pub(crate) fn release_work_admission(
        &self,
        binding: &p::ProductReleaseBinding,
        at: p::Timestamp,
    ) -> p::Result<()> {
        if self.state.release.as_ref() != Some(binding)
            || self.journal.acknowledged() != Some(self.state.revision)
        {
            return Err(invalid());
        }
        self.verify_release_readiness(binding, self.state.revision)?;
        self.write_release_admission(binding, true, at)
    }

    pub(crate) fn release_is_open(&self, binding: &p::ProductReleaseBinding) -> p::Result<bool> {
        let admission: p::ProductReleaseAdmission = storage::json(
            &self
                .template
                .core_config_path
                .with_extension("release-admission.json"),
            8192,
        )?;
        admission.validate_for(binding)?;
        Ok(admission.released)
    }

    fn write_release_admission(
        &self,
        binding: &p::ProductReleaseBinding,
        released: bool,
        at: p::Timestamp,
    ) -> p::Result<()> {
        let admission = p::ProductReleaseAdmission {
            schema_version: p::SchemaVersion(1),
            binding_digest: binding.digest()?,
            released,
            at,
        };
        admission.validate_for(binding)?;
        storage::publish(
            &self
                .template
                .core_config_path
                .with_extension("release-admission.json"),
            &storage::encode(&admission)?,
        )
    }
}
