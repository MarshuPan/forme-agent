use forme_protocol as p;
use forme_store::EventStore;

use crate::{now_ms, production::ProductionRuntime};

impl ProductionRuntime {
    pub fn workspace_charter(
        &self,
        owner: &p::VerifiedPrincipal,
        workspace: &p::WorkspaceRef,
    ) -> p::Result<Option<p::WorkspaceCharterRecord>> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        if *workspace != self.config().workspace {
            return Err(p::Error(
                "charter read is outside the runtime workspace".into(),
            ));
        }
        self.services().store.workspace_charter(workspace)
    }

    pub fn prepare_workspace_charter(
        &self,
        owner: &p::VerifiedPrincipal,
        mut record: p::WorkspaceCharterRecord,
        expected_version: u64,
    ) -> p::Result<p::WorkspaceCharterChangedPayload> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        if record.workspace != self.config().workspace || record.actor != p::Actor::Owner {
            return Err(p::Error(
                "charter control is outside its owner or workspace".into(),
            ));
        }
        record.version = expected_version
            .checked_add(1)
            .ok_or_else(|| p::Error("charter version exhausted".into()))?;
        record.refresh_digest()?;
        p::CharterRules::parse(&record)?;
        Ok(p::WorkspaceCharterChangedPayload {
            charter: record,
            expected_version,
            committed_version: expected_version + 1,
        })
    }

    pub fn apply_workspace_charter(
        &self,
        owner: &p::VerifiedPrincipal,
        command: p::IdempotencyKey,
        change: p::WorkspaceCharterChangedPayload,
    ) -> p::Result<p::EventId> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        if change.charter.workspace != self.config().workspace || command.0.trim().is_empty() {
            return Err(p::Error(
                "charter command has an invalid workspace or identity".into(),
            ));
        }
        change.validate()?;
        p::CharterRules::parse(&change.charter)?;
        let store = self.services().store.clone();
        let id = p::EventId(format!(
            "charter-command:{}",
            p::canonical_digest(&(owner, command))?.0
        ));
        let payload = p::EventPayload::WorkspaceCharterChanged(change);
        for event in store.read_run(self.services().aggregates.charter.clone()) {
            let event = event?;
            if event.event_id == id {
                return if event.payload == payload {
                    Ok(id)
                } else {
                    Err(p::Error("charter command identity conflict".into()))
                };
            }
        }
        store.append(p::Event::new(
            id,
            self.services().aggregates.charter.clone(),
            None,
            payload,
            p::SchemaVersion(1),
            now_ms(),
            p::Provenance {
                source: p::Source::OwnerControl,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
        ))
    }
}
