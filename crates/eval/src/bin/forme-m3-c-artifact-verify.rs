#![forbid(unsafe_code)]

use std::path::PathBuf;

use forme_eval::M3CArtifactStore;

fn main() {
    let Some(root) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: forme-m3-c-artifact-verify <artifact-root>");
        std::process::exit(2);
    };
    let result = M3CArtifactStore::new(root).and_then(|store| store.verify_complete_set());
    match result {
        Ok(receipt) => {
            println!(
                "[PASS] m3-c-governed-golden {} {}",
                receipt.digest.0, receipt.content_ref.0
            );
            println!("M3-C ARTIFACT VERIFICATION: PASS");
        }
        Err(error) => {
            eprintln!("M3-C ARTIFACT VERIFICATION: FAIL: {error}");
            std::process::exit(1);
        }
    }
}
