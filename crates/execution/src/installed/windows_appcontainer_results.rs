//! Terminal observation and durable handoff of native sandbox result sources.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use forme_protocol as p;

use super::super::process::write_private_json;
use super::super::windows_workspace_overlay::Completion;
use super::{recovery, WindowsAppContainerWorkerIsolation};

const PENDING: &str = "result-seal-pending.json";
const SEALED: &str = "result-sealed.json";

fn binding(job: &p::WorkerJob) -> p::Result<p::SchemaDigest> {
    p::canonical_digest(&("forme-native-result-handoff-v1", job))
}

pub(in crate::installed) fn begin_handoff(run_dir: &Path, job: &p::WorkerJob) -> p::Result<()> {
    write_private_json(&run_dir.join(PENDING), &binding(job)?)
}

/// Startup and Drop may revoke capabilities and terminate the original tree,
/// but must retain source bytes whose Host seal has not been acknowledged.
pub(in crate::installed) fn verify_cleanup_allowed(run_dir: &Path) -> p::Result<()> {
    let pending = run_dir.join(PENDING);
    super::super::reject_linked_ancestors(&pending)?;
    let sealed = run_dir.join(SEALED);
    super::super::reject_linked_ancestors(&sealed)?;
    match fs::read(&pending) {
        Ok(expected) if fs::read(&sealed).ok().as_ref() == Some(&expected) => Ok(()),
        Ok(_) => Err(p::Error(
            "worker result handoff is unsealed; original job evidence is retained".into(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Legacy/setup-only leases have no result directory. A missing
            // marker beside a current source must never authorize deletion.
            if sealed
                .try_exists()
                .map_err(|_| p::Error("result handoff is unavailable".into()))?
                || run_dir
                    .join("results")
                    .try_exists()
                    .map_err(|_| p::Error("result source is unavailable".into()))?
            {
                Err(p::Error("worker result handoff evidence is missing".into()))
            } else {
                Ok(())
            }
        }
        Err(_) => Err(p::Error("worker result handoff cannot be verified".into())),
    }
}

pub(in crate::installed) fn acknowledge_handoff(
    run_dir: &Path,
    job: &p::WorkerJob,
) -> p::Result<()> {
    let expected = serde_json::to_vec(&binding(job)?)
        .map_err(|_| p::Error("worker result handoff cannot be encoded".into()))?;
    if fs::read(run_dir.join(PENDING)).ok().as_ref() != Some(&expected) {
        return Err(p::Error("worker result handoff binding changed".into()));
    }
    let sealed = run_dir.join(SEALED);
    if sealed
        .try_exists()
        .map_err(|_| p::Error("result handoff is unavailable".into()))?
    {
        verify_cleanup_allowed(run_dir)?;
    } else {
        write_private_json(&sealed, &binding(job)?)?;
    }
    Ok(())
}

impl WindowsAppContainerWorkerIsolation {
    pub(super) fn observe_worker(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<Option<p::WorkerReceipt>> {
        let mut children = self
            .children
            .lock()
            .map_err(|_| p::Error("worker process registry is unavailable".into()))?;
        let Some(worker) = children.get_mut(&job.job_id) else {
            return Ok(None);
        };
        if let Some(receipt) = &worker.terminal_receipt {
            return Ok(Some(receipt.clone()));
        }
        let observed = worker.native.terminal_state()?;
        if !observed.0 {
            return Ok(None);
        }
        let orphaned_children = observed.1 != 0;
        if orphaned_children {
            worker.native.cancel()?;
        }
        let terminal = worker.native.terminal_state()?;
        if !terminal.0 || terminal.1 != 0 {
            return Ok(None);
        }
        let exit_code = worker.native.exit_code()?;
        let committed = exit_code == 0
            && !orphaned_children
            && worker.overlay.complete(job, now)? == Completion::Committed;
        let digest =
            p::canonical_digest(&("worker-appcontainer-receipt", &job.job_id, &job.plan_digest))?.0;
        let mut evidence = vec![
            format!("worker-exit-{digest}"),
            format!("worker-exit-code-{exit_code}"),
        ];
        if orphaned_children {
            evidence.push(format!("worker-orphaned-child-tree-{digest}"));
        }
        let receipt = p::WorkerReceipt {
            schema_version: p::SchemaVersion(1),
            receipt_ref: format!("worker-receipt-{digest}"),
            job_id: job.job_id.clone(),
            plan_digest: job.plan_digest.clone(),
            authority_epoch: job.authority_epoch,
            nonce: job.nonce.clone(),
            observed_outcome: if committed {
                p::WorkerOutcome::Completed
            } else {
                p::WorkerOutcome::Failed
            },
            safe_result_ref: Some(format!("worker-result-{digest}")),
            ground_truth_refs: evidence,
            started_at: worker.started_at,
            finished_at: Some(now),
        };
        worker.terminal_receipt = Some(receipt.clone());
        Ok(Some(receipt))
    }

    pub(super) fn terminal_result_path(&self, job: &p::WorkerJob) -> p::Result<Option<PathBuf>> {
        let children = self
            .children
            .lock()
            .map_err(|_| p::Error("worker process registry is unavailable".into()))?;
        let worker = children
            .get(&job.job_id)
            .ok_or_else(|| p::Error("worker result source is not active".into()))?;
        if worker.native.terminal_state()? != (true, 0) {
            return Err(p::Error("worker result source is not terminal".into()));
        }
        Ok(Some(worker.run_dir.join("results/result.bin")))
    }

    pub(super) fn finish_result_handoff(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<()> {
        let mut children = self
            .children
            .lock()
            .map_err(|_| p::Error("worker process registry is unavailable".into()))?;
        let worker = children
            .get(&job.job_id)
            .ok_or_else(|| p::Error("worker result handoff is not active".into()))?;
        if worker.native.terminal_state()? != (true, 0) {
            return Err(p::Error("worker result handoff is not terminal".into()));
        }
        acknowledge_handoff(&worker.run_dir, job)?;
        if let Err(error) = recovery::cleanup_lease(
            &self.journal,
            &worker.lease_id,
            &worker.run_dir,
            worker.native.profile(),
            (true, 0),
            now,
        ) {
            self.recovery_required.store(true, Ordering::SeqCst);
            return Err(error);
        }
        children.remove(&job.job_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsealed_or_changed_handoff_preserves_source_and_blocks_cleanup() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        fs::create_dir(root.join("results")).unwrap();
        fs::write(root.join("results/result.bin"), b"preserve unknown result").unwrap();
        assert!(verify_cleanup_allowed(root).is_err());
        fs::write(root.join(PENDING), b"bound-job").unwrap();
        assert!(verify_cleanup_allowed(root).is_err());
        fs::write(root.join(SEALED), b"other-job").unwrap();
        assert!(verify_cleanup_allowed(root).is_err());
        assert_eq!(
            fs::read(root.join("results/result.bin")).unwrap(),
            b"preserve unknown result"
        );
        fs::write(root.join(SEALED), b"bound-job").unwrap();
        verify_cleanup_allowed(root).unwrap();
    }
}
