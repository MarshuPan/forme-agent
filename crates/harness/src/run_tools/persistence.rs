//! Run originals use the existing encrypted retention/deletion lifecycle.
use super::*;

const MAX_BYTES: usize = 4 * 1024 * 1024;
fn object(run: &p::RunId) -> String {
    format!("run-tools-v1:{}", run.0)
}
fn reference(run: &p::RunId) -> p::ContentRef {
    p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(object(run).as_bytes()).0
    ))
}
impl RunToolSnapshots {
    pub(super) fn read(&self, run: &p::RunId) -> p::Result<Option<RunToolSnapshot>> {
        let Some(record) = self
            .store
            .protected_content_for_current_owner(&reference(run))?
        else {
            return Ok(None);
        };
        if record.owner_digest != p::sha256_content_digest(self.owner.0.as_bytes())
            || record.subject_digest != p::sha256_content_digest(record.scope.0.as_bytes())
            || record.envelope.ciphertext.len() > MAX_BYTES + 16
        {
            return Err(invalid());
        }
        let protection = self.store.content_protection()?.ok_or_else(invalid)?;
        let bytes = protection
            .content_service()
            .decrypt(&record.envelope.binding(), &record.envelope)?;
        let value: RunToolSnapshot = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if value.scope != record.scope || value.run != *run {
            return Err(invalid());
        }
        Ok(Some(value))
    }
    pub(super) fn write(&self, snapshot: &RunToolSnapshot) -> p::Result<()> {
        if let Some(prior) = self.read(&snapshot.run)? {
            return if prior == *snapshot {
                Ok(())
            } else {
                Err(invalid())
            };
        }
        let bytes = serde_json::to_vec(snapshot).map_err(|_| invalid())?;
        if bytes.len() > MAX_BYTES {
            return Err(invalid());
        }
        let protection = self.store.content_protection()?.ok_or_else(invalid)?;
        let (_, record) = protection.seal_record(
            &bytes,
            &snapshot.scope,
            &object(&snapshot.run),
            &p::sha256_content_digest(snapshot.scope.0.as_bytes()),
        )?;
        if record.content_ref != reference(&snapshot.run) {
            return Err(invalid());
        }
        protection
            .lifecycle_service(self.store.as_ref().clone())
            .put_protected_content(&self.owner, record)?;
        if self.read(&snapshot.run)?.as_ref() != Some(snapshot) {
            return Err(invalid());
        }
        Ok(())
    }
}
