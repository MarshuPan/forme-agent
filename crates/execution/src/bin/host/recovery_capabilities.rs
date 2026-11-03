//! Pure offline mode: no environment, configuration, credentials or journal.
use forme_protocol as p;
use std::io::{BufRead, Read, Write};

pub(super) fn run() -> p::Result<()> {
    let mut line = Vec::new();
    std::io::stdin()
        .lock()
        .take(1025)
        .read_until(b'\n', &mut line)
        .map_err(|_| invalid())?;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Probe {
        schema_version: p::SchemaVersion,
    }
    let probe: Probe = serde_json::from_slice(&line).map_err(|_| invalid())?;
    if line.len() > 1024
        || line.last() != Some(&b'\n')
        || probe.schema_version != p::SchemaVersion(1)
    {
        return Err(invalid());
    }
    let capabilities = p::ProductReleaseRecoveryCapabilities {
        schema_version: p::SchemaVersion(1),
        journal_read_max: 2,
        updater_version: forme_execution::installed::RELEASE_UPDATER_VERSION,
    };
    capabilities.validate()?;
    let mut output = serde_json::to_vec(&capabilities).map_err(|_| invalid())?;
    output.push(b'\n');
    std::io::stdout()
        .lock()
        .write_all(&output)
        .map_err(|_| invalid())
}
fn invalid() -> p::Error {
    p::Error("release recovery capabilities are unavailable or incompatible".into())
}
