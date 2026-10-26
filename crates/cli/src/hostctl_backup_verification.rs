//! One fixed read-only frame; no caller-selected program, key or storage path.
use forme_protocol as p;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;

pub fn run(args: &[String]) -> Result<(), String> {
    let [flag, path] = args else {
        return Err(invalid());
    };
    let path = PathBuf::from(path);
    if flag != "--installation-manifest" || !path.is_absolute() {
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
    let request: p::ProductBackupVerificationRequest =
        serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let response = forme_gateway::offline_data::verify_installed_backup(&path, request)
        .map_err(|_| invalid())?;
    let mut bytes = serde_json::to_vec(&response).map_err(|_| invalid())?;
    bytes.push(b'\n');
    if bytes.len() > p::RELEASE_WORKER_MAX_FRAME {
        return Err(invalid());
    }
    let mut output = io::stdout().lock();
    output
        .write_all(&bytes)
        .and_then(|_| output.flush())
        .map_err(|_| invalid())
}

fn invalid() -> String {
    "installed backup verification is unavailable or invalid".into()
}
