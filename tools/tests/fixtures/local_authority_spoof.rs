//! Repository-owned negative fixture; it must never be invoked by hostctl.
#![forbid(unsafe_code)]

fn main() {
    if let Some(path) = std::env::var_os("FORME_AUTHORITY_SPOOF_MARKER") {
        std::fs::write(path, b"spoof process was invoked").unwrap();
    }
    #[cfg(windows)]
    println!("S-1-5-32-544 Group used for deny only\nAnother group Enabled");
    #[cfg(not(windows))]
    println!("0");
}
