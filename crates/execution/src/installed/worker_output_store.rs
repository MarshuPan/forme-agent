//! Only Host-verified complete PNG bytes enter this sealing path.
use super::*;
impl WorkerResultStore {
    pub(in crate::installed) fn desktop_staging_root(&self) -> PathBuf {
        self.root.join("desktop-pending")
    }
    pub(in crate::installed) fn seal_desktop_output(
        &self,
        binding: p::WorkerResultBinding,
        bytes: &[u8],
        evidence_refs: Vec<String>,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultManifest> {
        binding.validate()?;
        if bytes.is_empty() || bytes.len() as u64 > binding.output_budget_bytes {
            return Err(p::Error("desktop output exceeds its result budget".into()));
        }
        let result_ref = format!(
            "desktop-result:{}",
            &p::canonical_digest(&("forme.desktop.sealed-output.v2", &binding))?.0[7..]
        );
        let manifest = Self::manifest(ManifestInput {
            binding,
            result_ref: Some(result_ref),
            outcome: Some(p::WorkerOutcome::Completed),
            status: p::WorkerResultStatus::Available,
            bytes: bytes.len() as u64,
            content_digest: Some(p::sha256_content_digest(bytes)),
            encoding: p::WorkerResultEncoding::Binary,
            evidence_refs,
            produced_at: now,
        })?;
        self.persist_bytes(&manifest, bytes)?;
        self.persist_manifest(manifest)
    }
}
