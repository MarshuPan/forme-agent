//! Content-free correction receipts stay readable after feedback erasure.
use super::*;
use serde::{Deserialize, Serialize};

pub(super) const PREFIX: &str = "workbench-correction-v1|";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CorrectionFact {
    pub schema_version: p::SchemaVersion,
    pub command: p::IdempotencyKey,
    pub binding: p::SchemaDigest,
    pub target: p::SchemaDigest,
    pub candidate: p::CandidateId,
}

pub(super) fn key(
    owner: &p::VerifiedPrincipal,
    command: &p::IdempotencyKey,
) -> p::Result<p::SchemaDigest> {
    p::canonical_digest(&(owner, command))
}

pub(super) fn event_id(key: &p::SchemaDigest, stage: &str) -> p::EventId {
    p::EventId(format!("owner-correction:{}:{stage}", key.0))
}

pub(super) fn candidate_id(key: &p::SchemaDigest) -> p::CandidateId {
    p::CandidateId(format!("owner-correction-candidate:{}", key.0))
}

impl ProductionRuntime {
    /// Pure authenticated projection. An absent receipt is not permission to
    /// dispatch a replacement command while the original request is unknown.
    pub fn workbench_corrections(
        &self,
        owner: &p::VerifiedPrincipal,
        query: p::WorkbenchCorrectionQuery,
    ) -> p::Result<p::WorkbenchCorrectionsView> {
        let _data = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        query.validate()?;
        let mut receipts = self.correction_receipts(owner)?;
        receipts.reverse();
        if let Some(command) = query.command {
            receipts.retain(|v| v.command == command);
        }
        let truncated = receipts.len() > 100;
        receipts.truncate(100);
        Ok(p::WorkbenchCorrectionsView {
            schema_version: p::SchemaVersion(1),
            receipts,
            truncated,
        })
    }

    pub(super) fn correction_receipts(
        &self,
        owner: &p::VerifiedPrincipal,
    ) -> p::Result<Vec<p::WorkbenchCorrectionReceipt>> {
        let mut receipts = Vec::new();
        let mut seen = BTreeSet::new();
        for event in self
            .services()
            .store
            .read_run(self.services().aggregates.memory_cognition.clone())
        {
            let event = event?;
            let p::EventPayload::MemoryMaintenanceApplied(value) = &event.payload else {
                continue;
            };
            let Some(encoded) = value.deltas_ref.0.strip_prefix(PREFIX) else {
                continue;
            };
            if encoded.len() > 4096 {
                return Err(p::Error("owner correction receipt is unbounded".into()));
            }
            let fact: CorrectionFact = serde_json::from_str(encoded)
                .map_err(|_| p::Error("owner correction receipt is invalid".into()))?;
            let key = key(owner, &fact.command)?;
            p::WorkbenchCorrectionQuery {
                schema_version: fact.schema_version,
                command: Some(fact.command.clone()),
            }
            .validate()?;
            p::sha256_digest_bytes(&fact.binding)?;
            p::sha256_digest_bytes(&fact.target)?;
            if event.provenance.source != p::Source::OwnerControl
                || event.provenance.actor != p::Actor::Owner
                || event.provenance.trust_tier != p::TrustTier::OwnerInput
                || event.event_id != event_id(&key, "receipt")
                || fact.candidate != candidate_id(&key)
                || !seen.insert(fact.command.clone())
            {
                return Err(p::Error(
                    "owner correction receipt binding is invalid".into(),
                ));
            }
            receipts.push(p::WorkbenchCorrectionReceipt {
                schema_version: p::SchemaVersion(1),
                command: fact.command,
                binding: fact.binding,
                status: p::WorkbenchCommandStatus::Confirmed,
                event: event.event_id,
                candidate: fact.candidate,
            });
            if receipts.len() > 50_000 {
                return Err(p::Error("owner correction history is unbounded".into()));
            }
        }
        Ok(receipts)
    }

    pub(super) fn previous_correction(
        &self,
        owner: &p::VerifiedPrincipal,
        command: &p::WorkbenchCorrectionCommand,
    ) -> p::Result<Option<p::WorkbenchCorrectionReceipt>> {
        let previous = self
            .correction_receipts(owner)?
            .into_iter()
            .find(|v| v.command == command.command);
        let binding = p::canonical_digest(&(owner, command))?;
        if previous.as_ref().is_some_and(|v| v.binding != binding) {
            return Err(p::Error("workbench command identity conflict".into()));
        }
        Ok(previous)
    }
}
