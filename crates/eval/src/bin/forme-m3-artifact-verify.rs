#![forbid(unsafe_code)]

use std::path::PathBuf;

use forme_eval::M3ArtifactStore;

fn main() {
    if let Err(error) = run() {
        eprintln!("forme-m3-artifact-verify: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: forme-m3-artifact-verify <artifact-directory>")?;
    if args.next().is_some() {
        return Err("usage: forme-m3-artifact-verify <artifact-directory>".into());
    }

    let store = M3ArtifactStore::open(&root)?;
    for receipt in store.verify_complete_set()? {
        println!(
            "[PASS] {} {} {}",
            receipt.kind.as_str(),
            receipt.digest.0,
            receipt.content_ref.0
        );
    }
    println!("M3-A ARTIFACT VERIFICATION: PASS");
    Ok(())
}
