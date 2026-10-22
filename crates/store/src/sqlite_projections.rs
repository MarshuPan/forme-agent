//! Transactional charter and orchestration projections.
use super::*;

impl SqliteEventStore {
    pub fn orchestration_ledger(&self, stream: &p::RunId) -> p::Result<p::OrchestrationLedger> {
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("orchestration store is unavailable".into()))?;
        crate::orchestration::read(&connection, stream)
    }

    pub fn rebuild_orchestration(&self, stream: &p::RunId) -> p::Result<()> {
        self.require_authority_writer()?;
        let events = self
            .read_run(stream.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("orchestration store is unavailable".into()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| p::Error("orchestration rebuild transaction failed".into()))?;
        self.require_authority_writer_in_transaction(&tx)?;
        if aggregate_version_in_transaction(&tx, stream)?
            != events.last().map_or(0, |e| e.stream_seq)
        {
            return Err(p::Error("orchestration changed during rebuild".into()));
        }
        tx.execute(
            "DELETE FROM orchestration_state WHERE stream=?1",
            params![stream.0],
        )
        .map_err(|_| p::Error("orchestration reset failed".into()))?;
        for event in &events {
            crate::orchestration::fold(&tx, event)?;
        }
        tx.commit()
            .map_err(|_| p::Error("orchestration rebuild commit failed".into()))
    }

    pub fn workspace_charter(
        &self,
        workspace: &p::WorkspaceRef,
    ) -> p::Result<Option<p::WorkspaceCharterRecord>> {
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("charter store is unavailable".into()))?;
        crate::charter::read(&connection, workspace)
    }

    pub fn rebuild_charters(&self) -> p::Result<()> {
        self.require_authority_writer()?;
        let metadata = self
            .brain_authority()?
            .ok_or_else(|| p::Error("charter authority is missing".into()))?;
        let stream = metadata
            .aggregates
            .get(&BrainAggregateKind::Charter)
            .ok_or_else(|| p::Error("charter stream is missing".into()))?;
        let events = self
            .read_run(stream.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("charter store is unavailable".into()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| p::Error("charter rebuild transaction failed".into()))?;
        self.require_authority_writer_in_transaction(&tx)?;
        if aggregate_version_in_transaction(&tx, stream)?
            != events.last().map_or(0, |e| e.stream_seq)
        {
            return Err(p::Error("charter changed during rebuild".into()));
        }
        tx.execute("DELETE FROM workspace_charter_state", [])
            .map_err(|_| p::Error("charter rebuild reset failed".into()))?;
        for event in &events {
            crate::charter::fold(&tx, event)?;
        }
        tx.commit()
            .map_err(|_| p::Error("charter rebuild commit failed".into()))
    }
}
