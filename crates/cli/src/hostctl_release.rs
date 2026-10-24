//! Private installed release-data pipe; no model, service control or public API.
use forme_gateway::offline_data::OfflineReleaseWorker;
use forme_protocol as p;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    io::{self, BufRead, Read, Write},
    path::PathBuf,
};

pub fn run(args: &[String]) -> Result<(), String> {
    let [flag, path] = args else {
        return Err(usage());
    };
    let path = PathBuf::from(path);
    if flag != "--installation-manifest" || !path.is_absolute() {
        return Err(usage());
    }
    let mut input = io::BufReader::new(io::stdin().lock());
    let mut output = io::stdout().lock();
    let request: p::ProductReleaseDataRequest = read_frame(&mut input)?;
    let mut worker = OfflineReleaseWorker::open(&path, request).map_err(|e| e.0)?;
    write_frame(&mut output, &worker.inspected().map_err(|e| e.0)?)?;
    loop {
        let command: p::ProductReleaseDataStepRequest = read_frame(&mut input)?;
        let release = command.step == p::ProductReleaseDataStep::Release;
        let response = worker.step(command).map_err(|e| e.0)?;
        write_frame(&mut output, &response)?;
        if release {
            return Ok(());
        }
    }
}

fn read_frame<T: DeserializeOwned>(input: &mut impl BufRead) -> Result<T, String> {
    let mut bytes = Vec::new();
    input
        .take((p::RELEASE_WORKER_MAX_FRAME + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| "release data frame could not be read".to_owned())?;
    if bytes.len() > p::RELEASE_WORKER_MAX_FRAME || bytes.last() != Some(&b'\n') {
        return Err("release data frame is incomplete or oversized".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "release data frame is invalid".into())
}

fn write_frame(output: &mut impl Write, value: &impl Serialize) -> Result<(), String> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| "release data response is unavailable".to_owned())?;
    if bytes.len() + 1 > p::RELEASE_WORKER_MAX_FRAME {
        return Err("release data response is oversized".into());
    }
    output
        .write_all(&bytes)
        .and_then(|_| output.write_all(b"\n"))
        .and_then(|_| output.flush())
        .map_err(|_| "release data response could not be sent".into())
}

fn usage() -> String {
    "usage: forme-hostctl maintenance-release-data --installation-manifest <protected-absolute-path>".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_data_pipe_rejects_incomplete_oversized_and_unknown_frames() {
        for bytes in [
            b"{}".to_vec(),
            vec![b'x'; p::RELEASE_WORKER_MAX_FRAME + 1],
            b"{\"schema_version\":1,\"unexpected\":true}\n".to_vec(),
        ] {
            assert!(read_frame::<p::ProductReleaseDataRequest>(&mut &bytes[..]).is_err());
        }
    }
}
