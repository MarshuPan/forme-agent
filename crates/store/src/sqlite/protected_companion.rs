//! Atomic append of an encrypted command companion and its authority event.
use super::*;
use crate::ProtectedContentRecord;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableControlResolution {
    schema_version: p::SchemaVersion,
    owner_digest: p::SchemaDigest,
    run: p::RunId,
    command_digest: p::SchemaDigest,
    resolution_digest: p::SchemaDigest,
    target_digest: p::SchemaDigest,
    command: p::RunControl,
}

impl SqliteEventStore {
    pub fn append_control_resolution_claim(
        &self,
        event: p::Event,
        command: p::RunControl,
    ) -> p::Result<p::EventId> {
        self.require_authority_writer()?;
        crate::domain_claim::validate_resolution_claim(&event, true)?;
        command.validate()?;
        let binding = p::ControlResolutionBinding::from_event(&event)?
            .ok_or_else(|| p::Error("control resolution claim is missing".into()))?;
        match self.probe_domain_claim(&event)? {
            crate::DomainClaimProbe::Conflict(_) => {
                return Err(p::Error(
                    "control command conflicts with its original binding".into(),
                ))
            }
            crate::DomainClaimProbe::Committed(original) if original != event.run_id => {
                return Err(p::Error(
                    "control command is already bound to another run".into(),
                ))
            }
            crate::DomainClaimProbe::Unclaimed | crate::DomainClaimProbe::Committed(_) => {}
            crate::DomainClaimProbe::NotIdempotent => {
                return Err(p::Error(
                    "control resolution claim is not idempotent".into(),
                ))
            }
        }
        let protection = self
            .content_protection()?
            .ok_or_else(|| p::Error("control resolution protection unavailable".into()))?;
        let command_digest = command_digest(&command)?;
        let resolution_digest =
            p::canonical_digest(&(protection.owner_digest(), &event.run_id, &command))?;
        if binding.command_digest != command_digest
            || binding.resolution_digest != resolution_digest
        {
            return Err(p::Error("control resolution binding invalid".into()));
        }
        let durable = DurableControlResolution {
            schema_version: p::SchemaVersion(1),
            owner_digest: protection.owner_digest().clone(),
            run: event.run_id.clone(),
            command_digest,
            resolution_digest,
            target_digest: binding.target_digest,
            command,
        };
        let encoded = serde_json::to_vec(&durable)
            .map_err(|_| p::Error("control resolution encoding failed".into()))?;
        let object = control_resolution_object(&durable.owner_digest, &durable.command_digest);
        let subject = p::sha256_content_digest(durable.run.0.as_bytes());
        let (_, companion) =
            protection.seal_record_in_default_scope(&encoded, &object, &subject)?;
        self.append_with_protected_companion(event, companion)
    }

    pub fn read_control_resolution_claim(
        &self,
        run: &p::RunId,
        claim_event: &p::Event,
    ) -> p::Result<Option<p::RunControl>> {
        if &claim_event.run_id != run {
            return Ok(None);
        }
        let Some(binding) = p::ControlResolutionBinding::from_event(claim_event)? else {
            return Ok(None);
        };
        let protection = self
            .content_protection()?
            .ok_or_else(|| p::Error("control resolution protection unavailable".into()))?;
        let id = control_resolution_content_ref(protection.owner_digest(), &binding.command_digest);
        let Some(record) = self.protected_content_for_current_owner(&id)? else {
            return Ok(None);
        };
        if record.owner_digest != *protection.owner_digest()
            || record.scope != *protection.default_scope()
            || record.subject_digest != p::sha256_content_digest(run.0.as_bytes())
            || record.content_ref != id
        {
            return Ok(None);
        }
        let plaintext = protection
            .content_service()
            .decrypt(&record.envelope.binding(), &record.envelope)?;
        let durable: DurableControlResolution = match serde_json::from_slice(&plaintext) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let expected_resolution =
            p::canonical_digest(&(protection.owner_digest(), run, &durable.command))?;
        if durable.schema_version != p::SchemaVersion(1)
            || durable.owner_digest != record.owner_digest
            || durable.run != *run
            || durable.command_digest != binding.command_digest
            || durable.resolution_digest != binding.resolution_digest
            || durable.target_digest != binding.target_digest
            || expected_resolution != binding.resolution_digest
            || command_digest(&durable.command)? != binding.command_digest
            || p::ControlResolutionBinding::from_event(claim_event)? != Some(binding)
            || durable.command.validate().is_err()
        {
            return Ok(None);
        }
        Ok(Some(durable.command))
    }

    fn append_with_protected_companion(
        &self,
        event: p::Event,
        record: ProtectedContentRecord,
    ) -> p::Result<p::EventId> {
        self.require_authority_writer()?;
        crate::domain_claim::validate_resolution_claim(&event, true)?;
        let binding = p::ControlResolutionBinding::from_event(&event)?.ok_or_else(|| {
            p::Error("protected command companion requires a resolution claim".into())
        })?;
        let protection = self
            .content_protection()?
            .ok_or_else(|| p::Error("control resolution protection unavailable".into()))?;
        record.validate()?;
        let expected_content_ref =
            control_resolution_content_ref(protection.owner_digest(), &binding.command_digest);
        if record.owner_digest != *protection.owner_digest()
            || record.scope != *protection.default_scope()
            || record.subject_digest != p::sha256_content_digest(event.run_id.0.as_bytes())
            || record.content_ref != expected_content_ref
        {
            return Err(p::Error(
                "control resolution companion binding invalid".into(),
            ));
        }
        let plaintext = protection
            .content_service()
            .decrypt(&record.envelope.binding(), &record.envelope)?;
        let prepared = self.prepare_event(event)?;
        let _registration = if self.core.fts_enabled {
            Some(
                self.core
                    .upcaster_registration
                    .lock()
                    .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?,
            )
        } else {
            None
        };
        let fts_upcaster_identity = if self.core.fts_enabled {
            Some(
                self.core
                    .upcasters
                    .read()
                    .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?
                    .identity_bytes(),
            )
        } else {
            None
        };
        let fts_schema_identity = self
            .core
            .fts_enabled
            .then(|| schema_registry_identity(&self.core.current_schema));
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| p::Error("control resolution transaction unavailable".into()))?;
        self.require_authority_writer_in_transaction(&transaction)?;
        insert_companion(&transaction, &record, &plaintext, &protection)?;
        let outcome = append_prepared(
            &transaction,
            prepared,
            self.core.fts_enabled,
            fts_schema_identity.as_deref(),
            fts_upcaster_identity.as_deref(),
        )?;
        transaction
            .commit()
            .map_err(|_| p::Error("control resolution transaction commit failed".into()))?;
        Ok(outcome.event_id)
    }
}

fn command_digest(command: &p::RunControl) -> p::Result<p::SchemaDigest> {
    let key = match command {
        p::RunControl::ResolveToolOutcome { command, .. }
        | p::RunControl::ResolveHandoff { command, .. } => command,
        _ => return Err(p::Error("control resolution command is invalid".into())),
    };
    p::canonical_digest(&("control-resolution-v1", key))
}

fn control_resolution_object(owner: &p::SchemaDigest, command: &p::SchemaDigest) -> String {
    format!("run-control-resolution:{}:{}", owner.0, command.0)
}

fn control_resolution_content_ref(
    owner: &p::SchemaDigest,
    command: &p::SchemaDigest,
) -> p::ContentRef {
    let object = control_resolution_object(owner, command);
    p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(object.as_bytes()).0
    ))
}

fn insert_companion(
    transaction: &Transaction<'_>,
    record: &ProtectedContentRecord,
    plaintext: &[u8],
    protection: &crate::StoreContentProtection,
) -> p::Result<()> {
    let blocked: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM data_lifecycle_subjects
             WHERE scope=?1 AND subject_digest=?2 AND state IN ('pending','deleted')",
            params![record.scope.0, record.subject_digest.0],
            |row| row.get(0),
        )
        .map_err(|_| p::Error("control resolution lifecycle fence unavailable".into()))?;
    if blocked != 0 {
        return Err(p::Error("control resolution subject is unavailable".into()));
    }
    let erased: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM data_lifecycle_deleted_objects WHERE object_ref=?1",
            params![record.content_ref.0],
            |row| row.get(0),
        )
        .map_err(|_| p::Error("control resolution erase fence unavailable".into()))?;
    if erased != 0 {
        return Err(p::Error("control resolution object was erased".into()));
    }
    let existing: Option<(Vec<u8>, String)> = transaction
        .query_row(
            "SELECT record,digest FROM protected_content WHERE content_ref=?1",
            params![record.content_ref.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| p::Error("control resolution identity unavailable".into()))?;
    if let Some((encoded, digest)) = existing {
        let previous: ProtectedContentRecord = serde_json::from_slice(&encoded)
            .map_err(|_| p::Error("control resolution companion corrupt".into()))?;
        previous.validate()?;
        let previous_plaintext = protection
            .content_service()
            .decrypt(&previous.envelope.binding(), &previous.envelope)?;
        if p::canonical_digest(&previous)?.0 != digest
            || previous.content_ref != record.content_ref
            || previous.owner_digest != record.owner_digest
            || previous.scope != record.scope
            || previous.subject_digest != record.subject_digest
            || previous_plaintext != plaintext
        {
            return Err(p::Error("control resolution companion conflict".into()));
        }
        return Ok(());
    }
    let encoded = serde_json::to_vec(record)
        .map_err(|_| p::Error("control resolution companion encoding failed".into()))?;
    let digest = p::canonical_digest(record)?.0;
    transaction
        .execute(
            "INSERT INTO protected_content
             (content_ref,owner_digest,scope,subject_digest,key_digest,record,digest)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                record.content_ref.0,
                record.owner_digest.0,
                record.scope.0,
                record.subject_digest.0,
                record.envelope.key_digest.0,
                encoded,
                digest,
            ],
        )
        .map_err(|_| p::Error("control resolution companion insert failed".into()))?;
    transaction
        .execute(
            "INSERT INTO protected_rows
             (object_ref,owner_digest,scope,subject_digest,key_digest,table_name,row_key)
             VALUES(?1,?2,?3,?4,?5,'protected_content',?1)",
            params![
                record.content_ref.0,
                record.owner_digest.0,
                record.scope.0,
                record.subject_digest.0,
                record.envelope.key_digest.0,
            ],
        )
        .map_err(|_| p::Error("control resolution catalog insert failed".into()))?;
    transaction
        .execute(
            "INSERT INTO protected_key_history(object_ref,key_digest) VALUES(?1,?2)",
            params![record.content_ref.0, record.envelope.key_digest.0],
        )
        .map_err(|_| p::Error("control resolution key history insert failed".into()))?;
    Ok(())
}
