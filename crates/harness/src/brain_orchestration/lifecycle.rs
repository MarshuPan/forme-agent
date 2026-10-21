use crate::{AgentHarness, ReactiveHarness};
use forme_protocol as p;

impl ReactiveHarness {
    pub(crate) fn cancel_child_runs(&self, parent: &p::RunId) -> p::Result<()> {
        let children = self
            .child_harnesses
            .lock()
            .map_err(|_| p::Error("child registry is unavailable".into()))?
            .iter()
            .filter(|(_, (p, _))| p == parent)
            .map(|(run, (_, h))| (run.clone(), h.clone()))
            .collect::<Vec<_>>();
        for (run, harness) in children {
            if harness.run_handle(&run).is_ok() {
                harness.cancel(run)?;
            }
        }
        Ok(())
    }
}
