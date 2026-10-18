//! Recoverable owner-controlled cleanup. Key deletion is external to SQLite:
//! a durable pending plan precedes it; a success event follows verification.
#![forbid(unsafe_code)]
use super::{append_prepared, schema_registry_identity};
use crate::{
    BrainAuthorityStore, ContentKeyResolver, EventStore, ProtectedContentService, SqliteEventStore,
};
use forme_protocol as p;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};

pub const DATA_LIFECYCLE_STORE_SCHEMA_VERSION: p::SchemaVersion = p::SchemaVersion(1);
pub const PROTECTED_CONTENT_PROJECTIONS: [&str; 6] =
    ["content", "fts", "summary", "vector", "graph", "candidate"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedContentRecord {
    pub schema_version: p::SchemaVersion,
    pub content_ref: p::ContentRef,
    pub owner_digest: p::SchemaDigest,
    pub scope: p::Scope,
    pub subject_digest: p::SchemaDigest,
    pub envelope: p::EncryptedContentEnvelope,
}
impl ProtectedContentRecord {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != DATA_LIFECYCLE_STORE_SCHEMA_VERSION
            || self.content_ref.0 != self.envelope.object.0
            || self.owner_digest != self.envelope.owner_digest
            || self.scope != self.envelope.scope
        {
            return Err(p::Error("protected content binding is invalid".into()));
        }
        p::sha256_digest_bytes(&self.subject_digest)?;
        self.envelope.validate()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataLifecycleRequest {
    pub schema_version: p::SchemaVersion,
    pub owner: p::VerifiedPrincipal,
    pub scope: p::Scope,
    pub subject_digest: p::SchemaDigest,
    pub operation: p::DataLifecycleOperation,
    pub expected_version: u64,
    pub event_id: p::EventId,
    pub timestamp: p::Timestamp,
    pub remote_disposition: p::RemoteDeletionDisposition,
    pub evidence: Vec<p::EvidenceRef>,
}
impl DataLifecycleRequest {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::V1_CLOSURE_SCHEMA_VERSION
            || self.owner.0.is_empty()
            || self.scope.0.is_empty()
            || self.event_id.0.is_empty()
            || self.timestamp < 0
            || self.expected_version == u64::MAX
            || self.remote_disposition == p::RemoteDeletionDisposition::Verified
        {
            return Err(p::Error(
                "data lifecycle command is incomplete or claims unverified remote deletion".into(),
            ));
        }
        p::sha256_digest_bytes(&self.subject_digest)?;
        Ok(())
    }
    pub fn owner_digest(&self) -> p::SchemaDigest {
        p::sha256_content_digest(self.owner.0.as_bytes())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleObject {
    pub object: p::ObjectRef,
    pub key_digest: p::SchemaDigest,
    pub table: String,
    pub row_key: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataLifecyclePlan {
    pub schema_version: p::SchemaVersion,
    pub owner_digest: p::SchemaDigest,
    pub aggregate: p::RunId,
    pub scope: p::Scope,
    pub subject_digest: p::SchemaDigest,
    pub operation: p::DataLifecycleOperation,
    pub expected_version: u64,
    pub objects: Vec<LifecycleObject>,
    pub digest: p::SchemaDigest,
}
impl DataLifecyclePlan {
    fn finish(mut self) -> p::Result<Self> {
        self.digest = p::SchemaDigest(String::new());
        self.digest = p::canonical_digest(&self)?;
        Ok(self)
    }
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != DATA_LIFECYCLE_STORE_SCHEMA_VERSION
            || self.objects.is_empty()
            || self.clone().finish()?.digest != self.digest
        {
            Err(p::Error("data lifecycle plan is invalid".into()))
        } else {
            Ok(())
        }
    }
}
#[derive(Clone)]
pub struct DataLifecycleService {
    store: SqliteEventStore,
    content: ProtectedContentService,
}
impl DataLifecycleService {
    pub fn new(store: SqliteEventStore, keys: Arc<dyn ContentKeyResolver>) -> Self {
        Self {
            store,
            content: ProtectedContentService::new(keys),
        }
    }
    pub fn store(&self) -> &SqliteEventStore {
        &self.store
    }
    fn authority(&self, r: &DataLifecycleRequest) -> p::Result<p::RunId> {
        r.validate()?;
        self.store.require_authority_writer()?;
        let a = self
            .store
            .brain_authority()?
            .ok_or_else(|| p::Error("lifecycle owner authority missing".into()))?;
        if a.owner_digest != r.owner_digest() {
            return Err(p::Error("lifecycle owner mismatch".into()));
        }
        a.aggregates
            .get(&crate::BrainAggregateKind::DataLifecycle)
            .cloned()
            .ok_or_else(|| p::Error("lifecycle aggregate missing".into()))
    }
    pub fn plan(&self, r: &DataLifecycleRequest) -> p::Result<DataLifecyclePlan> {
        let aggregate = self.authority(r)?;
        let c = self
            .store
            .core_connection()
            .map_err(|_| p::Error("lifecycle store unavailable".into()))?;
        if let Some((_, plan)) = read_job(&c, &aggregate)? {
            check_plan_request(&plan, r, &plan.digest)?;
            return Ok(plan);
        }
        preview(&c, r, aggregate)
    }
    /// Recheck the exact approved cleanup before consuming an approval nonce.
    /// apply_plan repeats this check under the writer transaction.
    pub fn validate_approved_plan(
        &self,
        r: &DataLifecycleRequest,
        approved: &p::SchemaDigest,
    ) -> p::Result<()> {
        let aggregate = self.authority(r)?;
        let c = self
            .store
            .core_connection()
            .map_err(|_| p::Error("lifecycle store unavailable".into()))?;
        if let Some(receipt) = receipt_for_event(&c, &r.event_id)? {
            return check_receipt(&c, &receipt, r, Some(approved));
        }
        let plan = match read_job(&c, &aggregate)? {
            Some((id, plan)) if id == r.event_id.0 => plan,
            Some(_) => return Err(p::Error("another lifecycle operation is pending".into())),
            None => preview(&c, r, aggregate)?,
        };
        check_plan_request(&plan, r, approved)
    }
    pub fn apply(&self, r: DataLifecycleRequest) -> p::Result<p::DataLifecycleReceipt> {
        let a = self.authority(&r)?;
        let pending = {
            let c = self
                .store
                .core_connection()
                .map_err(|_| p::Error("lifecycle store unavailable".into()))?;
            if let Some(receipt) = receipt_for_event(&c, &r.event_id)? {
                check_receipt(&c, &receipt, &r, None)?;
                return Ok(receipt);
            }
            read_job(&c, &a)?
        };
        let plan = match pending {
            Some((id, p)) if id == r.event_id.0 => p,
            Some(_) => return Err(p::Error("another lifecycle operation is pending".into())),
            None => self.plan(&r)?,
        };
        self.apply_plan(r, &plan.digest)
    }
    pub fn apply_plan(
        &self,
        r: DataLifecycleRequest,
        approved: &p::SchemaDigest,
    ) -> p::Result<p::DataLifecycleReceipt> {
        let aggregate = self.authority(&r)?;
        let plan = {
            let mut c = self
                .store
                .core_connection()
                .map_err(|_| p::Error("lifecycle store unavailable".into()))?;
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| p::Error("lifecycle transaction failed".into()))?;
            self.store.require_authority_writer_in_transaction(&tx)?;
            if let Some(receipt) = receipt_for_event(&tx, &r.event_id)? {
                check_receipt(&tx, &receipt, &r, Some(approved))?;
                return Ok(receipt);
            }
            let plan = match read_job(&tx, &aggregate)? {
                Some((id, p)) if id == r.event_id.0 => p,
                Some(_) => return Err(p::Error("another lifecycle operation is pending".into())),
                None => preview(&tx, &r, aggregate.clone())?,
            };
            check_plan_request(&plan, &r, approved)?;
            let json = serde_json::to_string(&plan)
                .map_err(|_| p::Error("lifecycle plan encoding failed".into()))?;
            tx.execute("INSERT INTO data_lifecycle_jobs(aggregate,event_id,plan,state) VALUES(?1,?2,?3,'pending') ON CONFLICT(aggregate) DO NOTHING",params![aggregate.0,r.event_id.0,json]).map_err(|_|p::Error("lifecycle recovery job could not be recorded".into()))?;
            if r.operation != p::DataLifecycleOperation::Retain {
                tx.execute("INSERT INTO data_lifecycle_subjects(scope,subject_digest,state) VALUES(?1,?2,'pending') ON CONFLICT(scope,subject_digest) DO UPDATE SET state='pending'",params![r.scope.0,r.subject_digest.0]).map_err(|_|p::Error("lifecycle subject fence could not be installed".into()))?;
                for object in &plan.objects {
                    clean_object(&tx, object)?;
                }
            }
            tx.commit()
                .map_err(|_| p::Error("lifecycle cleanup commit is unconfirmed".into()))?;
            plan
        };
        let deleting = r.operation != p::DataLifecycleOperation::Retain;
        let keys = if deleting {
            plan.objects
                .iter()
                .map(|o| o.key_digest.0.clone())
                .collect::<BTreeSet<_>>()
        } else {
            BTreeSet::new()
        };
        for key in &keys {
            let key = p::SchemaDigest(key.clone());
            self.content.destroy_key(&key).map_err(|_|p::Error("lifecycle remains pending: key destruction is unconfirmed; resume the same approved operation".into()))?;
            if self.content.ensure_key_available(&key).is_ok() {
                return Err(p::Error(
                    "lifecycle remains pending: content key is still available".into(),
                ));
            }
        }
        let projections = if deleting {
            plan.objects
                .iter()
                .map(|o| projection_name(&o.table).to_owned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(p::ProjectionRef)
                .collect()
        } else {
            Vec::new()
        };
        let mut evidence = r.evidence.clone();
        evidence.push(p::EvidenceRef(format!("lifecycle-plan:{}", approved.0)));
        let receipt = p::DataLifecycleReceipt {
            schema_version: p::V1_CLOSURE_SCHEMA_VERSION,
            aggregate: aggregate.clone(),
            operation: r.operation,
            scope: r.scope.clone(),
            subject_digest: r.subject_digest.clone(),
            cleaned_projections: projections,
            destroyed_key_digests: keys.into_iter().map(p::SchemaDigest).collect(),
            remote_disposition: r.remote_disposition,
            evidence,
        };
        receipt.validate()?;
        let event = p::Event::new(
            r.event_id.clone(),
            aggregate.clone(),
            None,
            p::EventPayload::DataLifecycleApplied(p::DataLifecycleAppliedPayload {
                receipt: receipt.clone(),
                expected_version: r.expected_version,
                committed_version: r.expected_version + 1,
            }),
            p::SchemaVersion(1),
            r.timestamp,
            p::Provenance {
                source: p::Source::OwnerControl,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
        );
        let prepared = self.store.prepare_event(event)?;
        let _registration = self
            .store
            .core
            .upcaster_registration
            .lock()
            .map_err(|_| p::Error("lifecycle upcaster lock failed".into()))?;
        let upcasters = self
            .store
            .core
            .upcasters
            .read()
            .map_err(|_| p::Error("lifecycle schema lock failed".into()))?
            .identity_bytes();
        let schemas = schema_registry_identity(&self.store.core.current_schema);
        let mut c = self
            .store
            .core_connection()
            .map_err(|_| p::Error("lifecycle store unavailable".into()))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| p::Error("lifecycle completion transaction failed".into()))?;
        self.store.require_authority_writer_in_transaction(&tx)?;
        if let Some(existing) = receipt_for_event(&tx, &r.event_id)? {
            check_receipt(&tx, &existing, &r, Some(approved))?;
            return Ok(existing);
        }
        tx.execute(
            "UPDATE data_lifecycle_jobs SET state='verified' WHERE aggregate=?1 AND event_id=?2",
            params![aggregate.0, r.event_id.0],
        )
        .map_err(|_| p::Error("lifecycle completion fence failed".into()))?;
        append_prepared(
            &tx,
            prepared,
            self.store.core.fts_enabled,
            Some(&schemas),
            Some(&upcasters),
        )?;
        if deleting {
            tx.execute("UPDATE data_lifecycle_subjects SET state='deleted' WHERE scope=?1 AND subject_digest=?2",params![r.scope.0,r.subject_digest.0]).map_err(|_|p::Error("lifecycle subject completion failed".into()))?;
        }
        tx.execute(
            "DELETE FROM data_lifecycle_jobs WHERE aggregate=?1 AND event_id=?2",
            params![aggregate.0, r.event_id.0],
        )
        .map_err(|_| p::Error("lifecycle job completion failed".into()))?;
        tx.commit().map_err(|_| {
            p::Error("lifecycle completion is unconfirmed; consult its tombstone".into())
        })?;
        Ok(receipt)
    }
    pub fn tombstone(&self) -> p::Result<Option<p::DataLifecycleReceipt>> {
        let c = self
            .store
            .core_connection()
            .map_err(|_| p::Error("lifecycle store unavailable".into()))?;
        let row: Option<String> = c
            .query_row(
                "SELECT tombstone FROM data_lifecycle_state ORDER BY version DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| p::Error("tombstone read failed".into()))?;
        row.map(|s| serde_json::from_str(&s).map_err(|_| p::Error("tombstone corrupt".into())))
            .transpose()
    }
    pub fn rebuild_tombstone(&self) -> p::Result<()> {
        self.store.require_authority_writer()?;
        let a = self
            .store
            .brain_authority()?
            .ok_or_else(|| p::Error("lifecycle authority missing".into()))?;
        let a = a
            .aggregates
            .get(&crate::BrainAggregateKind::DataLifecycle)
            .ok_or_else(|| p::Error("lifecycle aggregate missing".into()))?;
        let events = self
            .store
            .read_run(a.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let mut c = self
            .store
            .core_connection()
            .map_err(|_| p::Error("lifecycle store unavailable".into()))?;
        let tx = c
            .transaction()
            .map_err(|_| p::Error("tombstone rebuild failed".into()))?;
        tx.execute_batch("DELETE FROM data_lifecycle_state; DELETE FROM data_lifecycle_history;")
            .map_err(|_| p::Error("tombstone reset failed".into()))?;
        for e in events {
            fold(&tx, &e)?;
        }
        tx.commit()
            .map_err(|_| p::Error("tombstone rebuild commit failed".into()))
    }
    pub fn put_protected_content(
        &self,
        owner: &p::VerifiedPrincipal,
        record: ProtectedContentRecord,
    ) -> p::Result<()> {
        record.validate()?;
        self.store.require_authority_writer()?;
        let a = self
            .store
            .brain_authority()?
            .ok_or_else(|| p::Error("content authority missing".into()))?;
        if p::sha256_content_digest(owner.0.as_bytes()) != record.owner_digest
            || a.owner_digest != record.owner_digest
        {
            return Err(p::Error("protected content owner mismatch".into()));
        }
        self.content
            .decrypt(&record.envelope.binding(), &record.envelope)?;
        let encoded =
            serde_json::to_vec(&record).map_err(|_| p::Error("content encoding failed".into()))?;
        let mut c = self
            .store
            .core_connection()
            .map_err(|_| p::Error("content store unavailable".into()))?;
        let tx = c
            .transaction()
            .map_err(|_| p::Error("content transaction failed".into()))?;
        self.store.require_authority_writer_in_transaction(&tx)?;
        let blocked: i64 = tx.query_row("SELECT COUNT(*) FROM data_lifecycle_subjects WHERE scope=?1 AND subject_digest=?2 AND state IN ('pending','deleted')", params![record.scope.0,record.subject_digest.0], |r|r.get(0)).map_err(|_|p::Error("protected content lifecycle fence unavailable".into()))?;
        if blocked != 0 {
            return Err(p::Error(
                "protected content subject has been deleted".into(),
            ));
        }
        let erased: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM data_lifecycle_deleted_objects WHERE object_ref=?1",
                params![record.content_ref.0],
                |r| r.get(0),
            )
            .map_err(|_| p::Error("content erasure fence unavailable".into()))?;
        if erased != 0 {
            return Err(p::Error("protected object was erased".into()));
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT digest FROM protected_content WHERE content_ref=?1",
                params![record.content_ref.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| p::Error("content identity check unavailable".into()))?;
        if let Some(digest) = existing {
            return if digest == p::canonical_digest(&record)?.0 {
                Ok(())
            } else {
                Err(p::Error("protected content identity conflict".into()))
            };
        }
        tx.execute("INSERT INTO protected_content(content_ref,owner_digest,scope,subject_digest,key_digest,record,digest) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![record.content_ref.0,record.owner_digest.0,record.scope.0,record.subject_digest.0,record.envelope.key_digest.0,encoded,p::canonical_digest(&record)?.0]).map_err(|_|p::Error("protected content record conflict".into()))?;
        tx.execute("INSERT INTO protected_rows(object_ref,owner_digest,scope,subject_digest,key_digest,table_name,row_key) VALUES(?1,?2,?3,?4,?5,'protected_content',?1)",params![record.content_ref.0,record.owner_digest.0,record.scope.0,record.subject_digest.0,record.envelope.key_digest.0]).map_err(|_|p::Error("external content catalog failed".into()))?;
        tx.execute(
            "INSERT INTO protected_key_history(object_ref,key_digest) VALUES(?1,?2)",
            params![record.content_ref.0, record.envelope.key_digest.0],
        )
        .map_err(|_| p::Error("external content key history failed".into()))?;
        tx.commit()
            .map_err(|_| p::Error("content commit failed".into()))
    }
    pub fn protected_content(
        &self,
        owner: &p::VerifiedPrincipal,
        id: &p::ContentRef,
    ) -> p::Result<Option<ProtectedContentRecord>> {
        let c = self
            .store
            .core_connection()
            .map_err(|_| p::Error("content store unavailable".into()))?;
        let row: Option<Vec<u8>> = c
            .query_row(
                "SELECT record FROM protected_content WHERE content_ref=?1 AND owner_digest=?2",
                params![id.0, p::sha256_content_digest(owner.0.as_bytes()).0],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| p::Error("content read failed".into()))?;
        row.map(|b| {
            let r: ProtectedContentRecord = serde_json::from_slice(&b)
                .map_err(|_| p::Error("protected content corrupt".into()))?;
            r.validate()?;
            Ok(r)
        })
        .transpose()
    }
}
impl SqliteEventStore {
    pub fn has_pending_data_cleanup(&self) -> p::Result<bool> {
        let c = self
            .core_connection()
            .map_err(|_| p::Error("lifecycle store unavailable".into()))?;
        c.query_row("SELECT COUNT(*) FROM data_lifecycle_jobs", [], |r| {
            r.get::<_, i64>(0)
        })
        .map(|n| n != 0)
        .map_err(|_| p::Error("lifecycle recovery fence unavailable".into()))
    }
    pub fn data_lifecycle_service(
        &self,
        keys: Arc<dyn ContentKeyResolver>,
    ) -> DataLifecycleService {
        DataLifecycleService::new(self.clone(), keys)
    }
}

fn lifecycle_version(c: &Connection, a: &p::RunId) -> p::Result<u64> {
    let v: i64 = c
        .query_row(
            "SELECT version FROM data_lifecycle_state WHERE aggregate=?1",
            params![a.0],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| p::Error("lifecycle version read failed".into()))?
        .unwrap_or(0);
    u64::try_from(v).map_err(|_| p::Error("lifecycle version invalid".into()))
}
fn check_plan_request(
    plan: &DataLifecyclePlan,
    r: &DataLifecycleRequest,
    approved: &p::SchemaDigest,
) -> p::Result<()> {
    plan.validate()?;
    if &plan.digest != approved
        || plan.owner_digest != r.owner_digest()
        || plan.scope != r.scope
        || plan.subject_digest != r.subject_digest
        || plan.operation != r.operation
        || plan.expected_version != r.expected_version
    {
        return Err(p::Error("lifecycle plan changed after approval".into()));
    }
    Ok(())
}
fn check_receipt(
    c: &Connection,
    receipt: &p::DataLifecycleReceipt,
    r: &DataLifecycleRequest,
    approved: Option<&p::SchemaDigest>,
) -> p::Result<()> {
    let version: i64 = c
        .query_row(
            "SELECT version FROM data_lifecycle_history WHERE event_id=?1",
            params![r.event_id.0],
            |row| row.get(0),
        )
        .map_err(|_| p::Error("lifecycle receipt version unavailable".into()))?;
    if receipt.scope != r.scope
        || receipt.subject_digest != r.subject_digest
        || receipt.operation != r.operation
        || u64::try_from(version).ok() != r.expected_version.checked_add(1)
        || approved.is_some_and(|digest| {
            !receipt
                .evidence
                .contains(&p::EvidenceRef(format!("lifecycle-plan:{}", digest.0)))
        })
    {
        return Err(p::Error(
            "lifecycle event identity was reused for a different command".into(),
        ));
    }
    Ok(())
}
fn preview(
    c: &Connection,
    r: &DataLifecycleRequest,
    aggregate: p::RunId,
) -> p::Result<DataLifecyclePlan> {
    if lifecycle_version(c, &aggregate)? != r.expected_version {
        return Err(p::Error("lifecycle expected version conflict".into()));
    }
    let legacy: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM events WHERE substr(payload,1,?1)!=?2",
            params![crate::at_rest::PREFIX.len() as i64, crate::at_rest::PREFIX],
            |r| r.get(0),
        )
        .map_err(|_| p::Error("historical protection inspection failed".into()))?;
    if legacy != 0 {
        return Err(p::Error("historical recoverable plaintext blocks deletion; explicit verified migration is required".into()));
    }
    let mut s=c.prepare("WITH RECURSIVE affected(ref) AS (SELECT object_ref FROM protected_rows WHERE owner_digest=?1 AND scope=?2 AND subject_digest=?3 UNION SELECT d.object_ref FROM protected_dependencies d JOIN affected a ON d.source_object=a.ref) SELECT r.object_ref,h.key_digest,r.table_name,r.row_key FROM protected_rows r JOIN protected_key_history h ON r.object_ref=h.object_ref WHERE r.object_ref IN(SELECT ref FROM affected) AND r.object_ref NOT IN(SELECT object_ref FROM data_lifecycle_deleted_objects) ORDER BY 1,2").map_err(|_|p::Error("lifecycle target query failed".into()))?;
    let objects = s
        .query_map(
            params![r.owner_digest().0, r.scope.0, r.subject_digest.0],
            |r| {
                Ok(LifecycleObject {
                    object: p::ObjectRef(r.get(0)?),
                    key_digest: p::SchemaDigest(r.get(1)?),
                    table: r.get(2)?,
                    row_key: r.get(3)?,
                })
            },
        )
        .map_err(|_| p::Error("lifecycle targets unavailable".into()))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| p::Error("lifecycle target corrupt".into()))?;
    let plan = DataLifecyclePlan {
        schema_version: DATA_LIFECYCLE_STORE_SCHEMA_VERSION,
        owner_digest: r.owner_digest(),
        aggregate,
        scope: r.scope.clone(),
        subject_digest: r.subject_digest.clone(),
        operation: r.operation,
        expected_version: r.expected_version,
        objects,
        digest: p::SchemaDigest(String::new()),
    }
    .finish()?;
    plan.validate()?;
    for o in &plan.objects {
        let outside:i64=c.query_row("SELECT COUNT(*) FROM protected_rows WHERE object_ref=?1 AND (owner_digest!=?2 OR scope!=?3)",params![o.object.0,r.owner_digest().0,r.scope.0],|r|r.get(0)).map_err(|_|p::Error("lifecycle key isolation inspection failed".into()))?;
        if outside != 0 {
            return Err(p::Error(
                "lifecycle key is shared outside approved subject".into(),
            ));
        }
    }
    Ok(plan)
}
fn read_job(c: &Connection, a: &p::RunId) -> p::Result<Option<(String, DataLifecyclePlan)>> {
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT event_id,plan FROM data_lifecycle_jobs WHERE aggregate=?1",
            params![a.0],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|_| p::Error("lifecycle recovery read failed".into()))?;
    row.map(|(id, json)| {
        let plan: DataLifecyclePlan = serde_json::from_str(&json)
            .map_err(|_| p::Error("lifecycle recovery plan corrupt".into()))?;
        plan.validate()?;
        Ok((id, plan))
    })
    .transpose()
}
fn clean_object(tx: &Transaction<'_>, o: &LifecycleObject) -> p::Result<()> {
    let sql = match o.table.as_str() {
        "events" => None,
        "session_state" => Some("DELETE FROM session_state WHERE run_id=?1"),
        "transcript" => Some("DELETE FROM transcript WHERE run_id=?1"),
        "workspace_charter_state" => Some("DELETE FROM workspace_charter_state WHERE workspace=?1"),
        "orchestration_state" => Some("DELETE FROM orchestration_state WHERE stream=?1"),
        "protected_content" => Some("DELETE FROM protected_content WHERE content_ref=?1"),
        "stable_strategies" => Some("DELETE FROM stable_strategies WHERE candidate_id=?1"),
        "strategy_candidates" => Some("DELETE FROM strategy_candidates WHERE candidate_id=?1"),
        "evolution_evaluations" => {
            Some("DELETE FROM evolution_evaluations WHERE evaluation_ref=?1")
        }
        // Both rows contain the exact active snapshot. Their key is the
        // originating event, so cleanup cannot remove another activation.
        "active_strategies" => Some("DELETE FROM active_strategies WHERE activation_event=?1"),
        "evolution_history" => Some("DELETE FROM evolution_history WHERE event_id=?1"),
        _ => {
            if let Some((table, key, _, _, _)) = crate::at_rest::AUXILIARY_CONTENT
                .iter()
                .find(|(table, _, _, _, _)| *table == o.table)
            {
                tx.execute(
                    &format!("DELETE FROM {table} WHERE {key}=?1"),
                    params![o.row_key],
                )
                .map_err(|_| p::Error("auxiliary content cleanup failed".into()))?;
                None
            } else {
                return Err(p::Error(
                    "lifecycle projection requires a registered cleanup handler".into(),
                ));
            }
        }
    };
    if let Some(sql) = sql {
        tx.execute(sql, params![o.row_key])
            .map_err(|_| p::Error("lifecycle projection cleanup failed".into()))?;
    }
    tx.execute(
        "INSERT INTO data_lifecycle_deleted_objects(object_ref) VALUES(?1) ON CONFLICT DO NOTHING",
        params![o.object.0],
    )
    .map_err(|_| p::Error("lifecycle retrieval exclusion failed".into()))?;
    Ok(())
}
fn projection_name(table: &str) -> &str {
    match table {
        "events" | "protected_content" => "content",
        "transcript" => "summary",
        "strategy_candidates" | "stable_strategies" => "candidate",
        other => other,
    }
}
fn receipt_for_event(
    c: &Connection,
    id: &p::EventId,
) -> p::Result<Option<p::DataLifecycleReceipt>> {
    let row: Option<String> = c
        .query_row(
            "SELECT tombstone FROM data_lifecycle_history WHERE event_id=?1",
            params![id.0],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| p::Error("lifecycle receipt lookup failed".into()))?;
    row.map(|s| serde_json::from_str(&s).map_err(|_| p::Error("lifecycle receipt corrupt".into())))
        .transpose()
}
pub(crate) fn validate(tx: &Transaction<'_>, event: &p::Event) -> p::Result<()> {
    let p::EventPayload::DataLifecycleApplied(payload) = &event.payload else {
        return Ok(());
    };
    payload.validate()?;
    let exists:i64=tx.query_row("SELECT COUNT(*) FROM data_lifecycle_jobs WHERE aggregate=?1 AND event_id=?2 AND state='verified'",params![event.run_id.0,event.event_id.0],|r|r.get(0)).map_err(|_|p::Error("lifecycle authorization fence unavailable".into()))?;
    if exists != 1
        || event.provenance.source != p::Source::OwnerControl
        || event.provenance.actor != p::Actor::Owner
        || event.provenance.trust_tier != p::TrustTier::OwnerInput
        || payload.receipt.aggregate != event.run_id
        || lifecycle_version(tx, &event.run_id)? != payload.expected_version
    {
        return Err(p::Error(
            "lifecycle completion requires its verified owner cleanup job".into(),
        ));
    }
    Ok(())
}
pub(crate) fn fold(tx: &Transaction<'_>, event: &p::Event) -> p::Result<()> {
    let p::EventPayload::DataLifecycleApplied(payload) = &event.payload else {
        return Ok(());
    };
    if lifecycle_version(tx, &event.run_id)? != payload.expected_version {
        return Err(p::Error("lifecycle replay version discontinuity".into()));
    }
    let json = serde_json::to_string(&payload.receipt)
        .map_err(|_| p::Error("lifecycle tombstone encoding failed".into()))?;
    let digest = p::canonical_digest(&payload.receipt)?;
    let v = i64::try_from(payload.committed_version)
        .map_err(|_| p::Error("lifecycle version exhausted".into()))?;
    tx.execute("INSERT INTO data_lifecycle_state(aggregate,version,event_id,tombstone,digest) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(aggregate) DO UPDATE SET version=excluded.version,event_id=excluded.event_id,tombstone=excluded.tombstone,digest=excluded.digest",params![event.run_id.0,v,event.event_id.0,json,digest.0]).map_err(|_|p::Error("lifecycle tombstone fold failed".into()))?;
    tx.execute("INSERT INTO data_lifecycle_history(aggregate,version,event_id,tombstone,digest) VALUES(?1,?2,?3,?4,?5)",params![event.run_id.0,v,event.event_id.0,json,digest.0]).map_err(|_|p::Error("lifecycle history fold failed".into()))?;
    Ok(())
}
