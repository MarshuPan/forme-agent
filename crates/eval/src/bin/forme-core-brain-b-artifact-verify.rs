//! Offline verification; no model call or access to the owner's store.
#![forbid(unsafe_code)]
fn main() {
    if let Err(error) = verify() {
        eprintln!("core-brain-b artifact: FAIL: {error}");
        std::process::exit(1);
    }
    println!("core-brain-b artifact: PASS");
}
fn verify() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let path = std::path::PathBuf::from(
        arguments
            .next()
            .ok_or("comparison artifact path required")?,
    );
    if arguments.next().is_some() {
        return Err("exactly one artifact path is required".into());
    }
    if std::fs::metadata(&path)?.len() > 262_144 {
        return Err("comparison artifact exceeds byte bound".into());
    }
    let artifact: forme_protocol::RelevanceComparisonArtifact =
        serde_json::from_slice(&std::fs::read(path)?)?;
    artifact.validate()?;
    Ok(())
}
