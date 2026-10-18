#![forbid(unsafe_code)]
fn main() {
    match verify() {
        Ok(verdict) => {
            println!("core-brain-d artifact: {verdict:?}");
            if verdict != forme_eval::ProductVerdict::Pass
                && std::env::args().any(|s| s == "--require-pass")
            {
                std::process::exit(2);
            }
        }
        Err(_) => {
            eprintln!("core-brain-d artifact: FAIL (invalid evidence)");
            std::process::exit(1);
        }
    }
}
fn verify() -> Result<forme_eval::ProductVerdict, Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args.len() > 2 || (args.len() == 2 && args[1] != "--require-pass") {
        return Err("artifact path [--require-pass] required".into());
    }
    let path = std::path::PathBuf::from(&args[0]);
    if std::fs::metadata(&path)?.len() > 33_554_432 {
        return Err("oversized artifact".into());
    }
    let a: forme_eval::CoreBrainDArtifact = serde_json::from_slice(&std::fs::read(path)?)?;
    let score = a.verify()?;
    for task in &score.tasks {
        println!(
            "{:?}: quality={}/{}, baseline={}/{}, workflow={}/{}, unverifiable={}",
            task.case,
            task.quality_passes,
            task.quality_trials,
            task.baseline_quality_passes,
            task.baseline_trials,
            task.workflow_passes,
            task.workflow_trials,
            task.unverifiable
        );
    }
    Ok(score.verdict)
}
