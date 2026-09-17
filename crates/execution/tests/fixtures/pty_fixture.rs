#![forbid(unsafe_code)]

use std::io::{BufRead, Write};
use std::time::Duration;

fn main() {
    if std::env::var("FORME_PTY_FIXTURE_CHILD").as_deref() != Ok("1") {
        std::process::exit(2);
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
