//! Typed boundary for the updater/backup worker used by release activation.
//!
//! The product host deliberately has no built-in fallback implementation. A
//! managed installation must attach a fixed, independently supervised worker;
//! otherwise upgrade and rollback are rejected before a journal operation is
//! created.
use forme_protocol as p;
use std::time::Instant;

pub trait ReleaseWorkerProvider: Send + Sync {
    fn installation(&self) -> &p::InstallationManifest;
    fn preflight(&self, request: &p::ProductReleaseRequest) -> p::Result<()>;
    fn start(
        &self,
        request: p::ProductReleaseRequest,
        deadline: Instant,
    ) -> p::Result<Box<dyn ReleaseWorkerSession>>;
}

pub trait ReleaseWorkerSession: Send {
    fn step(
        &mut self,
        request: &p::ProductReleaseRequest,
        step: p::ProductReleaseStep,
    ) -> p::Result<p::ProductReleaseWorkerResponse>;
}
