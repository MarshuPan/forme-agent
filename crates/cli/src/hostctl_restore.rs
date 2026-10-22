//! Fixed installed maintenance worker; stdin is an inherited private Host pipe.
use forme_gateway::offline_data::OfflineRestoreWorker;
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
    let request: p::ProductRestoreRequest = read_frame(&mut input)?;
    let mut worker = OfflineRestoreWorker::open(&path, request).map_err(|e| e.0)?;
    write_frame(&mut output, &worker.validated().map_err(|e| e.0)?)?;
    loop {
        let command: p::ProductRestoreStepRequest = read_frame(&mut input)?;
        let release = command.step == p::ProductRestoreStep::Release;
        let response = worker.step(command).map_err(|e| e.0)?;
        if release {
            // The release receipt is sent only after both database guards drop.
            drop(worker);
            write_frame(&mut output, &response)?;
            return Ok(());
        }
        write_frame(&mut output, &response)?;
    }
}

fn read_frame<T: DeserializeOwned>(input: &mut impl BufRead) -> Result<T, String> {
    let mut line = String::new();
    input
        .take((p::RESTORE_WORKER_MAX_FRAME + 1) as u64)
        .read_line(&mut line)
        .map_err(|_| "restore worker frame could not be read".to_owned())?;
    if line.len() > p::RESTORE_WORKER_MAX_FRAME || !line.ends_with('\n') {
        return Err("restore worker frame is incomplete or oversized".into());
    }
    serde_json::from_str(&line).map_err(|_| "restore worker frame is invalid".into())
}

fn write_frame(output: &mut impl Write, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| "restore worker response is unavailable".to_owned())?;
    if bytes.len() + 1 > p::RESTORE_WORKER_MAX_FRAME {
        return Err("restore worker response is oversized".into());
    }
    output
        .write_all(&bytes)
        .and_then(|_| output.write_all(b"\n"))
        .and_then(|_| output.flush())
        .map_err(|_| "restore worker response could not be sent".into())
}

fn usage() -> String {
    "usage: forme-hostctl maintenance-restore --installation-manifest <protected-absolute-path>"
        .into()
}
