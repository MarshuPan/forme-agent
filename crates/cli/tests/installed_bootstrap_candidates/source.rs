//! Build a historical source through the real CLI/runtime and a local model.
use super::*;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub(super) fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
        }
    }
}

pub(super) fn seed(f: &Fixture) -> PathBuf {
    let source = f.install.paths.backups.join("historical-brains/old-brain");
    fs::create_dir_all(&source).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let model = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if stopped.load(Ordering::SeqCst) {
                        return false;
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "CLI fixture made no model call"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("fixture accept: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        read_request(&mut stream);
        let body = r#"{"choices":[{"message":{"content":"historical source answer"},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":3}}"#;
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        true
    });
    let mut command = Command::new(env!("CARGO_BIN_EXE_forme-cli"));
    command.env_clear();
    platform_environment(&mut command, &f.root);
    #[cfg(windows)]
    {
        // Creation of the fixture's own vault invokes Windows ACL tooling.
        // Pass only its OS identity and fixed system tools, never user secrets.
        let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        command.env("USERNAME", std::env::var_os("USERNAME").unwrap());
        command.env("PATH", system.join("System32"));
    }
    let output = command
        .args(["--embedded", "Record a historical source fixture"])
        .env("FORME_MODEL_BASE_URL", format!("http://{address}/v1"))
        .env("FORME_MODEL_NAME", "bootstrap-fixture")
        .env("FORME_MODEL_API_KEY", "bootstrap-local-fixture-key")
        .env("FORME_MODEL_TIMEOUT_MS", "5000")
        .env("FORME_STORE_PATH", source.join("forme.db"))
        .env("FORME_ARTIFACT_ROOT", source.join("artifacts"))
        .env("FORME_OWNER_ID", "owner:historical-cli")
        .env("FORME_SCHEDULER_ENABLED", "false")
        .env("FORME_COGNITIVE_TICK_ENABLED", "false")
        .current_dir(&f.root)
        .output()
        .unwrap();
    stop.store(true, Ordering::SeqCst);
    let called = model.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(called, "CLI fixture made no model call");
    assert!(String::from_utf8_lossy(&output.stdout).contains("historical source answer"));
    fs::create_dir_all(source.join("artifacts")).unwrap();
    let core = serde_json::json!({
        "schema_version":1,"install_ref":"installation:historical-fixture","revision":1,
        "brain": {"schema_version":1,"owner":"owner:historical-cli",
            "workspace":"workspace:local","store_path":source.join("forme.db"),
            "artifact_root":source.join("artifacts"), "agent_profile":null,
            "models":null,"scheduler_enabled":null,"scheduler":null,
            "cognition":null,"authority_lease":null},
        "transport":null,"drain_timeout_ms":null
    });
    fs::write(source.join("core.json"), serde_json::to_vec(&core).unwrap()).unwrap();
    source
}

fn read_request(stream: &mut TcpStream) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0 && bytes.len() + count <= 1024 * 1024);
        bytes.extend_from_slice(&buffer[..count]);
        let Some(offset) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let headers = std::str::from_utf8(&bytes[..offset]).unwrap();
        let length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(str::trim)
                    .map(|value| value.parse::<usize>().unwrap())
            })
            .unwrap();
        if bytes.len() >= offset + 4 + length {
            break;
        }
    }
}
