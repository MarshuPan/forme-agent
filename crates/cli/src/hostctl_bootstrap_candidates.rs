//! Fixed read-only bootstrap pipe; no credentials, claim writes or runtime.
use forme_protocol as p;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;

pub fn run(args: &[String]) -> Result<(), String> {
    let [manifest_flag, manifest, epoch_flag, epoch] = args else {
        return Err(invalid());
    };
    let path = PathBuf::from(manifest);
    let epoch = epoch.parse::<u64>().map_err(|_| invalid())?;
    if manifest_flag != "--installation-manifest"
        || epoch_flag != "--verification-epoch"
        || !path.is_absolute()
        || epoch == 0
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    io::BufReader::new(io::stdin().lock())
        .take((p::RELEASE_WORKER_MAX_FRAME + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > p::RELEASE_WORKER_MAX_FRAME || bytes.last() != Some(&b'\n') {
        return Err(invalid());
    }
    let request: p::BootstrapCandidateCatalogRequestV2 =
        serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let inspection =
        forme_gateway::offline_data::OfflineBootstrapInspection::open(&path, &request, epoch)
            .map_err(|_| invalid())?;
    let catalog = inspection.catalog().map_err(|_| invalid())?;
    let mut bytes = serde_json::to_vec(catalog).map_err(|_| invalid())?;
    bytes.push(b'\n');
    if bytes.len() > p::RELEASE_WORKER_MAX_FRAME {
        return Err(invalid());
    }
    // Keep every source fence alive until the final byte is delivered.
    let mut output = io::stdout().lock();
    output
        .write_all(&bytes)
        .and_then(|_| output.flush())
        .map_err(|_| invalid())
}

fn invalid() -> String {
    "installed bootstrap candidate inspection is unavailable or invalid".into()
}
