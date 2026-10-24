//! A release hold never opens or replaces the separate restored-brain hold.
use super::*;
use std::io::Read;
use std::path::Path;

pub(super) fn validate_transport(config: &CoreServiceConfig) -> p::Result<()> {
    let transport = &config.transport;
    match (
        &transport.release_binding,
        &transport.release_admission_path,
        &transport.release_readiness_path,
    ) {
        (None, None, None) => Ok(()),
        (Some(binding), Some(_), Some(_)) if transport.configuration_ack_path.is_some() => {
            binding.validate()?;
            if binding.install_ref != config.install_ref
                || binding.config_revision > config.revision
                || binding.owner_digest != p::sha256_content_digest(config.brain.owner.0.as_bytes())
            {
                return Err(unavailable());
            }
            Ok(())
        }
        _ => Err(unavailable()),
    }
}

/// Runs before a ProductionRuntime is constructed or a brain writer opened.
pub(super) fn current_image(
    binding: Option<&p::ProductReleaseBinding>,
) -> p::Result<Option<p::SchemaDigest>> {
    let Some(binding) = binding else {
        return Ok(None);
    };
    let image = std::env::current_exe().map_err(|_| unavailable())?;
    crate::web::config::check_path(&image)?;
    let file = fs::File::open(image).map_err(|_| unavailable())?;
    let digest = p::sha256_reader_digest(file)?;
    if digest != binding.core_image_digest {
        return Err(unavailable());
    }
    Ok(Some(digest))
}

pub(super) fn managed_admission(path: PathBuf, binding: p::ProductReleaseBinding) -> WorkAdmission {
    WorkAdmission::guarded(move || check(&path, &binding))
}

fn check(path: &Path, binding: &p::ProductReleaseBinding) -> p::Result<()> {
    crate::web::config::check_path(path)?;
    let file = fs::File::open(path).map_err(|_| unavailable())?;
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
    let admission: p::ProductReleaseAdmission =
        serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    admission.validate_for(binding)?;
    if !admission.released {
        return Err(unavailable());
    }
    Ok(())
}

fn unavailable() -> p::Error {
    p::Error("installed release has not verified its runtime and work admission".into())
}

#[cfg(test)]
mod tests;
