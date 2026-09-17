use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use forme_protocol as p;

use crate::{ActionBackend, ActionResult, BackendKind, CancelToken, EventSink, ExecutionPlan};

#[derive(Default)]
pub struct ExecutionBackendRegistry {
    backends: Mutex<HashMap<BackendKind, Arc<dyn ActionBackend + Send + Sync>>>,
}

impl ExecutionBackendRegistry {
    pub fn register(&self, backend: Arc<dyn ActionBackend + Send + Sync>) -> p::Result<()> {
        let kind = backend.kind();
        let mut backends = self
            .backends
            .lock()
            .map_err(|_| p::Error("execution backend registry is unavailable".into()))?;
        if backends.insert(kind, backend).is_some() {
            return Err(p::Error(
                "execution backend kind is already registered".into(),
            ));
        }
        Ok(())
    }

    pub fn backend(&self, kind: BackendKind) -> p::Result<Arc<dyn ActionBackend + Send + Sync>> {
        self.backends
            .lock()
            .map_err(|_| p::Error("execution backend registry is unavailable".into()))?
            .get(&kind)
            .cloned()
            .ok_or_else(|| p::Error("requested execution backend is not registered".into()))
    }

    pub fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        cancel: CancelToken,
    ) -> p::Result<ActionResult> {
        self.backend(plan.backend)?.execute(plan, sink, cancel)
    }
}
