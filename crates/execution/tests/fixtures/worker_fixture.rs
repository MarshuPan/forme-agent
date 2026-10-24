#![forbid(unsafe_code)]

use std::io::Write;
use std::process::Command;
use std::{env, fs, thread, time::Duration};

fn main() {
    let workspace = env::current_dir().expect("worker fixture workspace");
    let marker = workspace.join("worker-fixture-ran");
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(marker)
        .expect("worker fixture marker");
    file.write_all(b"worker fixture completed\n")
        .expect("worker fixture append");
    file.sync_all().expect("worker fixture durable marker");
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
