#![forbid(unsafe_code)]

use std::path::PathBuf;

use forme_eval::M3BArtifactStore;

fn main() {
    let Some(root) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: forme-m3-b-artifact-verify <artifact-root>");
        std::process::exit(2);
    };
    let result = M3BArtifactStore::new(root).and_then(|store| store.verify_complete_set());
    match result {
        Ok(receipt) => {
            println!(
                "[PASS] m3-b-long-horizon {} {}",
                receipt.digest.0, receipt.content_ref.0
            );
            println!("M3-B ARTIFACT VERIFICATION: PASS");
        }
        Err(error) => {
            eprintln!("M3-B ARTIFACT VERIFICATION: FAIL: {error}");
            std::process::exit(1);
        }
    }
}
