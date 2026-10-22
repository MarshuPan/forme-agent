//! Immutable encrypted companions. Authoritative approvals/lifecycle remain events.
use super::*;

fn scope(phase: &str) -> p::Scope {
    p::Scope(format!("maintenance:data-workbench/{phase}"))
}
fn object(
    owner: &p::VerifiedPrincipal,
    command: &p::IdempotencyKey,
    phase: &str,
) -> p::Result<String> {
    Ok(format!(
        "data-workbench:{}:{phase}",
        p::canonical_digest(&(owner, command))?.0
    ))
}
fn content_ref(object: &str) -> p::ContentRef {
    p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(object.as_bytes()).0
    ))
}

impl ProductionRuntime {
    pub(super) fn write_data_record(
        &self,
        owner: &p::VerifiedPrincipal,
        phase: &str,
        record: &Record,
    ) -> p::Result<()> {
        let protection = self
            .services()
            .store
            .content_protection()?
            .ok_or_else(|| p::Error("data operation protection unavailable".into()))?;
        let service = protection.lifecycle_service(self.services().store.as_ref().clone());
        let object = object(owner, &record.request.command, phase)?;
        if let Some(previous) = self.read_data_record(owner, &content_ref(&object), phase)? {
            return if previous == *record {
                Ok(())
            } else {
                Err(p::Error("data operation record conflict".into()))
            };
        }
        let bytes = serde_json::to_vec(record)
            .map_err(|_| p::Error("data operation encoding failed".into()))?;
        let (_, sealed) =
            protection.seal_record(&bytes, &scope(phase), &object, &record.receipt.binding)?;
        service.put_protected_content(owner, sealed)
    }
    pub(super) fn data_record(
        &self,
        owner: &p::VerifiedPrincipal,
        command: &p::IdempotencyKey,
    ) -> p::Result<Option<Record>> {
        let Some(started) = self.read_data_record(
            owner,
            &content_ref(&object(owner, command, "started")?),
            "started",
        )?
        else {
            return Ok(None);
        };
        if started.request.command != *command
            || started.receipt.state != p::DataCommandState::Unknown
        {
            return Err(p::Error("data operation start binding invalid".into()));
        }
        let completed = self.read_data_record(
            owner,
            &content_ref(&object(owner, command, "confirmed")?),
            "confirmed",
        )?;
        if let Some(completed) = completed {
            if completed.request != started.request
                || completed.receipt.binding != started.receipt.binding
                || completed.receipt.state == p::DataCommandState::Unknown
            {
                return Err(p::Error("data operation receipt binding invalid".into()));
            }
            Ok(Some(completed))
        } else {
            Ok(Some(started))
        }
    }
    pub(super) fn data_started_records(
        &self,
        owner: &p::VerifiedPrincipal,
    ) -> p::Result<Vec<Record>> {
        self.services()
            .store
            .protected_content_refs(owner, &scope("started"))?
            .iter()
            .map(|id| {
                self.read_data_record(owner, id, "started")?
                    .ok_or_else(|| p::Error("data operation record missing".into()))
            })
            .collect()
    }
    fn read_data_record(
        &self,
        owner: &p::VerifiedPrincipal,
        id: &p::ContentRef,
        phase: &str,
    ) -> p::Result<Option<Record>> {
        let protection = self
            .services()
            .store
            .content_protection()?
            .ok_or_else(|| p::Error("data operation protection unavailable".into()))?;
        let service = protection.lifecycle_service(self.services().store.as_ref().clone());
        let Some(record) = service.protected_content(owner, id)? else {
            return Ok(None);
        };
        if record.scope != scope(phase) {
            return Err(p::Error("data operation scope invalid".into()));
        }
        let bytes = protection
            .content_service()
            .decrypt(&record.envelope.binding(), &record.envelope)?;
        let value: Record = serde_json::from_slice(&bytes)
            .map_err(|_| p::Error("data operation record invalid".into()))?;
        value.request.validate()?;
        if value.schema_version != p::SchemaVersion(1)
            || value.receipt.schema_version != p::SchemaVersion(1)
            || value.receipt.command != value.request.command
            || value.receipt.binding != p::canonical_digest(&(owner, &value.request))?
            || *id != content_ref(&object(owner, &value.request.command, phase)?)
        {
            return Err(p::Error("data operation record binding invalid".into()));
        }
        Ok(Some(value))
    }
}
