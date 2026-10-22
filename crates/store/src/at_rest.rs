//! Application-layer protection of persisted payloads and derived objects.
//! Keys live in a separate local resolver; portable database backups contain
//! only ciphertext and safe binding digests.
#![forbid(unsafe_code)]

use crate::{ContentKeyResolver, ProtectedContentService};
use forme_protocol as p;
use rusqlite::{functions::FunctionFlags, params, Connection};
use std::sync::{Arc, RwLock};

pub(crate) const PREFIX: &[u8] = b"FORME-AEAD-1\n";
pub(crate) type ProtectionState = Arc<RwLock<Option<StoreContentProtection>>>;

#[derive(Clone)]
pub struct StoreContentProtection {
    owner: p::SchemaDigest,
    scope: p::Scope,
    keys: Arc<dyn ContentKeyResolver>,
    content: ProtectedContentService,
}

impl StoreContentProtection {
    pub(crate) fn owner_digest(&self) -> &p::SchemaDigest {
        &self.owner
    }
    pub(crate) fn default_scope(&self) -> &p::Scope {
        &self.scope
    }
    pub fn new(
        owner: &p::VerifiedPrincipal,
        scope: p::Scope,
        keys: Arc<dyn ContentKeyResolver>,
    ) -> p::Result<Self> {
        let owner = p::sha256_content_digest(owner.0.as_bytes());
        p::ContentBinding {
            schema_version: p::CONTENT_PROTECTION_SCHEMA_VERSION,
            owner_digest: owner.clone(),
            scope: scope.clone(),
            object: p::ObjectRef("content:validation".into()),
        }
        .validate()?;
        Ok(Self {
            owner,
            scope,
            content: ProtectedContentService::new(keys.clone()),
            keys,
        })
    }

    pub fn seal(
        &self,
        bytes: &[u8],
        scope: Option<&p::Scope>,
        object: &str,
        subject: &p::SchemaDigest,
    ) -> p::Result<Vec<u8>> {
        let binding = p::ContentBinding {
            schema_version: p::CONTENT_PROTECTION_SCHEMA_VERSION,
            owner_digest: self.owner.clone(),
            scope: scope.cloned().unwrap_or_else(|| self.scope.clone()),
            object: p::ObjectRef(format!(
                "content:{}",
                p::sha256_content_digest(object.as_bytes()).0
            )),
        };
        let generation = p::canonical_digest(&(subject, p::sha256_content_digest(bytes)))?;
        let key = self.keys.subject_key(&binding, &generation)?;
        let envelope = self.content.encrypt(&binding, &key, bytes)?;
        let mut result = PREFIX.to_vec();
        result.extend(
            serde_json::to_vec(&envelope)
                .map_err(|_| p::Error("protected object encoding failed".into()))?,
        );
        Ok(result)
    }

    pub fn open(&self, bytes: &[u8]) -> p::Result<Vec<u8>> {
        let envelope = envelope(bytes)?.ok_or_else(|| {
            p::Error("unprotected content cannot be read in protected mode".into())
        })?;
        if envelope.owner_digest != self.owner {
            return Err(p::Error("protected object owner mismatch".into()));
        }
        self.content.decrypt(&envelope.binding(), &envelope)
    }
    /// Registers an externally stored ciphertext through the same lifecycle
    /// catalog as event/projection content. No external plaintext is retained.
    pub fn seal_record(
        &self,
        bytes: &[u8],
        scope: &p::Scope,
        object: &str,
        subject: &p::SchemaDigest,
    ) -> p::Result<(Vec<u8>, crate::ProtectedContentRecord)> {
        let bytes = self.seal(bytes, Some(scope), object, subject)?;
        let envelope =
            envelope(&bytes)?.ok_or_else(|| p::Error("content envelope missing".into()))?;
        let record = crate::ProtectedContentRecord {
            schema_version: crate::DATA_LIFECYCLE_STORE_SCHEMA_VERSION,
            content_ref: p::ContentRef(envelope.object.0.clone()),
            owner_digest: self.owner.clone(),
            scope: scope.clone(),
            subject_digest: subject.clone(),
            envelope,
        };
        record.validate()?;
        Ok((bytes, record))
    }

    fn open_bound(&self, bytes: &[u8], object: &str) -> p::Result<Vec<u8>> {
        let envelope = envelope(bytes)?
            .ok_or_else(|| p::Error("protected object envelope is missing".into()))?;
        let expected = p::ObjectRef(format!(
            "content:{}",
            p::sha256_content_digest(object.as_bytes()).0
        ));
        if envelope.object != expected {
            return Err(p::Error("protected object row binding mismatch".into()));
        }
        self.open(bytes)
    }

    pub fn content_service(&self) -> ProtectedContentService {
        self.content.clone()
    }
    pub fn lifecycle_service(&self, store: crate::SqliteEventStore) -> crate::DataLifecycleService {
        crate::DataLifecycleService::new(store, self.keys.clone())
    }
}

pub(crate) fn envelope(bytes: &[u8]) -> p::Result<Option<p::EncryptedContentEnvelope>> {
    let Some(encoded) = bytes.strip_prefix(PREFIX) else {
        return Ok(None);
    };
    let value: p::EncryptedContentEnvelope = serde_json::from_slice(encoded)
        .map_err(|_| p::Error("protected object envelope is corrupt".into()))?;
    value.validate()?;
    Ok(Some(value))
}

pub(crate) fn install(connection: &Connection, state: ProtectionState) -> p::Result<()> {
    let enabled = state.clone();
    connection
        .create_scalar_function(
            "forme_protection_enabled",
            0,
            FunctionFlags::SQLITE_UTF8,
            move |_| {
                enabled
                    .read()
                    .map(|s| s.is_some())
                    .map_err(|_| sql_error("protection lock unavailable"))
            },
        )
        .map_err(|_| p::Error("protection state function unavailable".into()))?;
    connection
        .create_scalar_function("forme_digest", 1, FunctionFlags::SQLITE_UTF8, |ctx| {
            Ok(p::sha256_content_digest(ctx.get::<String>(0)?.as_bytes()).0)
        })
        .map_err(|_| p::Error("protection digest function unavailable".into()))?;
    connection
        .create_scalar_function(
            "forme_content_references",
            1,
            FunctionFlags::SQLITE_UTF8,
            |ctx| {
                let bytes = ctx.get::<Vec<u8>>(0)?;
                let value: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|_| sql_error("protected JSON content invalid"))?;
                let mut refs = std::collections::BTreeSet::new();
                collect_references(&value, &mut refs);
                serde_json::to_string(&refs)
                    .map_err(|_| sql_error("content references unavailable"))
            },
        )
        .map_err(|_| p::Error("content reference function unavailable".into()))?;
    let bound = state.clone();
    connection
        .create_scalar_function(
            "forme_open_bound",
            3,
            FunctionFlags::SQLITE_UTF8,
            move |ctx| {
                let bytes = ctx.get::<Vec<u8>>(0)?;
                let table = ctx.get::<String>(1)?;
                let key = ctx.get::<String>(2)?;
                match bound
                    .read()
                    .map_err(|_| sql_error("protection lock is poisoned"))?
                    .as_ref()
                {
                    Some(p) => p
                        .open_bound(&bytes, &format!("{table}:{key}"))
                        .map_err(|_| {
                            sql_error("protected object row binding or authentication failed")
                        }),
                    None if bytes.starts_with(PREFIX) => Err(sql_error(
                        "protected object requires its configured resolver",
                    )),
                    None => Ok(bytes),
                }
            },
        )
        .map_err(|_| p::Error("bound content read function could not be installed".into()))?;
    let read = state.clone();
    connection
        .create_scalar_function("forme_open", 1, FunctionFlags::SQLITE_UTF8, move |ctx| {
            let bytes = ctx.get::<Vec<u8>>(0)?;
            let protection = read
                .read()
                .map_err(|_| sql_error("protection lock is poisoned"))?;
            match protection.as_ref() {
                Some(service) => service
                    .open(&bytes)
                    .map_err(|_| sql_error("protected object cannot be decrypted")),
                None if bytes.starts_with(PREFIX) => Err(sql_error(
                    "protected object requires its configured resolver",
                )),
                None => Ok(bytes),
            }
        })
        .map_err(|_| p::Error("content read function could not be installed".into()))?;
    connection
        .create_scalar_function("forme_seal", 4, FunctionFlags::SQLITE_UTF8, move |ctx| {
            let bytes = ctx.get::<Vec<u8>>(0)?;
            let scope = ctx.get::<Option<String>>(1)?.map(p::Scope);
            let object = ctx.get::<String>(2)?;
            let subject = p::SchemaDigest(ctx.get::<String>(3)?);
            let protection = state
                .read()
                .map_err(|_| sql_error("protection lock is poisoned"))?;
            match protection.as_ref() {
                Some(service) => service
                    .seal(&bytes, scope.as_ref(), &object, &subject)
                    .map_err(|_| sql_error("protected object cannot be encrypted")),
                None => Ok(bytes),
            }
        })
        .map_err(|_| p::Error("content write function could not be installed".into()))
}

// Companion protection for pre-existing M3/M4/M5 object ledgers. TEMP
// triggers belong only to the authorized writer connection, run inside its
// transaction, and never manufacture domain events or mutable metrics.
// Each public reader still authenticates/decrypts the resulting envelope.
pub(crate) const AUXILIARY_CONTENT: &[(&str, &str, &str, &str, Option<&str>)] = &[
    (
        "sync_peers",
        "singleton",
        "semantic_bytes",
        "'sync-peer'",
        None,
    ),
    (
        "sync_batches",
        "batch_id",
        "semantic_bytes",
        "NEW.aggregate",
        None,
    ),
    (
        "federated_peers",
        "peer_ref",
        "grant",
        "COALESCE((SELECT run_id FROM events WHERE event_id=NEW.last_event_id),NEW.peer_ref)",
        None,
    ),
    (
        "remote_leases",
        "lease_ref",
        "lease",
        "COALESCE((SELECT run_id FROM events WHERE event_id=NEW.last_event_id),NEW.lease_ref)",
        None,
    ),
    (
        "replication_exports",
        "batch_ref",
        "semantic_bytes",
        "NEW.aggregate",
        None,
    ),
    (
        "federated_checkpoint_artifacts",
        "checkpoint_ref",
        "artifact",
        "NEW.source_run",
        None,
    ),
    (
        "federated_handoffs",
        "next_run",
        "plan",
        "NEW.next_run",
        None,
    ),
    (
        "federated_retention",
        "request_ref",
        "state",
        "NEW.request_ref",
        None,
    ),
    (
        "remote_action_recovery",
        "run_id",
        "record",
        "NEW.run_id",
        None,
    ),
    (
        "ecosystem_publishers",
        "publisher_ref",
        "grant",
        "COALESCE((SELECT run_id FROM events WHERE event_id=NEW.last_event_id),NEW.publisher_ref)",
        Some("NEW.scope"),
    ),
    (
        "ecosystem_admissions",
        "release_ref",
        "admission",
        "COALESCE((SELECT run_id FROM events WHERE event_id=NEW.last_event_id),NEW.release_ref)",
        Some("NEW.scope"),
    ),
    (
        "ecosystem_package_states",
        "package_ref",
        "state",
        "COALESCE((SELECT run_id FROM events WHERE event_id=NEW.last_event_id),NEW.package_ref)",
        Some("NEW.scope"),
    ),
    (
        "ecosystem_distributions",
        "receipt_ref",
        "receipt",
        "COALESCE((SELECT run_id FROM events WHERE event_id=NEW.last_event_id),NEW.receipt_ref)",
        Some("NEW.scope"),
    ),
    (
        "ecosystem_distribution_attempts",
        "receipt_ref",
        "semantic",
        "NEW.receipt_ref",
        None,
    ),
    (
        "ecosystem_package_bundles",
        "release_ref",
        "bundle",
        "NEW.release_ref",
        None,
    ),
];
pub(crate) fn install_auxiliary_protection(c: &Connection) -> p::Result<()> {
    let n = PREFIX.len();
    for (table, key, column, subject, scope) in AUXILIARY_CONTENT {
        let scope = scope.unwrap_or("NULL");
        for operation in ["INSERT", "UPDATE"] {
            let sql=format!("CREATE TEMP TRIGGER IF NOT EXISTS protect_{table}_{operation} AFTER {operation} ON main.{table}
WHEN forme_protection_enabled() AND substr(NEW.{column},1,{n})!=X'464F524D452D414541442D310A'
BEGIN
  SELECT CASE WHEN EXISTS(SELECT 1 FROM data_lifecycle_subjects WHERE subject_digest=forme_digest({subject}) AND state IN ('pending','deleted')) THEN RAISE(ABORT,'content subject deleted') END;
  UPDATE {table} SET {column}=forme_seal(NEW.{column},{scope},'{table}:'||NEW.{key},forme_digest({subject})) WHERE {key}=NEW.{key};
  INSERT INTO protected_rows(object_ref,owner_digest,scope,subject_digest,key_digest,table_name,row_key)
  SELECT json_extract(substr({column},{n}+1),'$.object'),json_extract(substr({column},{n}+1),'$.owner_digest'),json_extract(substr({column},{n}+1),'$.scope'),forme_digest({subject}),json_extract(substr({column},{n}+1),'$.key_digest'),'{table}',CAST(NEW.{key} AS TEXT) FROM {table} WHERE {key}=NEW.{key}
  ON CONFLICT(object_ref) DO UPDATE SET key_digest=excluded.key_digest;
  INSERT OR IGNORE INTO protected_key_history(object_ref,key_digest) SELECT object_ref,key_digest FROM protected_rows WHERE table_name='{table}' AND row_key=CAST(NEW.{key} AS TEXT);
  INSERT OR IGNORE INTO protected_dependencies(object_ref,source_object)
  SELECT target.object_ref,source.object_ref FROM protected_rows target,protected_rows source WHERE target.table_name='{table}' AND target.row_key=CAST(NEW.{key} AS TEXT) AND source.table_name='events' AND (source.subject_digest=forme_digest({subject}) OR source.row_key IN (SELECT value FROM json_each(forme_content_references(NEW.{column}))));
END;");
            c.execute_batch(&sql).map_err(|_| {
                p::Error("auxiliary content protection could not be installed".into())
            })?;
        }
    }
    Ok(())
}

fn sql_error(message: &str) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(Box::new(p::Error(message.into())))
}

pub(crate) fn open_bound_blob(
    connection: &Connection,
    bytes: &[u8],
    table: &str,
    key: &str,
) -> p::Result<Vec<u8>> {
    connection
        .query_row(
            "SELECT forme_open_bound(?1,?2,?3)",
            params![bytes, table, key],
            |row| row.get(0),
        )
        .map_err(|_| p::Error("persisted content cannot be decrypted".into()))
}

pub(crate) fn seal_blob(
    connection: &Connection,
    bytes: &[u8],
    scope: Option<&p::Scope>,
    table: &str,
    key: &str,
    subject: &str,
) -> p::Result<Vec<u8>> {
    let object = format!("{table}:{key}");
    let subject = p::sha256_content_digest(subject.as_bytes());
    let blocked:i64=connection.query_row("SELECT COUNT(*) FROM data_lifecycle_subjects WHERE subject_digest=?1 AND (?2 IS NULL OR scope=?2) AND state IN ('pending','deleted')",params![subject.0,scope.map(|s|&s.0)],|r|r.get(0)).map_err(|_|p::Error("content lifecycle fence unavailable".into()))?;
    if blocked != 0 {
        return Err(p::Error(
            "content subject is pending deletion or has been deleted".into(),
        ));
    }
    let sealed: Vec<u8> = connection
        .query_row(
            "SELECT forme_seal(?1,?2,?3,?4)",
            params![bytes, scope.map(|s| &s.0), &object, &subject.0],
            |row| row.get(0),
        )
        .map_err(|_| p::Error("persisted content cannot be encrypted".into()))?;
    if let Some(envelope) = envelope(&sealed)? {
        connection.execute("INSERT INTO protected_rows(object_ref,owner_digest,scope,subject_digest,key_digest,table_name,row_key) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(object_ref) DO UPDATE SET key_digest=excluded.key_digest", params![envelope.object.0,envelope.owner_digest.0,envelope.scope.0,subject.0,envelope.key_digest.0,table,key]).map_err(|_| p::Error("protected object catalog could not be recorded".into()))?;
        connection
            .execute(
                "INSERT OR IGNORE INTO protected_key_history(object_ref,key_digest) VALUES(?1,?2)",
                params![envelope.object.0, envelope.key_digest.0],
            )
            .map_err(|_| p::Error("protected key history could not be recorded".into()))?;
        let mut references = std::collections::BTreeSet::new();
        if table == "events" {
            // Later facts in one aggregate depend on its preceding history.
            // Deleting a source invalidates the dependent suffix, including
            // numeric graph-version deltas that carry no explicit evidence id.
            // One predecessor is enough for transitive suffix invalidation.
            // Linking every earlier fact would grow quadratically over time.
            connection.execute("INSERT OR IGNORE INTO protected_dependencies(object_ref,source_object) SELECT ?1,r.object_ref FROM protected_rows r JOIN events e ON e.event_id=r.row_key WHERE r.table_name='events' AND r.subject_digest=?2 AND r.object_ref!=?1 ORDER BY e.stream_seq DESC LIMIT 1",params![envelope.object.0,subject.0]).map_err(|_|p::Error("aggregate content lineage could not be recorded".into()))?;
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
                collect_references(&value, &mut references);
            }
        } else {
            references.insert(subject.0.clone());
        }
        for reference in references {
            let by_subject = p::sha256_content_digest(reference.as_bytes());
            connection.execute("INSERT OR IGNORE INTO protected_dependencies(object_ref,source_object) SELECT ?1,object_ref FROM protected_rows WHERE table_name='events' AND object_ref!=?1 AND (row_key=?2 OR subject_digest=?3 OR subject_digest=?4)",params![envelope.object.0,reference,by_subject.0,if table!="events" {&subject.0} else {""}]).map_err(|_|p::Error("protected content lineage could not be recorded".into()))?;
            if table == "events" {
                connection.execute("INSERT OR IGNORE INTO protected_dependencies(object_ref,source_object) SELECT object_ref,?1 FROM protected_rows WHERE table_name='protected_content' AND row_key=?2",params![envelope.object.0,reference]).map_err(|_|p::Error("external artifact lineage could not be recorded".into()))?;
            }
        }
    }
    Ok(sealed)
}

fn collect_references(value: &serde_json::Value, refs: &mut std::collections::BTreeSet<String>) {
    match value {
        serde_json::Value::String(s) => {
            refs.insert(s.clone());
            // EvidenceRef carries a transport prefix around an EventId. Keep
            // the literal too: an EventId itself may begin with `event:`.
            if let Some(id) = s.strip_prefix("event:") {
                refs.insert(id.to_owned());
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_references(value, refs);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                collect_references(value, refs);
            }
        }
        _ => {}
    }
}

pub use crate::file_keys::FileContentKeyResolver;

#[cfg(test)]
mod auxiliary_tests {
    use super::*;
    use crate::{EventStore, SqliteEventStore, StoreOptions};
    use std::fs;
    #[test]
    fn c22_all_auxiliary_blob_writers_authenticate_insert_update_and_row_binding() {
        let root = std::env::temp_dir().join(format!(
            "forme-aux-encryption-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let s = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
        s.enable_content_protection(
            StoreContentProtection::new(
                &p::VerifiedPrincipal("owner:aux".into()),
                p::Scope("workspace:aux".into()),
                Arc::new(FileContentKeyResolver::new(root.join("keys")).unwrap()),
            )
            .unwrap(),
        )
        .unwrap();
        s.append(p::Event::new(
            p::EventId("event:aux".into()),
            p::RunId("run:aux".into()),
            None,
            p::EventPayload::GoalFramed(p::GoalFramedPayload {
                goal_frame: p::GoalFrameRef("goal:aux".into()),
                long_term: None,
                knowledge: None,
            }),
            p::SchemaVersion(1),
            1,
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        ))
        .unwrap();
        {
            let c = s.core_connection().unwrap();
            for (table, key, column, _, _) in AUXILIARY_CONTENT {
                let fields = c
                    .prepare(&format!("PRAGMA table_info({table})"))
                    .unwrap()
                    .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                let private =
                    br#"{"fixture":"private-aux-c22-e546","source":"event:aux"}"#.to_vec();
                let values = fields
                    .iter()
                    .map(|(name, kind)| {
                        use rusqlite::types::Value;
                        if name == column {
                            return Value::Blob(private.clone());
                        }
                        if kind == "INTEGER" {
                            return Value::Integer(1);
                        }
                        Value::Text(
                            match name.as_str() {
                                "last_event_id" => "event:aux",
                                "aggregate" | "run_id" | "source_run" | "next_run" => "run:aux",
                                "scope" => "workspace:aux",
                                _ => "aux",
                            }
                            .into(),
                        )
                    })
                    .collect::<Vec<_>>();
                let columns = fields
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                let placeholders = vec!["?"; fields.len()].join(",");
                c.execute(
                    &format!("INSERT INTO {table}({columns}) VALUES({placeholders})"),
                    rusqlite::params_from_iter(values.iter()),
                )
                .unwrap_or_else(|e| panic!("{table}: {e}"));
                let (stored,plain):(Vec<u8>,Vec<u8>)=c.query_row(&format!("SELECT {column},forme_open_bound({column},'{table}',CAST({key} AS TEXT)) FROM {table}"),[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
                assert!(stored.starts_with(PREFIX), "{table}");
                assert_eq!(plain, private);
                assert!(!stored.windows(19).any(|w| w == b"private-aux-c22-e546"));
                assert!(c
                    .query_row(
                        &format!(
                            "SELECT forme_open_bound({column},'{table}','forged-row') FROM {table}"
                        ),
                        [],
                        |r| r.get::<_, Vec<u8>>(0)
                    )
                    .is_err());
                c.execute(
                    &format!("UPDATE {table} SET {column}=?1"),
                    params![br#"{"fixture":"private-second-generation"}"#.to_vec()],
                )
                .unwrap();
                let count:i64=c.query_row("SELECT COUNT(*) FROM protected_key_history h JOIN protected_rows r ON h.object_ref=r.object_ref WHERE r.table_name=?1",params![table],|r|r.get(0)).unwrap();
                assert_eq!(count, 2, "{table}");
            }
        }
        drop(s);
        fs::remove_dir_all(&root).unwrap();
    }
}
