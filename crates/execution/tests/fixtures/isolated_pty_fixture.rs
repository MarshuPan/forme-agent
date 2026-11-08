#![forbid(unsafe_code)]

use std::fs;
use std::io::{BufRead, IsTerminal, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};

fn record(name: &str, value: Value) {
    let bytes = serde_json::to_vec(&value).expect("fixture record");
    let temporary = format!("{name}.pending");
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .expect("fresh fixture record");
    file.write_all(&bytes).expect("fixture record write");
    file.sync_all().expect("fixture record flush");
    drop(file);
    fs::rename(temporary, name).expect("fixture record publication");
}

fn environment_is_empty() -> bool {
    [
        "PATH",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "FORME_PROBE_CREDENTIAL",
    ]
    .iter()
    .all(|name| std::env::var_os(name).is_none())
}

fn boundary_observations() -> Value {
    let input_path = std::env::var_os("FORME_WORKER_JOB_PATH")
        .or_else(|| std::env::var_os("FORME_PTY_PROBE_INPUT"))
        .unwrap();
    let input: Value = serde_json::from_slice(&fs::read(input_path).unwrap()).unwrap();
    let denied: Vec<bool> = input["deny_paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| fs::read(path.as_str().unwrap()).is_err())
        .collect();
    let address: SocketAddr = input["listener"].as_str().unwrap().parse().unwrap();
    let network_denied = TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_err();
    json!({"environment_empty": environment_is_empty(), "files_denied": denied, "network_denied": network_denied})
}

fn terminal_child(tree: bool) -> Result<(), String> {
    let grandchild = if tree {
        let mut command = Command::new(std::env::current_exe().map_err(|_| "child image")?);
        command.arg("--grandchild").env_clear();
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        command.env(
            "FORME_PTY_PROBE_INPUT",
            std::env::var_os("FORME_PTY_PROBE_INPUT").unwrap(),
        );
        Some(
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "grandchild spawn")?,
        )
    } else {
        None
    };
    record(
        "child-ready.json",
        json!({
            "pid": std::process::id(),
            "grandchild": grandchild.as_ref().map(std::process::Child::id),
            "stdin_terminal": std::io::stdin().is_terminal(),
            "stdout_terminal": std::io::stdout().is_terminal(),
            "boundary": boundary_observations(),
        }),
    );
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|_| "terminal input")?;
    println!("FORME-PTY-ECHO:{}", line.trim_end());
    std::io::stdout().flush().map_err(|_| "terminal output")?;
    while !std::path::Path::new("release-child").exists() {
        std::thread::sleep(Duration::from_millis(10));
    }
    if let Some(mut grandchild) = grandchild {
        let _ = grandchild.kill();
        let _ = grandchild.wait();
    }
    Ok(())
}

fn runner(tree: bool) -> Result<(), String> {
    record(
        "runner-started.json",
        json!({
            "pid": std::process::id(), "boundary": boundary_observations(),
        }),
    );
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("openpty: {error}"))?;
    record("pty-opened.json", json!({"rows": 24, "cols": 80}));
    let image = std::env::current_exe().map_err(|_| "runner image")?;
    let mut command = CommandBuilder::new(&image);
    command.arg(if tree { "--child-tree" } else { "--child" });
    command.cwd(std::env::current_dir().map_err(|_| "runner directory")?);
    command.env_clear();
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    command.env(
        "FORME_PTY_PROBE_INPUT",
        std::env::var_os("FORME_WORKER_JOB_PATH").unwrap(),
    );
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| format!("terminal child: {error}"))?;
    drop(pair.slave);
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|_| "terminal reader")?;
    let mut writer = pair.master.take_writer().map_err(|_| "terminal writer")?;
    let (sender, receiver) = mpsc::sync_channel(8);
    let _reader = std::thread::spawn(move || {
        let mut bytes = [0u8; 2048];
        while let Ok(length) = reader.read(&mut bytes) {
            if length == 0 || sender.send(bytes[..length].to_vec()).is_err() {
                break;
            }
        }
    });
    writer
        .write_all(b"forme-isolated-terminal\r\n")
        .map_err(|_| "terminal input")?;
    writer.flush().map_err(|_| "terminal input flush")?;
    let deadline = Instant::now() + Duration::from_secs(7);
    let mut output = Vec::new();
    let mut cursor_requests = 0;
    let mut roundtrip = false;
    loop {
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Err("fixture terminal deadline".into());
        }
        if let Ok(bytes) = receiver.recv_timeout(Duration::from_millis(10)) {
            if output.len() + bytes.len() > 16 * 1024 {
                let _ = child.kill();
                return Err("fixture terminal output bound".into());
            }
            output.extend_from_slice(&bytes);
            let count = output
                .windows(4)
                .filter(|bytes| *bytes == b"\x1b[6n")
                .count();
            for _ in cursor_requests..count {
                writer
                    .write_all(b"\x1b[1;1R")
                    .map_err(|_| "terminal cursor reply")?;
                writer.flush().map_err(|_| "terminal cursor flush")?;
            }
            cursor_requests = count;
            const ECHO: &[u8] = b"FORME-PTY-ECHO:forme-isolated-terminal";
            if !roundtrip && output.windows(ECHO.len()).any(|bytes| bytes == ECHO) {
                record(
                    "roundtrip.json",
                    json!({"matched": true, "bytes": output.len()}),
                );
                roundtrip = true;
            }
        }
        if let Some(status) = child.try_wait().map_err(|_| "terminal wait")? {
            record(
                "runner-exited.json",
                json!({"success": status.success(), "roundtrip": roundtrip}),
            );
            return if status.success() && roundtrip {
                Ok(())
            } else {
                Err("terminal exit".into())
            };
        }
    }
}

fn main() {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--file-policy-control") => {
            match Command::new(std::env::current_exe().unwrap())
                .arg("--grandchild")
                .env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(mut child) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    Err("file policy allowed a forbidden child".into())
                }
                Err(error) => {
                    record(
                        "file-policy-denial.json",
                        json!({"win32": error.raw_os_error()}),
                    );
                    Ok(())
                }
            }
        }
        Some("--runner") => runner(false),
        Some("--runner-tree") => runner(true),
        Some("--child") => terminal_child(false),
        Some("--child-tree") => terminal_child(true),
        Some("--grandchild") => {
            record(
                "grandchild-ready.json",
                json!({"pid": std::process::id(), "boundary": boundary_observations()}),
            );
            std::thread::sleep(Duration::from_secs(30));
            Ok(())
        }
        _ => Err("fixed fixture mode is required".into()),
    };
    if let Err(error) = result {
        let directory = std::env::current_dir().unwrap_or_default();
        let image = std::env::current_exe().unwrap_or_default();
        let error = error
            .replace(directory.to_string_lossy().as_ref(), "<probe-workspace>")
            .replace(image.to_string_lossy().as_ref(), "<probe-fixture>");
        record("fixture-failure.json", json!({"error": error}));
        std::process::exit(2);
    }
}
