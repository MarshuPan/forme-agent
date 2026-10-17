//! Offline C-wave artifact verification without credentials or owner-store access.
#![forbid(unsafe_code)]
fn main() {
    if let Err(error) = verify() {
        eprintln!("core-brain-c artifact: FAIL: {error}");
        std::process::exit(1)
    }
    println!("core-brain-c artifact: PASS");
}
fn verify() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = std::path::PathBuf::from(args.next().ok_or("artifact path required")?);
    if args.next().is_some() || std::fs::metadata(&path)?.len() > 8_388_608 {
        return Err("invalid argument or oversized artifact".into());
    }
    let a: forme_eval::CoreBrainCArtifact = serde_json::from_slice(&std::fs::read(path)?)?;
    let ledger = a.validate()?;
    println!(
        "case={:?}; routes={}; streams={}; witnesses={}; evidence={}",
        a.case,
        ledger.routes.len(),
        a.stream_heads.len(),
        a.witnesses.len(),
        a.evidence_grade
    );
    Ok(())
}
