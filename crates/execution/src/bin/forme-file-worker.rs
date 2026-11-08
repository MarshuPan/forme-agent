//! One-shot file image. Keep desktop and remote executor libraries unlinked.
#![forbid(unsafe_code)]

// Compile the shared implementation without importing the execution library,
// whose desktop dependencies also carry process-initialization requirements.
#[path = "../installed/file_worker.rs"]
mod file_worker;

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--installed-file-worker"))
        || arguments.next().is_some()
    {
        eprintln!("forme-file-worker: unsupported file entry arguments");
        std::process::exit(2);
    }
    if let Err(error) = file_worker::run_installed_file_worker() {
        eprintln!("forme-file-worker: {}", error.0);
        std::process::exit(1);
    }
}
