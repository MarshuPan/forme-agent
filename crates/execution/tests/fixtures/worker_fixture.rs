#![forbid(unsafe_code)]

use std::{env, fs, thread, time::Duration};

fn main() {
    let workspace = env::current_dir().expect("worker fixture workspace");
    let marker = workspace.join("worker-fixture-ran");
    fs::write(marker, b"worker fixture completed").expect("worker fixture marker");
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
