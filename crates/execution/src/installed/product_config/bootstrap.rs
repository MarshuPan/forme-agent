//! Narrow first-adoption CAS. No model publication or runtime acknowledgement.
use super::*;

impl ProductConfigStore {
    pub(crate) fn bootstrap_admission(&mut self, install: &str) -> p::Result<()> {
        self.recover()?;
        self.verify_published()?;
        if self.template.install_ref != install
            || self.state.revision != 0
            || self.state.owner.is_some()
            || self.state.active_data.is_some()
            || !self.state.models.is_empty()
            || !self.state.credentials.is_empty()
            || self.journal.acknowledged().is_some()
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub(crate) fn publish_bootstrap_data(
        &mut self,
        intent: &p::BootstrapDataIntentV2,
        data: &p::BootstrapDataPreparedResultV2,
        commit: &p::BootstrapClaimCommitV2,
    ) -> p::Result<(String, p::SchemaDigest)> {
        data.validate_for(intent)?;
        commit.validate_against_prepared(&data.prepared)?;
        if commit.receipt.account_commitment != intent.request.account_commitment
            || commit.receipt.owner_ref
                != *intent.candidate.owner_ref.as_ref().ok_or_else(invalid)?
            || commit.receipt.install_ref != intent.install_ref
        {
            return Err(invalid());
        }
        let operation = &intent.request.session.operation_id;
        let digest = commit.receipt.digest()?;
        self.recover()?;
        if !self.journal.existing(operation, &digest)? {
            self.bootstrap_admission(&intent.install_ref)?;
            let root = self
                .template
                .brain_store_path
                .parent()
                .ok_or_else(invalid)?
                .join("bootstrap-data")
                .join(intent.target_ref()?);
            if data.binding.store_path != root.join("forme.db")
                || data.binding.content_key_directory != root.join("forme.content-keys")
            {
                return Err(invalid());
            }
            let mut state = self.state.clone();
            state.owner = Some(commit.receipt.owner_ref.clone());
            state.active_data = Some(data.binding.clone());
            self.commit_state(operation.clone(), digest, state)?;
        }
        self.verify_published()?;
        if self.state.revision != 0
            || self.state.owner.as_ref() != Some(&commit.receipt.owner_ref)
            || self.state.active_data.as_ref() != Some(&data.binding)
            || !self.state.models.is_empty()
            || self.journal.acknowledged().is_some()
        {
            return Err(invalid());
        }
        let reference = self.journal.current().ok_or_else(invalid)?;
        Ok((reference.id.clone(), reference.state_digest.clone()))
    }
}
