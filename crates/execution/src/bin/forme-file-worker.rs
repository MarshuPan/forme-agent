//! One-shot file image. Keep desktop and remote executor libraries unlinked.
#![forbid(unsafe_code)]

// Compile the shared implementation without importing the execution library,
// whose desktop dependencies also carry process-initialization requirements.
#[path = "../installed/file_worker.rs"]
mod file_worker;

#[path = "../installed/channel_worker.rs"]
mod channel_worker;

fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let result = match arguments.as_slice() {
        [mode] if mode == "--installed-file-worker" => file_worker::run_installed_file_worker(),
        [mode] if mode == "--installed-channel-worker" => channel_worker::run(),
        _ => {
            eprintln!("forme-file-worker: unsupported file entry arguments");
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("forme-file-worker: {}", error.0);
        std::process::exit(1);
    }
}
