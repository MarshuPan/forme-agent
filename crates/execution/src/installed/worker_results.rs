//! Host-owned, bounded worker result artifacts.
//!
//! Result bytes never enter the worker journal or brain event store. The
//! journal records the validated manifest binding; this store owns the
//! private immutable artifact and verifies its digest again on every read.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use forme_protocol as p;

use super::desktop_helper::DesktopHelperAdapter;
use super::worker_host::{WorkerExecutorHost, WorkerIsolation};
use super::worker_result_crypto::{ResultKeys, ENVELOPE_OVERHEAD};
use super::{reject_linked_ancestors, set_private_directory};

const MAX_RESULT_FILE_BYTES: u64 = p::WORKER_RESULT_MAX_BYTES;

struct ManifestInput {
    binding: p::WorkerResultBinding,
    result_ref: Option<String>,
    outcome: Option<p::WorkerOutcome>,
    status: p::WorkerResultStatus,
    bytes: u64,
    content_digest: Option<p::SchemaDigest>,
    encoding: p::WorkerResultEncoding,
    evidence_refs: Vec<String>,
    produced_at: p::Timestamp,
}

pub(super) struct WorkerResultSealRequest<'a> {
    pub(super) job: &'a p::WorkerJob,
    pub(super) receipt_ref: Option<String>,
    pub(super) result_ref: Option<String>,
    pub(super) outcome: Option<p::WorkerOutcome>,
    pub(super) source: Option<&'a Path>,
    pub(super) evidence_refs: Vec<String>,
    pub(super) now: p::Timestamp,
}

pub(super) fn installation_ref(agent_credential_digest: &p::SchemaDigest) -> p::Result<String> {
    p::validate_product_digest(agent_credential_digest)?;
    let value = agent_credential_digest
        .0
        .strip_prefix("sha256:")
        .ok_or_else(|| p::Error("agent execution credential digest is invalid".into()))?;
    let reference = format!("install-auth-{value}");
    p::validate_product_id(&reference)?;
    Ok(reference)
}

pub(crate) struct WorkerResultStore {
    root: PathBuf,
    keys: ResultKeys,
}

impl WorkerResultStore {
    pub(crate) fn open(root: &Path) -> p::Result<Self> {
        if !root.is_absolute() {
            return Err(p::Error("worker result root must be absolute".into()));
        }
        reject_linked_ancestors(root)?;
        fs::create_dir_all(root)
            .map_err(|_| p::Error("worker result root cannot be created".into()))?;
        reject_linked_ancestors(root)?;
        set_private_directory(root)?;
        Ok(Self {
            root: root.to_path_buf(),
            keys: ResultKeys::open(&root.with_extension("result-keys"))?,
        })
    }

    pub(crate) fn seal_path(
        &self,
        binding: p::WorkerResultBinding,
        result_ref: Option<String>,
        outcome: Option<p::WorkerOutcome>,
        source: Option<&Path>,
        evidence_refs: Vec<String>,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultManifest> {
        let Some(source) = source else {
            return self.persist_manifest(Self::manifest(ManifestInput {
                binding,
                result_ref,
                outcome,
                status: p::WorkerResultStatus::Missing,
                bytes: 0,
                content_digest: None,
                encoding: p::WorkerResultEncoding::Binary,
                evidence_refs,
                produced_at: now,
            })?);
        };
        reject_linked_ancestors(source)?;
        let metadata = match fs::symlink_metadata(source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return self.persist_manifest(Self::manifest(ManifestInput {
                    binding,
                    result_ref,
                    outcome,
                    status: p::WorkerResultStatus::Missing,
                    bytes: 0,
                    content_digest: None,
                    encoding: p::WorkerResultEncoding::Binary,
                    evidence_refs,
                    produced_at: now,
                })?);
            }
            Err(_) => return Err(p::Error("worker result source is unavailable".into())),
        };
        if !metadata.is_file() {
            return Err(p::Error(
                "worker result source is not a regular file".into(),
            ));
        }
        if metadata.len() > binding.output_budget_bytes || metadata.len() > MAX_RESULT_FILE_BYTES {
            return Err(p::Error("worker result exceeds its output budget".into()));
        }
        let mut bytes = Vec::new();
        File::open(source)
            .map_err(|_| p::Error("worker result source cannot be read".into()))?
            .take(MAX_RESULT_FILE_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| p::Error("worker result source cannot be read".into()))?;
        if bytes.len() as u64 > binding.output_budget_bytes
            || bytes.len() as u64 > MAX_RESULT_FILE_BYTES
        {
            return Err(p::Error("worker result exceeds its output budget".into()));
        }
        let content_digest = p::sha256_content_digest(&bytes);
        let status = if bytes.is_empty() {
            p::WorkerResultStatus::Empty
        } else {
            p::WorkerResultStatus::Available
        };
        let encoding = if std::str::from_utf8(&bytes).is_ok() {
            p::WorkerResultEncoding::Utf8
        } else {
            p::WorkerResultEncoding::Binary
        };
        let manifest = Self::manifest(ManifestInput {
            binding,
            result_ref,
            outcome,
            status,
            bytes: bytes.len() as u64,
            content_digest: Some(content_digest),
            encoding,
            evidence_refs,
            produced_at: now,
        })?;
        self.persist_bytes(&manifest, &bytes)?;
        self.persist_manifest(manifest)
    }

    pub(crate) fn pending(
        &self,
        binding: p::WorkerResultBinding,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultManifest> {
        Self::manifest(ManifestInput {
            binding,
            result_ref: None,
            outcome: None,
            status: p::WorkerResultStatus::Pending,
            bytes: 0,
            content_digest: None,
            encoding: p::WorkerResultEncoding::Binary,
            evidence_refs: Vec::new(),
            produced_at: now,
        })
    }

    pub(crate) fn read(
        &self,
        manifest: &p::WorkerResultManifest,
        request: &p::WorkerResultRead,
    ) -> p::Result<p::WorkerResultChunk> {
        manifest.validate()?;
        request.validate_for(manifest)?;
        match manifest.status {
            p::WorkerResultStatus::Empty | p::WorkerResultStatus::Available => {}
            p::WorkerResultStatus::Pending => {
                return Err(p::Error("worker result is not terminal".into()))
            }
            p::WorkerResultStatus::Missing => {
                return Err(p::Error("worker result is missing".into()))
            }
            p::WorkerResultStatus::Corrupt => {
                return Err(p::Error("worker result is corrupt".into()))
            }
            p::WorkerResultStatus::Cleaned => {
                return Err(p::Error("worker result has been cleaned".into()))
            }
        }
        let path = self.data_path(&manifest.manifest_digest)?;
        reject_linked_ancestors(&path)?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| p::Error("worker result artifact is unavailable".into()))?;
        if !metadata.is_file() || metadata.len() != manifest.bytes + ENVELOPE_OVERHEAD {
            return Err(p::Error("worker result artifact size is invalid".into()));
        }
        let mut envelope = Vec::new();
        File::open(&path)
            .and_then(|file| {
                file.take(MAX_RESULT_FILE_BYTES + ENVELOPE_OVERHEAD + 1)
                    .read_to_end(&mut envelope)
            })
            .map_err(|_| p::Error("worker result artifact cannot be read".into()))?;
        let plaintext = self.keys.decrypt(manifest, &envelope)?;
        let digest = p::sha256_content_digest(&plaintext);
        if Some(digest) != manifest.content_digest {
            return Err(p::Error("worker result artifact digest is invalid".into()));
        }
        if request.offset > manifest.bytes {
            return Err(p::Error(
                "worker result offset is outside the artifact".into(),
            ));
        }
        let remaining = manifest.bytes.saturating_sub(request.offset);
        let size = remaining.min(request.max_bytes) as usize;
        let start = request.offset as usize;
        let bytes = plaintext[start..start + size].to_vec();
        let next = request.offset + bytes.len() as u64;
        Self::chunk(manifest, request.offset, bytes, next == manifest.bytes)
    }

    fn manifest(input: ManifestInput) -> p::Result<p::WorkerResultManifest> {
        let mut manifest = p::WorkerResultManifest {
            schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
            manifest_digest: p::SchemaDigest(String::new()),
            binding: input.binding,
            result_ref: input.result_ref,
            status: input.status,
            outcome: input.outcome,
            bytes: input.bytes,
            content_digest: input.content_digest,
            encoding: input.encoding,
            truncated: false,
            produced_at: input.produced_at,
            evidence_refs: input.evidence_refs,
        };
        manifest.manifest_digest = manifest.expected_digest()?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn chunk(
        manifest: &p::WorkerResultManifest,
        offset: u64,
        bytes: Vec<u8>,
        finished: bool,
    ) -> p::Result<p::WorkerResultChunk> {
        let chunk = p::WorkerResultChunk {
            schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
            manifest_digest: manifest.manifest_digest.clone(),
            offset,
            chunk_digest: p::sha256_content_digest(&bytes),
            next_offset: offset + bytes.len() as u64,
            finished,
            bytes,
        };
        chunk.validate()?;
        Ok(chunk)
    }

    fn persist_bytes(&self, manifest: &p::WorkerResultManifest, bytes: &[u8]) -> p::Result<()> {
        let path = self.data_path(&manifest.manifest_digest)?;
        reject_linked_ancestors(&path)?;
        if path
            .try_exists()
            .map_err(|_| p::Error("worker result cannot be inspected".into()))?
        {
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| p::Error("worker result cannot be inspected".into()))?;
            if !metadata.is_file() || metadata.len() != manifest.bytes + ENVELOPE_OVERHEAD {
                return Err(p::Error("existing worker result cannot be verified".into()));
            }
            let envelope = fs::read(&path)
                .map_err(|_| p::Error("existing worker result cannot be verified".into()))?;
            if self.keys.decrypt(manifest, &envelope)?.as_slice() != bytes {
                return Err(p::Error(
                    "worker result artifact collision was detected".into(),
                ));
            }
            return Ok(());
        }
        let envelope = self.keys.seal(manifest, bytes)?;
        match create_private_file(&path) {
            Ok(mut file) => {
                file.write_all(&envelope)
                    .and_then(|()| file.sync_all())
                    .map_err(|_| p::Error("worker result artifact cannot be persisted".into()))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = fs::read(&path)
                    .map_err(|_| p::Error("existing worker result cannot be verified".into()))?;
                if self.keys.decrypt(manifest, &existing)?.as_slice() != bytes {
                    return Err(p::Error(
                        "worker result artifact collision was detected".into(),
                    ));
                }
            }
            Err(_) => return Err(p::Error("worker result artifact cannot be created".into())),
        }
        Ok(())
    }

    fn persist_manifest(
        &self,
        manifest: p::WorkerResultManifest,
    ) -> p::Result<p::WorkerResultManifest> {
        manifest.validate()?;
        let path = self.manifest_path(&manifest.manifest_digest)?;
        let bytes = serde_json::to_vec(&manifest)
            .map_err(|_| p::Error("worker result manifest cannot be serialized".into()))?;
        match create_private_file(&path) {
            Ok(mut file) => {
                file.write_all(&bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|_| p::Error("worker result manifest cannot be persisted".into()))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = fs::read(&path)
                    .map_err(|_| p::Error("existing worker manifest cannot be verified".into()))?;
                if existing != bytes {
                    return Err(p::Error(
                        "worker result manifest collision was detected".into(),
                    ));
                }
            }
            Err(_) => return Err(p::Error("worker result manifest cannot be created".into())),
        }
        Ok(manifest)
    }

    fn data_path(&self, digest: &p::SchemaDigest) -> p::Result<PathBuf> {
        let suffix = digest
            .0
            .strip_prefix("sha256:")
            .ok_or_else(|| p::Error("worker result digest is invalid".into()))?;
        Ok(self.root.join(format!("data-{suffix}.bin")))
    }

    fn manifest_path(&self, digest: &p::SchemaDigest) -> p::Result<PathBuf> {
        let suffix = digest
            .0
            .strip_prefix("sha256:")
            .ok_or_else(|| p::Error("worker result manifest digest is invalid".into()))?;
        Ok(self.root.join(format!("manifest-{suffix}.json")))
    }
}

pub(super) trait WorkerResultHostExt {
    fn seal_result(
        &self,
        request: WorkerResultSealRequest<'_>,
    ) -> p::Result<p::WorkerResultManifest>;

    fn result_manifest_for(
        &self,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultManifest>;

    fn result_manifest_endpoint(
        &self,
        auth: &super::HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultManifest>;

    fn read_result_endpoint(
        &self,
        auth: &super::HostAuth,
        request: p::WorkerResultRead,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultChunk>;
}

impl<I: WorkerIsolation, D: DesktopHelperAdapter> WorkerResultHostExt for WorkerExecutorHost<I, D> {
    fn seal_result(
        &self,
        request: WorkerResultSealRequest<'_>,
    ) -> p::Result<p::WorkerResultManifest> {
        if let Some(manifest) = self.journal.result_manifest(&request.job.job_id)? {
            return Ok(manifest);
        }
        let binding = self.journal.result_binding(
            &request.job.job_id,
            self.installation_ref.clone(),
            request.receipt_ref,
        )?;
        let manifest = self.result_store.seal_path(
            binding,
            request.result_ref,
            request.outcome,
            request.source,
            request.evidence_refs,
            request.now,
        )?;
        self.journal.record_result_manifest(manifest.clone())?;
        Ok(manifest)
    }

    fn result_manifest_for(
        &self,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultManifest> {
        let observation = match self.refresh_isolation(job_id, now) {
            Ok(observation) => observation,
            Err(error) => {
                let reason = p::canonical_digest(&("worker-result-observation-unknown", job_id))?.0;
                self.journal.mark_unknown(job_id, reason, now)?;
                return Err(error);
            }
        };
        if let Some(manifest) = self.journal.result_manifest(job_id)? {
            manifest.validate()?;
            return Ok(manifest);
        }
        let job = self.journal.job(job_id)?;
        let binding = self.journal.result_binding(
            job_id,
            self.installation_ref.clone(),
            observation.receipt_ref.clone(),
        )?;
        if matches!(
            observation.state,
            p::WorkerState::Pending | p::WorkerState::Running | p::WorkerState::CancelRequested
        ) {
            return self.result_store.pending(binding, now);
        }
        self.seal_result(WorkerResultSealRequest {
            job: &job,
            receipt_ref: observation.receipt_ref,
            result_ref: observation.safe_result_ref,
            outcome: observation.outcome,
            source: None,
            evidence_refs: observation.ground_truth_refs,
            now,
        })
    }

    fn result_manifest_endpoint(
        &self,
        auth: &super::HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultManifest> {
        self.verify(auth)?;
        let _operation = self.operations.lock().map_err(|_| {
            p::Error("worker journal storage is unavailable; outcome must be checked".into())
        })?;
        self.result_manifest_for(job_id, now)
    }

    fn read_result_endpoint(
        &self,
        auth: &super::HostAuth,
        request: p::WorkerResultRead,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultChunk> {
        self.verify(auth)?;
        let _operation = self.operations.lock().map_err(|_| {
            p::Error("worker journal storage is unavailable; outcome must be checked".into())
        })?;
        request.validate()?;
        let manifest = self.result_manifest_for(&request.binding.job_id, now)?;
        self.result_store.read(&manifest, &request)
    }
}

fn create_private_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
