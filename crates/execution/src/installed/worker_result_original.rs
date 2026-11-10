//! A read-only view of the same result store; no second writer or key store.
use super::*;

impl WorkerResultStore {
    pub(crate) fn read_original(
        &self,
        manifest: &p::WorkerResultManifest,
        maximum: u64,
    ) -> p::Result<Vec<u8>> {
        manifest.validate()?;
        if maximum == 0 || maximum > 1_048_576 || manifest.bytes > maximum {
            return Err(unavailable());
        }
        let path = self.manifest_path(&manifest.manifest_digest)?;
        reject_linked_ancestors(&path)?;
        let file = File::open(&path).map_err(|_| unavailable())?;
        if !file.metadata().map_err(|_| unavailable())?.is_file() {
            return Err(unavailable());
        }
        let mut original = Vec::new();
        file.take(131_073)
            .read_to_end(&mut original)
            .map_err(|_| unavailable())?;
        if original.len() > 131_072 {
            return Err(unavailable());
        }
        let stored: p::WorkerResultManifest =
            serde_json::from_slice(&original).map_err(|_| unavailable())?;
        if stored != *manifest {
            return Err(unavailable());
        }
        let mut body = Vec::with_capacity(manifest.bytes as usize);
        loop {
            let offset = body.len() as u64;
            let chunk = self.read(
                manifest,
                &p::WorkerResultRead {
                    schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
                    binding: manifest.binding.clone(),
                    manifest_digest: manifest.manifest_digest.clone(),
                    offset,
                    max_bytes: p::WORKER_RESULT_MAX_CHUNK_BYTES,
                },
            )?;
            chunk.validate()?;
            if chunk.offset != offset
                || chunk.manifest_digest != manifest.manifest_digest
                || chunk.next_offset > manifest.bytes
                || (!chunk.finished && chunk.next_offset <= offset)
            {
                return Err(unavailable());
            }
            body.extend_from_slice(&chunk.bytes);
            if chunk.finished {
                if body.len() as u64 != manifest.bytes
                    || manifest.content_digest.as_ref() != Some(&p::sha256_content_digest(&body))
                {
                    return Err(unavailable());
                }
                return Ok(body);
            }
        }
    }
}
fn unavailable() -> p::Error {
    p::Error("original managed MCP result material is unavailable".into())
}
