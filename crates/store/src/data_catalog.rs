//! Owner-only metadata projections over the existing protected-content catalog.
use crate::{BrainAuthorityStore, SqliteEventStore};
use forme_protocol as p;
use rusqlite::params;
use std::collections::{BTreeMap, BTreeSet};

impl SqliteEventStore {
    pub fn data_catalog(
        &self,
        owner: &p::VerifiedPrincipal,
        page: &p::DataPageQuery,
    ) -> p::Result<p::DataCatalogView> {
        page.validate()?;
        let authority = self
            .brain_authority()?
            .ok_or_else(|| p::Error("data owner is unavailable".into()))?;
        if authority.owner_digest != p::sha256_content_digest(owner.0.as_bytes()) {
            return Err(p::Error("data catalog owner mismatch".into()));
        }
        let mut connection = self
            .core_connection()
            .map_err(|_| p::Error("data catalog unavailable".into()))?;
        let tx = connection
            .transaction()
            .map_err(|_| p::Error("data catalog snapshot unavailable".into()))?;
        let mut statement = tx.prepare("SELECT r.scope,r.subject_digest,r.table_name,COALESCE(s.state,'active'),d.object_ref IS NOT NULL FROM protected_rows r LEFT JOIN data_lifecycle_subjects s ON s.scope=r.scope AND s.subject_digest=r.subject_digest LEFT JOIN data_lifecycle_deleted_objects d ON d.object_ref=r.object_ref WHERE r.owner_digest=?1 AND r.scope NOT LIKE 'maintenance:data-workbench%' ORDER BY r.scope,r.subject_digest,r.object_ref").map_err(|_| p::Error("data catalog unavailable".into()))?;
        let rows = statement
            .query_map(params![authority.owner_digest.0], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            })
            .map_err(|_| p::Error("data catalog unavailable".into()))?;
        let mut groups =
            BTreeMap::<(String, String), (usize, usize, BTreeSet<String>, String)>::new();
        for row in rows {
            let (scope, subject, table, state, deleted) =
                row.map_err(|_| p::Error("data catalog row invalid".into()))?;
            let entry = groups
                .entry((scope, subject))
                .or_insert((0, 0, BTreeSet::new(), state));
            entry.0 += 1;
            entry.1 += usize::from(deleted);
            entry.2.insert(projection_kind(&table).into());
        }
        drop(statement);
        let version: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(version),0) FROM data_lifecycle_state",
                [],
                |row| row.get(0),
            )
            .map_err(|_| p::Error("data version unavailable".into()))?;
        let version =
            u64::try_from(version).map_err(|_| p::Error("data version invalid".into()))?;
        let pending_cleanup: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM data_lifecycle_jobs)",
                [],
                |row| row.get(0),
            )
            .map_err(|_| p::Error("data fence unavailable".into()))?;
        let subjects: Vec<_> = groups
            .into_iter()
            .map(
                |((scope, subject), (objects, deleted_objects, projections, state))| {
                    p::DataSubjectView {
                        schema_version: p::SchemaVersion(1),
                        scope: p::Scope(scope),
                        subject: p::SchemaDigest(subject),
                        objects,
                        deleted_objects,
                        projections: projections.into_iter().collect(),
                        state,
                    }
                },
            )
            .collect();
        let digest = p::canonical_digest(&(version, pending_cleanup, &subjects))?;
        if page
            .snapshot
            .as_ref()
            .is_some_and(|expected| expected != &digest)
        {
            return Err(p::Error("data snapshot changed; restart pagination".into()));
        }
        let total = subjects.len();
        let end = page.offset.saturating_add(page.limit).min(total);
        Ok(p::DataCatalogView {
            schema_version: p::SchemaVersion(1),
            version,
            pending_cleanup,
            digest,
            total,
            next_offset: (end < total).then_some(end),
            subjects: subjects
                .into_iter()
                .skip(page.offset)
                .take(page.limit)
                .collect(),
        })
    }

    /// Internal opaque refs only; records remain encrypted and owner-bound.
    pub fn protected_content_refs(
        &self,
        owner: &p::VerifiedPrincipal,
        scope: &p::Scope,
    ) -> p::Result<Vec<p::ContentRef>> {
        let authority = self
            .brain_authority()?
            .ok_or_else(|| p::Error("content owner is unavailable".into()))?;
        if authority.owner_digest != p::sha256_content_digest(owner.0.as_bytes()) {
            return Err(p::Error("content catalog owner mismatch".into()));
        }
        let connection = self
            .core_connection()
            .map_err(|_| p::Error("content catalog unavailable".into()))?;
        let mut statement = connection.prepare("SELECT content_ref FROM protected_content WHERE owner_digest=?1 AND scope=?2 ORDER BY content_ref").map_err(|_| p::Error("content catalog unavailable".into()))?;
        let rows = statement
            .query_map(params![authority.owner_digest.0, scope.0], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| p::Error("content catalog unavailable".into()))?;
        rows.map(|row| {
            row.map(p::ContentRef)
                .map_err(|_| p::Error("content catalog invalid".into()))
        })
        .collect()
    }
}
fn projection_kind(table: &str) -> &'static str {
    match table {
        "events" | "protected_content" => "content",
        "transcript" | "transcripts" => "transcript",
        "session_state" => "session",
        _ if table.contains("fts") => "search",
        _ if table.contains("graph") => "graph",
        _ if table.contains("candidate") => "candidate",
        _ => "derived",
    }
}
