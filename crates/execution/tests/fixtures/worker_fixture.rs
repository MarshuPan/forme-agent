#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::process::Command;
use std::{env, fs, thread, time::Duration};

fn main() {
    if env::args().any(|arg| arg == "--noop") {
        return;
    }
    let workspace = env::current_dir().expect("worker fixture workspace");
    if env::args().any(|arg| arg == "--read-input") {
        let mut input = Vec::new();
        std::io::stdin()
            .read_to_end(&mut input)
            .expect("worker input frame");
        fs::write(workspace.join("worker-input-frame.bin"), input)
            .expect("worker input frame write");
    }
    if env::args().any(|arg| arg == "--isolation-probe") {
        isolation_probe(&workspace);
        write_result();
        return;
    }
    let marker = workspace.join("worker-fixture-ran");
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(marker)
        .expect("worker fixture marker");
    file.write_all(b"worker fixture completed\n")
        .expect("worker fixture append");
    file.sync_all().expect("worker fixture durable marker");
    write_result();
    let descendant = workspace.join("worker-fixture-descendant-ran");
    if env::args().any(|arg| arg == "--descendant") {
        thread::sleep(Duration::from_millis(750));
        let mut file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(descendant)
            .expect("worker descendant marker");
        file.write_all(b"worker descendant completed\n")
            .expect("worker descendant marker write");
        file.sync_all().expect("worker descendant marker durable");
        return;
    }
    if env::args().any(|arg| arg == "--spawn-descendant") {
        let mut child = Command::new(env::current_exe().expect("worker fixture executable"))
            .args(["--descendant", "--sleep-ms=750"])
            .spawn()
            .expect("worker fixture descendant");
        fs::write(
            workspace.join("worker-fixture-descendant.pid"),
            child.id().to_string(),
        )
        .expect("worker descendant pid");
        let _ = child.wait();
    }
    let sleep_ms = env::args()
        .skip(1)
        .find_map(|arg| {
            arg.strip_prefix("--sleep-ms=")
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(0_u64);
    if sleep_ms > 0 {
        thread::sleep(Duration::from_millis(sleep_ms));
    }
    if env::args().any(|arg| arg == "--fail") {
        std::process::exit(17);
    }
}

fn write_result() {
    if env::args().any(|arg| arg == "--no-result" || arg == "--descendant") {
        return;
    }
    if let Some(path) = env::var_os("FORME_WORKER_RESULT_PATH") {
        let mut result = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("worker result artifact");
        result
            .write_all(b"worker fixture result\n")
            .expect("worker result write");
        result.sync_all().expect("worker result durable");
    }
}

fn isolation_probe(workspace: &std::path::Path) {
    use std::net::{SocketAddr, TcpStream};
    fs::write(workspace.join("probe-stage"), b"started")
        .expect("private copy can record probe stage");
    let input: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join("isolation-probe-input.json")).expect("probe input"),
    )
    .expect("probe JSON");
    fs::write(workspace.join("probe-stage"), b"input-read").unwrap();
    let paths = input["deny_paths"].as_array().expect("probe paths");
    let denied = paths.iter().all(|value| {
        let path = std::path::Path::new(value.as_str().expect("probe path"));
        fs::File::open(path).is_err() && fs::OpenOptions::new().write(true).open(path).is_err()
    });
    let input_readonly = [
        "FORME_WORKER_JOB_PATH",
        "FORME_WORKER_PERMIT_PATH",
        "FORME_WORKER_EXECUTION_PATH",
    ]
    .iter()
    .all(|name| {
        let path = env::var_os(name).expect("probe bound input");
        fs::File::open(&path).is_ok() && fs::OpenOptions::new().write(true).open(&path).is_err()
    });
    let job_path = std::path::PathBuf::from(env::var_os("FORME_WORKER_JOB_PATH").unwrap());
    let job_dir = job_path.parent().unwrap();
    let host_records_denied = [
        job_dir.join("overlay-baseline.json"),
        job_dir.parent().unwrap().join("sandbox-leases.jsonl"),
    ]
    .iter()
    .all(|path| {
        fs::File::open(path).is_err() && fs::OpenOptions::new().write(true).open(path).is_err()
    });
    let existing = fs::read(workspace.join("existing")).expect("copied file is readable");
    fs::write(workspace.join("probe-stage"), b"copy-read").unwrap();
    fs::write(workspace.join("existing"), b"sandbox result").expect("private copy is writable");
    let program = env::current_exe().expect("registered probe program");
    fs::write(workspace.join("probe-stage"), b"copy-written").unwrap();
    let child_denied = Command::new(&program).arg("--noop").spawn().is_err();
    let copied = workspace.join("probe-copy.exe");
    fs::copy(&program, &copied).expect("private program copy");
    fs::write(workspace.join("probe-stage"), b"program-copied").unwrap();
    let writable_program_denied = Command::new(&copied).arg("--noop").spawn().is_err();
    fs::remove_file(copied).expect("private program cleanup");
    let port = input["loopback_port"].as_u64().expect("probe port") as u16;
    let address: SocketAddr = format!("127.0.0.1:{port}").parse().expect("probe address");
    let network_denied = TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_err();
    let external: SocketAddr = input["external_tcp"]
        .as_str()
        .expect("external TCP control")
        .parse()
        .expect("external probe address");
    let external_network_denied =
        TcpStream::connect_timeout(&external, Duration::from_millis(500)).is_err();
    let environment_closed = env::vars_os().all(|(name, _)| {
        name.to_str().is_some_and(|name| {
            matches!(
                name,
                "LOCALAPPDATA"
                    | "TEMP"
                    | "TMP"
                    | "SystemRoot"
                    | "FORME_WORKER_PROTOCOL_VERSION"
                    | "FORME_WORKER_JOB_PATH"
                    | "FORME_WORKER_PERMIT_PATH"
                    | "FORME_WORKER_EXECUTION_PATH"
                    | "FORME_WORKER_RESULT_PATH"
                    | "FORME_WORKER_WORKSPACE_PATH"
                    | "FORME_WORKER_WORKSPACE_SCOPE"
                    | "FORME_WORKER_DEADLINE"
            )
        })
    });
    let valid = denied
        && input_readonly
        && host_records_denied
        && child_denied
        && writable_program_denied
        && network_denied
        && external_network_denied
        && environment_closed
        && existing == b"before";
    let report = serde_json::json!({ "schema_version": 1, "denied_paths": denied,
        "inputs_readonly": input_readonly, "host_records_denied": host_records_denied, "child_denied": child_denied,
        "writable_program_denied": writable_program_denied, "network_denied": network_denied,
        "external_network_denied": external_network_denied,
        "environment_closed": environment_closed, "copy_read_write": existing == b"before" });
    fs::write(
        workspace.join("isolation-probe-result.json"),
        serde_json::to_vec(&report).expect("probe report"),
    )
    .expect("probe report write");
    if !valid {
        std::process::exit(17);
    }
}
