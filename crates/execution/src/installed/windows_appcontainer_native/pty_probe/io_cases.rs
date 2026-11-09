#![forbid(unsafe_code)]

use super::*;

#[derive(Clone, Copy)]
pub(super) enum Case {
    InputTimeout,
    InputCancel,
    OutputTimeout,
    OutputStrict,
    OutputRejected,
    OutputBurst,
    OutputTruncate,
    ExitError,
}

impl Case {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::InputTimeout => "input-timeout",
            Self::InputCancel => "input-cancel",
            Self::OutputTimeout => "output-timeout",
            Self::OutputStrict => "output-strict",
            Self::OutputRejected => "output-rejected",
            Self::OutputBurst => "output-burst",
            Self::OutputTruncate => "output-truncate",
            Self::ExitError => "exit-error",
        }
    }

    fn terminal(self) -> &'static str {
        match self {
            Self::InputCancel => "ActionCancelled",
            Self::OutputBurst | Self::OutputTruncate => "ActionCompleted",
            _ => "ActionFailed",
        }
    }
}

#[test]
#[ignore = "runs eight fixed real PTY I/O cases in independently observed Low AppContainers"]
fn native_isolated_pty_backend_io_matrix() {
    for case in [
        Case::InputTimeout,
        Case::InputCancel,
        Case::OutputTimeout,
        Case::OutputStrict,
        Case::OutputRejected,
        Case::OutputBurst,
        Case::OutputTruncate,
        Case::ExitError,
    ] {
        experiment(false, true, Some(case));
    }
}

pub(super) fn finish(
    root: &Path,
    worker: &mut SetupResources,
    observed: &[ObservedProcess],
    report: &mut Value,
    deadline: Instant,
    case: Case,
) -> ProbeResult<()> {
    let workspace = root.join("workspace");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(workspace.join("release-io"))?
        .sync_all()?;
    if matches!(case, Case::InputCancel) {
        while !workspace.join("io-body-started.json").is_file() {
            if Instant::now() >= deadline || worker.terminal_state()?.0 {
                return Err("input cancellation never reached the original fixture body".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(workspace.join("cancel-io"))?
            .sync_all()?;
    }
    while worker.terminal_state()? != (true, 0) {
        if Instant::now() >= deadline {
            return Err("isolated terminal I/O escaped the original outer deadline".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let result: Value = serde_json::from_slice(&fs::read(workspace.join("io-result.json"))?)?;
    report["io_result"] = result.clone();
    if worker.exit_code()? != 0
        || result["schema_version"] != 1
        || result["case"] != case.name()
        || result["passed"] != true
        || !workspace.join("io-body-started.json").is_file()
    {
        return Err("isolated backend result or observed fixture body is invalid".into());
    }
    let expected_failure = match case {
        Case::InputTimeout | Case::OutputTimeout => json!("timeout"),
        Case::OutputStrict => json!("output-budget"),
        Case::OutputRejected => json!("append-rejected"),
        Case::ExitError => json!("exit-7"),
        _ => Value::Null,
    };
    let elapsed = result["elapsed_ms"]
        .as_u64()
        .ok_or("missing elapsed time")?;
    if result["failure_kind"] != expected_failure
        || elapsed > 6500
        || (matches!(case, Case::InputTimeout | Case::OutputTimeout) && elapsed < 3500)
    {
        return Err("isolated backend failure cause or elapsed bound differs".into());
    }
    let kinds = result["event_kinds"]
        .as_array()
        .ok_or("missing terminal event evidence")?;
    let terminals = ["ActionCompleted", "ActionCancelled", "ActionFailed"];
    if kinds
        .iter()
        .filter(|kind| **kind == "ActionStarted")
        .count()
        != 1
        || kinds
            .iter()
            .filter(|kind| terminals.iter().any(|terminal| *kind == *terminal))
            .count()
            != 1
        || !kinds.iter().any(|kind| *kind == case.terminal())
        || kinds.iter().any(|kind| *kind == "ActionOutcomeUnknown")
        || !kinds
            .iter()
            .any(|kind| *kind == "CapabilityEvidenceRecorded")
    {
        return Err("isolated backend lifecycle events failed independent validation".into());
    }
    if matches!(case, Case::OutputTruncate)
        && (result["output_bytes"] != 4096 || result["truncated"] != true)
    {
        return Err("isolated terminal truncation did not respect the original budget".into());
    }
    if matches!(case, Case::OutputBurst) && result["tail_marker"] != true {
        return Err("isolated terminal tail marker is absent".into());
    }
    wait_for_observed(observed)?;
    report["original_tree_termination"] = json!("PASS");
    report["terminal_io_lifecycle"] = json!("PASS");
    Ok(())
}
