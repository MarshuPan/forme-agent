#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use forme_execution::installed::{decode_worker_input_frame, WorkerInputFrame};
use forme_protocol as p;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

const MAX_FRAME_BYTES: usize = p::WORKER_INPUT_MAX_TOTAL_BYTES as usize + 64 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

fn main() {
    if let Err(error) = run() {
        eprintln!("forme-pty-worker: {}", error.0);
        std::process::exit(1);
    }
}

fn run() -> p::Result<()> {
    let execution_path = required_path("FORME_WORKER_EXECUTION_PATH")?;
    let result_path = required_path("FORME_WORKER_RESULT_PATH")?;
    let workspace = fs::canonicalize(required_path("FORME_WORKER_WORKSPACE_PATH")?)
        .map_err(|_| p::Error("PTY worker workspace cannot be resolved".into()))?;
    let execution: p::WorkerExecutionSpec = read_json(&execution_path)?;
    if execution.backend != p::BackendKind::Pty {
        return Err(p::Error(
            "installed PTY worker received another backend".into(),
        ));
    }
    let p::ActionParameters::Pty(spec) = &execution.intent.parameters else {
        return Err(p::Error(
            "installed PTY worker received invalid parameters".into(),
        ));
    };
    let output_limit = usize::try_from(execution.output_budget_bytes)
        .ok()
        .filter(|limit| *limit > 0 && *limit <= MAX_OUTPUT_BYTES)
        .ok_or_else(|| p::Error("PTY output budget is invalid".into()))?;
    let allowed = allowed_programs();
    let program = absolute_file(&spec.program)?;
    if allowed.is_empty() || !allowed.iter().any(|candidate| candidate == &program) {
        return Err(p::Error(
            "PTY program is outside the installed allowlist".into(),
        ));
    }
    let cwd = absolute_directory(&spec.cwd)?;
    if !cwd.starts_with(&workspace) {
        return Err(p::Error(
            "PTY working directory is outside the worker workspace".into(),
        ));
    }
    let mut frame = Vec::new();
    std::io::stdin()
        .take((MAX_FRAME_BYTES + 1) as u64)
        .read_to_end(&mut frame)
        .map_err(|_| p::Error("PTY input frame cannot be read".into()))?;
    if frame.len() > MAX_FRAME_BYTES {
        return Err(p::Error("PTY input frame exceeds its bound".into()));
    }
    let inputs = decode_worker_input_frame(&frame)?;
    validate_inputs(&inputs, spec)?;
    let stdin_bytes = input_bytes(&inputs, "pty.stdin")?;
    let mut command = CommandBuilder::new(&program);
    command.args(&spec.args);
    command.cwd(&cwd);
    command.env_clear();
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    #[cfg(windows)]
    if let Some(comspec) = std::env::var_os("ComSpec") {
        command.env("ComSpec", comspec);
    }
    command.env("TERM", "dumb");
    command.env("NO_COLOR", "1");
    for binding in &spec.environment {
        let path = format!("pty.environment/{}", binding.name);
        let value = input_bytes(&inputs, &path)?
            .ok_or_else(|| p::Error("PTY environment input is missing".into()))?;
        let value = std::str::from_utf8(value)
            .map_err(|_| p::Error("PTY environment input is not UTF-8".into()))?;
        command.env(&binding.name, value);
    }
    let system = native_pty_system();
    let pair = system
        .openpty(PtySize {
            rows: spec.rows,
            cols: spec.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|_| p::Error("PTY could not be opened".into()))?;
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|_| p::Error("PTY child could not be started".into()))?;
    drop(pair.slave);
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|_| p::Error("PTY writer could not be obtained".into()))?;
    if let Some(bytes) = stdin_bytes {
        writer
            .write_all(bytes)
            .and_then(|()| writer.flush())
            .map_err(|_| p::Error("PTY input could not be written".into()))?;
    }
    drop(writer);
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|_| p::Error("PTY reader could not be cloned".into()))?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let output_overflow = Arc::new(AtomicBool::new(false));
    let output_overflow_reader = Arc::clone(&output_overflow);
    let output_limit_reader = output_limit;
    let reader_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 16 * 1024];
        let result = loop {
            match reader.read(&mut buffer) {
                Ok(0) => break Ok(bytes),
                Ok(read) => {
                    if bytes.len().saturating_add(read) > output_limit_reader {
                        output_overflow_reader.store(true, Ordering::SeqCst);
                        break Ok(bytes);
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof
                    ) || error.raw_os_error() == Some(232) =>
                {
                    // ConPTY closes its read pipe with ERROR_NO_DATA after
                    // the child exits; the bytes already captured remain valid.
                    break Ok(bytes);
                }
                Err(error) => break Err(error),
            }
        };
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + Duration::from_millis(execution.timeout.0);
    let output = loop {
        if let Ok(Some(status)) = child.try_wait() {
            drop(pair.master);
            if output_overflow.load(Ordering::SeqCst) {
                return Err(p::Error("PTY output exceeded its bound".into()));
            }
            let output = receiver
                .recv_timeout(Duration::from_secs(2))
                .ok()
                .and_then(Result::ok)
                .ok_or_else(|| p::Error("PTY output could not be observed".into()))?;
            if !status.success() {
                return Err(p::Error(format!(
                    "PTY child exited unsuccessfully with code {}",
                    status.exit_code()
                )));
            }
            break output;
        }
        if output_overflow.load(Ordering::SeqCst) {
            if child.kill().is_err() || child.wait().is_err() {
                return Err(p::Error("PTY output overflow cleanup is unknown".into()));
            }
            return Err(p::Error("PTY output exceeded its bound".into()));
        }
        if Instant::now() >= deadline {
            if child.kill().is_err() || child.wait().is_err() {
                return Err(p::Error("PTY timeout cleanup is unknown".into()));
            }
            return Err(p::Error(
                "installed PTY worker exceeded its deadline".into(),
            ));
        }
        thread::sleep(Duration::from_millis(5));
    };
    let _ = reader_thread.join();
    let secrets: Vec<Vec<u8>> = inputs
        .iter()
        .filter(|input| input.secret)
        .map(|input| input.bytes.to_vec())
        .filter(|bytes| !bytes.is_empty())
        .collect();
    let output = redact(&output, &secrets, output_limit)?;
    write_result(&result_path, &output)
}

fn input_bytes<'a>(inputs: &'a [WorkerInputFrame], path: &str) -> p::Result<Option<&'a [u8]>> {
    let matches: Vec<_> = inputs
        .iter()
        .filter(|input| input.input_path == path)
        .collect();
    if matches.len() > 1 {
        return Err(p::Error("PTY input path is duplicated".into()));
    }
    Ok(matches.first().map(|input| input.bytes.as_slice()))
}

fn validate_inputs(inputs: &[WorkerInputFrame], spec: &p::PtyActionSpec) -> p::Result<()> {
    for input in inputs {
        if input.input_path == "pty.stdin" {
            if spec.input.is_none() {
                return Err(p::Error("PTY received unexpected stdin input".into()));
            }
            continue;
        }
        let Some(name) = input.input_path.strip_prefix("pty.environment/") else {
            return Err(p::Error("PTY received an input for another backend".into()));
        };
        if !input.secret || !spec.environment.iter().any(|binding| binding.name == name) {
            return Err(p::Error(
                "PTY environment input is not an authorized secret".into(),
            ));
        }
    }
    for binding in &spec.environment {
        let path = format!("pty.environment/{}", binding.name);
        if input_bytes(inputs, &path)?.is_none() {
            return Err(p::Error("PTY environment input is missing".into()));
        }
    }
    Ok(())
}

fn allowed_programs() -> Vec<PathBuf> {
    let mut values = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--allowed-program" {
            if let Some(value) = args.next() {
                if let Ok(path) = absolute_file(&value) {
                    values.push(path);
                }
            }
        }
    }
    values
}

fn redact(mut output: &[u8], secrets: &[Vec<u8>], limit: usize) -> p::Result<Vec<u8>> {
    let mut result = Vec::with_capacity(output.len());
    while !output.is_empty() {
        if let Some(secret) = secrets
            .iter()
            .find(|secret| !secret.is_empty() && output.starts_with(secret))
        {
            if result.len().saturating_add(b"[REDACTED]".len()) > limit {
                return Err(p::Error("redacted PTY output exceeded its bound".into()));
            }
            result.extend_from_slice(b"[REDACTED]");
            output = &output[secret.len()..];
        } else {
            if result.len() == limit {
                return Err(p::Error("redacted PTY output exceeded its bound".into()));
            }
            result.push(output[0]);
            output = &output[1..];
        }
    }
    Ok(result)
}

fn write_result(path: &Path, bytes: &[u8]) -> p::Result<()> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(p::Error("PTY result path is invalid".into()));
    }
    let pending = path.with_extension("pending");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|_| p::Error("PTY result artifact cannot be created".into()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| p::Error("PTY result artifact cannot be written".into()))?;
    fs::rename(&pending, path)
        .map_err(|_| p::Error("PTY result artifact cannot be published".into()))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> p::Result<T> {
    serde_json::from_slice(
        &fs::read(path).map_err(|_| p::Error("PTY worker input file cannot be read".into()))?,
    )
    .map_err(|_| p::Error("PTY worker input file is malformed".into()))
}

fn required_path(name: &str) -> p::Result<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| p::Error(format!("required PTY worker path {name} is missing")))
}

fn absolute_file(value: &str) -> p::Result<PathBuf> {
    let path = PathBuf::from(value);
    if !path.is_absolute() || !path.is_file() {
        return Err(p::Error("PTY program is not a regular file".into()));
    }
    fs::canonicalize(path).map_err(|_| p::Error("PTY program cannot be resolved".into()))
}

fn absolute_directory(value: &str) -> p::Result<PathBuf> {
    let path = PathBuf::from(value);
    if !path.is_absolute() || !path.is_dir() {
        return Err(p::Error("PTY working directory is not a directory".into()));
    }
    fs::canonicalize(path).map_err(|_| p::Error("PTY working directory cannot be resolved".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pty_spec() -> p::PtyActionSpec {
        p::PtyActionSpec {
            schema_version: p::SchemaVersion(1),
            program: if cfg!(windows) {
                r"C:\Windows\System32\cmd.exe".into()
            } else {
                "/usr/bin/python3".into()
            },
            args: Vec::new(),
            cwd: if cfg!(windows) {
                r"C:\workspace".into()
            } else {
                "/workspace".into()
            },
            cols: 80,
            rows: 24,
            input: Some(p::ExternalInput::Content(p::ContentRef(
                "content:test".into(),
            ))),
            environment: vec![p::SecretBinding {
                schema_version: p::SchemaVersion(1),
                name: "TOKEN".into(),
                value: p::SecretRef("secret:test".into()),
            }],
        }
    }

    #[test]
    fn redaction_replaces_secret_bytes_without_changing_other_output() {
        assert_eq!(
            redact(b"before-top-secret-after", &[b"top-secret".to_vec()], 64).unwrap(),
            b"before-[REDACTED]-after"
        );
    }

    #[test]
    fn empty_secret_values_are_not_redacted_as_everything() {
        assert_eq!(redact(b"stable", &[Vec::new()], 64).unwrap(), b"stable");
    }

    #[test]
    fn input_validation_rejects_cross_backend_and_unmarked_environment_values() {
        let spec = pty_spec();
        let cross_backend = vec![WorkerInputFrame {
            input_path: "browser.type".into(),
            secret: false,
            bytes: Vec::new().into(),
        }];
        assert!(validate_inputs(&cross_backend, &spec).is_err());
        let unmarked = vec![
            WorkerInputFrame {
                input_path: "pty.stdin".into(),
                secret: false,
                bytes: b"input".to_vec().into(),
            },
            WorkerInputFrame {
                input_path: "pty.environment/TOKEN".into(),
                secret: false,
                bytes: b"secret".to_vec().into(),
            },
        ];
        assert!(validate_inputs(&unmarked, &spec).is_err());
    }
}
