//! Transport fault fixture. It does not implement actual data maintenance.
use forme_protocol as p;
use std::io::{self, BufRead, Write};

pub fn run() {
    let manifest = std::env::args().nth(3).unwrap();
    let installation: p::InstallationManifest =
        serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let mut input = io::BufReader::new(io::stdin().lock());
    let request: p::ProductReleaseDataRequest = read(&mut input);
    assert!(std::env::var_os("FORME_MODEL_API_KEY").is_none());
    std::fs::write(
        installation.paths.host_state.join("data-worker-started"),
        b"1",
    )
    .unwrap();
    let mode = request.release.target_release_ref.as_str();
    if mode == "stall-before-inspect" {
        let _: p::ProductReleaseDataStepRequest = read(&mut input);
        return;
    }
    if mode == "oversized-frame" {
        io::stdout()
            .write_all(&vec![b'x'; p::RELEASE_WORKER_MAX_FRAME + 1])
            .unwrap();
        return;
    }
    let identity = p::ProductReleaseDataIdentity {
        schema_version: p::SchemaVersion(1),
        config_schema_version: 1,
        database_schema_version: 13,
        owner_digest: p::sha256_content_digest(if mode == "wrong-owner" {
            b"other-owner"
        } else {
            request.owner.0.as_bytes()
        }),
        store_identity: p::canonical_digest(&"fixture-store").unwrap(),
        schema_digest: p::canonical_digest(&"fixture-schema").unwrap(),
        database_digest: p::canonical_digest(&"fixture-database").unwrap(),
        event_count: 10,
        last_stream_seq: 7,
    };
    respond(
        &request,
        p::ProductReleaseDataResult::Inspected {
            identity: identity.clone(),
        },
    );
    if mode == "exit-after-inspect" {
        return;
    }
    loop {
        let step: p::ProductReleaseDataStepRequest = read(&mut input);
        let result = match step.step {
            p::ProductReleaseDataStep::Backup => {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(installation.paths.host_state.join("data-worker-backup"))
                    .unwrap()
                    .write_all(b"1")
                    .unwrap();
                if mode == "stall-on-backup" {
                    let _: p::ProductReleaseDataStepRequest = read(&mut input);
                    return;
                }
                p::ProductReleaseDataResult::BackedUp {
                    backup_ref: request.release.backup_ref.clone(),
                    backup_digest: p::canonical_digest(&"fixture-backup").unwrap(),
                    identity: identity.clone(),
                }
            }
            p::ProductReleaseDataStep::Release => p::ProductReleaseDataResult::Released {},
        };
        respond(&request, result);
        if step.step == p::ProductReleaseDataStep::Release {
            if mode == "nonzero-release" {
                std::process::exit(2);
            }
            return;
        }
    }
}
fn read<T: serde::de::DeserializeOwned>(input: &mut impl BufRead) -> T {
    let mut line = String::new();
    input.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
fn respond(request: &p::ProductReleaseDataRequest, result: p::ProductReleaseDataResult) {
    let response = p::ProductReleaseDataResponse {
        schema_version: p::SchemaVersion(1),
        operation_id: request.release.operation_id.clone(),
        command_digest: request.release.command_digest.clone(),
        result,
    };
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &response).unwrap();
    output.write_all(b"\n").unwrap();
    output.flush().unwrap();
}
