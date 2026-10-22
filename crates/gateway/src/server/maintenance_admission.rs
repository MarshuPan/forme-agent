//! Read-only projection of Host's committed lifecycle; never a brain writer.
use super::*;
use std::io::Read;

pub(super) fn managed_admission(
    path: PathBuf,
    install_ref: String,
    revision: u64,
    identity: p::ProductDataAcknowledgement,
) -> WorkAdmission {
    WorkAdmission::guarded(move || {
        crate::web::config::check_path(&path)?;
        if !path.try_exists().map_err(|_| unavailable())? {
            return if identity.execution_fence_digest.is_none() {
                Ok(())
            } else {
                Err(unavailable())
            };
        }
        let file = fs::File::open(&path).map_err(|_| unavailable())?;
        if !file.metadata().map_err(|_| unavailable())?.is_file() {
            return Err(unavailable());
        }
        let mut bytes = Vec::new();
        file.take(8193)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable())?;
        if bytes.len() > 8192 {
            return Err(unavailable());
        }
        let admission: p::ProductWorkAdmission =
            serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        admission.validate()?;
        if !admission.released
            || admission.install_ref != install_ref
            || admission.config_revision > revision
            || admission.owner_digest != identity.owner_digest
            || Some(&admission.execution_fence_digest) != identity.execution_fence_digest.as_ref()
        {
            return Err(unavailable());
        }
        Ok(())
    })
}

fn unavailable() -> p::Error {
    p::Error("installed lifecycle has not released work admission".into())
}
