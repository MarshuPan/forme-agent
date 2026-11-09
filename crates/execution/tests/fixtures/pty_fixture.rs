#![forbid(unsafe_code)]

use std::io::{BufRead, Write};
use std::time::Duration;

fn main() {
    if std::env::var("FORME_PTY_FIXTURE_CHILD").as_deref() != Ok("1") {
        std::process::exit(2);
    }
    if let Some(mode) = std::env::args()
        .nth(1)
        .filter(|mode| mode.starts_with("--io-"))
    {
        io_fault(&mode);
        return;
    }
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() == Some(std::ffi::OsStr::new("--effect-marker")) {
        let Some(path) = arguments.next() else {
            std::process::exit(4);
        };
        let Ok(mut marker) = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        else {
            std::process::exit(4);
        };
        if marker.write_all(b"original PTY fixture executed").is_err() {
            std::process::exit(4);
        }
    }
    let mut input = String::new();
    if std::io::stdin().lock().read_line(&mut input).is_err() {
        std::process::exit(3);
    }
    let secret = std::env::var("FIXTURE_TOKEN").ok();
    let parent_path_absent = std::env::var_os("PATH").is_none();
    let mut output = std::io::stdout().lock();
    let _ = writeln!(output, "{}", input.trim());
    let _ = writeln!(
        output,
        "{}",
        if secret.is_some() {
            "secret-ref-resolved"
        } else {
            "secret-ref-missing"
        }
    );
    if let Some(secret) = secret {
        let split = secret.len() / 2;
        let _ = write!(output, "echo:{}", &secret[..split]);
        let _ = output.flush();
        std::thread::sleep(Duration::from_millis(30));
        let _ = writeln!(output, "{}", &secret[split..]);
    }
    if std::env::args().any(|argument| argument == "--wait") {
        let _ = writeln!(output, "waiting-for-cancel-or-timeout");
        let _ = output.flush();
        std::thread::sleep(Duration::from_secs(30));
    }
    let _ = writeln!(
        output,
        "{}",
        if parent_path_absent {
            "parent-env-cleared"
        } else {
            "parent-env-leaked"
        }
    );
}

fn io_fault(mode: &str) {
    let root = std::path::PathBuf::from(std::env::args_os().nth(2).unwrap());
    std::fs::write(root.join("fixture.pid"), std::process::id().to_string()).unwrap();
    if mode == "--io-unread" {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    if mode == "--io-error" {
        std::process::exit(7);
    }
    let mut output = std::io::stdout().lock();
    let chunk = [b'x'; 4096];
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let mut written = 0;
    while std::time::Instant::now() < deadline {
        if output
            .write_all(&chunk)
            .and_then(|()| output.flush())
            .is_err()
        {
            break;
        }
        written += chunk.len();
        if mode == "--io-burst" && written >= 128 * 1024 {
            let _ = output.write_all(b"\r\nFORME-PTY-IO-END\r\n");
            let _ = output.flush();
            break;
        }
    }
}
