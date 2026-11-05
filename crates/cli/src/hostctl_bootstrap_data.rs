//! Fixed finite bootstrap-data pipe. No runtime or model is constructed.
use forme_protocol as p;
use std::{
    io::{self, BufRead, Read, Write},
    path::PathBuf,
};

pub fn run(args: &[String]) -> Result<(), String> {
    let [flag, path] = args else {
        return Err(invalid());
    };
    let path = PathBuf::from(path);
    if flag != "--installation-manifest" || !path.is_absolute() {
        return Err(invalid());
    }
    let mut input = io::BufReader::new(io::stdin().lock());
    let p::BootstrapDataWorkerRequestV2::Prepare {
        schema_version: p::SchemaVersion(2),
        intent,
    } = read(&mut input)?
    else {
        return Err(invalid());
    };
    let mut worker =
        forme_gateway::offline_data::OfflineBootstrapDataWorker::prepare(&path, intent)
            .map_err(|_| invalid())?;
    let mut output = io::stdout().lock();
    write(&mut output, &worker.prepared().map_err(|_| invalid())?)?;
    let request = read(&mut input)?;
    if !matches!(request, p::BootstrapDataWorkerRequestV2::Confirm { .. }) {
        return Err(invalid());
    }
    write(&mut output, &worker.step(request).map_err(|_| invalid())?)?;
    let request = read(&mut input)?;
    if !matches!(request, p::BootstrapDataWorkerRequestV2::Release { .. }) {
        return Err(invalid());
    }
    write(&mut output, &worker.step(request).map_err(|_| invalid())?)
}

fn read(input: &mut impl BufRead) -> Result<p::BootstrapDataWorkerRequestV2, String> {
    let mut bytes = Vec::new();
    input
        .take((p::RELEASE_WORKER_MAX_FRAME + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > p::RELEASE_WORKER_MAX_FRAME || bytes.last() != Some(&b'\n') {
        return Err(invalid());
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

fn write(
    output: &mut impl Write,
    response: &p::BootstrapDataWorkerResponseV2,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(response).map_err(|_| invalid())?;
    bytes.push(b'\n');
    if bytes.len() > p::RELEASE_WORKER_MAX_FRAME {
        return Err(invalid());
    }
    output
        .write_all(&bytes)
        .and_then(|_| output.flush())
        .map_err(|_| invalid())
}

fn invalid() -> String {
    "installed bootstrap data transaction is unavailable or requires recovery".into()
}
