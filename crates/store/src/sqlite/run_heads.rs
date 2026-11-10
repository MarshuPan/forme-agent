//! Candidate run selection is a read-only hint; consumers still verify events.
use super::*;

impl SqliteEventStore {
    /// The current per-run heads, read against one statement's snapshot.
    pub fn run_heads(&self) -> p::Result<crate::TickEventCursor> {
        self.selected_run_heads(None)
    }

    /// Select only runs containing a kind using the existing events index.
    /// This does not order separate runs or authenticate the selected payloads.
    pub fn run_heads_with_kind(&self, kind: p::EventKind) -> p::Result<crate::TickEventCursor> {
        self.selected_run_heads(Some(kind))
    }

    fn selected_run_heads(&self, kind: Option<p::EventKind>) -> p::Result<crate::TickEventCursor> {
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let sql = if kind.is_some() {
            "SELECT run_id, MAX(stream_seq) FROM events WHERE run_id IN \
             (SELECT run_id FROM events WHERE kind = ?1) GROUP BY run_id"
        } else {
            "SELECT run_id, MAX(stream_seq) FROM events WHERE ?1 IS NULL GROUP BY run_id"
        };
        let mut statement = connection
            .prepare(sql)
            .map_err(|e| store_error("failed to prepare run head scan", e))?;
        let rows = statement
            .query_map([kind.map(|k| k.as_str())], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| store_error("failed to scan run heads", e))?;
        let mut heads = BTreeMap::new();
        for row in rows {
            let (run, head) = row.map_err(|e| store_error("failed to decode run head", e))?;
            let head = u64::try_from(head)
                .map_err(|_| p::Error("stored stream sequence is invalid".into()))?;
            heads.insert(p::RunId(run), head);
        }
        Ok(crate::TickEventCursor(heads))
    }
}
