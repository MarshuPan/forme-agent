//! Read-only current-source proof, using the original execution credential.
use forme_protocol as p;

/// Separate from dispatch and historical result access. Implementations must
/// verify the exact latest discovery, current settings and pinned registration.
pub trait ManagedMcpSourceVerifier: Send + Sync {
    fn verify_source(
        &self,
        original: &p::WorkerResultManifest,
    ) -> p::Result<p::ProductMcpServerConfig>;
}
