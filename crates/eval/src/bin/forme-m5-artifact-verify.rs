#![forbid(unsafe_code)]

use std::path::PathBuf;

use forme_eval::M5ArtifactStore;

fn main() {
    if let Err(error) = run() {
        eprintln!("forme-m5-artifact-verify: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: forme-m5-artifact-verify <artifact-directory>")?;
    if args.next().is_some() {
        return Err("usage: forme-m5-artifact-verify <artifact-directory>".into());
    }
    let receipt = M5ArtifactStore::new(root)?.verify_complete_set()?;
    println!("{}", receipt.digest.0);
    Ok(())
}
