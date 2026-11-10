#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use forme_protocol as p;

type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;
use serde_json::{json, Value};

use super::super::process::WorkerLaunchSpec;
use super::super::sandbox_lease::{
    SandboxAclMutation, SandboxAclOperation, SandboxRecoveryStage, SandboxTargetAccess,
};
use super::super::windows_appcontainer_acl as acl;
use super::runtime::ObservedProcess;
use super::{LaunchRequest, SetupResources};

mod io_cases;

#[test]
#[ignore = "creates a private Low AppContainer and real ConPTY; explicit mechanism evidence only"]
fn native_isolated_pty_roundtrip_and_exit() {
    experiment(false, true, None);
}

#[test]
#[ignore = "creates a private Low AppContainer and cancels its actual terminal process tree"]
fn native_isolated_pty_descendant_cancellation() {
    experiment(true, true, None);
}

#[test]
#[ignore = "independently verifies production child restriction with the fixed PTY fixture"]
fn native_file_policy_still_rejects_terminal_children() {
    experiment(false, false, None);
}

fn durable(path: &Path, value: &Value) -> ProbeResult<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    Ok(())
}

fn journal(file: &mut File, value: &Value) -> ProbeResult<()> {
    file.write_all(&serde_json::to_vec(value)?)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn fixture() -> PathBuf {
    let test = std::env::current_exe().unwrap();
    let debug = test.parent().unwrap().parent().unwrap();
    let fixture = debug.join("forme-isolated-pty-fixture.exe");
    assert!(
        fixture.is_file(),
        "build the fixed forme-isolated-pty-fixture binary first"
    );
    fixture
}

fn descendants(root: u32) -> ProbeResult<Vec<Value>> {
    let script = format!(
        "$ErrorActionPreference='Stop'; $rows=@(Get-CimInstance Win32_Process); \
         $ids=@({root}); do {{ $added=@($rows | Where-Object {{ \
         $ids -contains $_.ParentProcessId -and $ids -notcontains $_.ProcessId }} | \
         Select-Object -ExpandProperty ProcessId); $ids += $added }} while ($added.Count -gt 0); \
         ConvertTo-Json -Compress -InputObject @($rows | Where-Object {{ \
         $ids -contains $_.ProcessId }} | Select-Object ProcessId,ParentProcessId,Name)"
    );
    let program =
        super::system_windows_directory()?.join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut child = Command::new(program)
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err(p::Error("independent process inventory timed out".into()).into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() || output.stdout.len() > 16 * 1024 {
        return Err(
            p::Error("independent process inventory failed or exceeded its bound".into()).into(),
        );
    }
    let rows: Vec<Value> = serde_json::from_slice(&output.stdout)?;
    if rows.is_empty() || rows.len() > 32 {
        return Err(p::Error("independent process inventory is empty or unbounded".into()).into());
    }
    Ok(rows)
}

fn grant(
    path: &Path,
    access: SandboxTargetAccess,
    worker: &SetupResources,
    grants: &mut Vec<SandboxAclMutation>,
    lease: &mut File,
) -> ProbeResult<()> {
    let profile = worker.profile().unwrap();
    let mutation = SandboxAclMutation {
        schema_version: p::SchemaVersion(1),
        lease_id: profile.name().into(),
        operation_id: format!("probe-grant-{}", grants.len()),
        job_id: profile.name().into(),
        plan_digest: p::canonical_digest(&("isolated-pty-mechanism", profile.name()))?,
        appcontainer_identity: profile.name().into(),
        operation: SandboxAclOperation::Grant,
        recovery_stage: SandboxRecoveryStage::GrantAclPending,
        target_path: fs::canonicalize(path)?.to_string_lossy().into_owned(),
        target_file_identity: acl::file_identity(path)?,
        target_access: access,
    };
    journal(lease, &json!({"intent": mutation}))?;
    grants.push(mutation);
    let result = acl::apply_acl(grants.last().unwrap(), profile)?;
    journal(lease, &json!({"readback": result}))
}

fn assert_boundaries(observation: &Value) -> ProbeResult<()> {
    if observation["environment_empty"] != true
        || observation["network_denied"] != true
        || observation["files_denied"] != json!([true, true])
    {
        return Err(p::Error("fixture file, network or environment boundary failed".into()).into());
    }
    Ok(())
}

fn wait_for_observed(observed: &[ObservedProcess]) -> ProbeResult<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut exited = true;
        for process in observed {
            exited &= process.exited()?;
        }
        if exited {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(
                "an original observed process did not exit within the cleanup bound".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn run_probe(
    root: &Path,
    worker: &mut SetupResources,
    tree: bool,
    allow_terminal: bool,
    observed: &mut Vec<ObservedProcess>,
    report: &mut Value,
    io_case: Option<io_cases::Case>,
) -> ProbeResult<()> {
    worker.verify_resume()?;
    report["pre_resume_token_and_single_job_root"] = json!("PASS");
    let (root_created, root_exited, root_image) = worker.root_identity()?;
    if root_exited && allow_terminal {
        report["root_exit_code"] = json!(worker.exit_code()?);
        return Err(p::Error("fixture root exited before startup observation".into()).into());
    }
    let workspace = root.join("workspace");
    let deadline = Instant::now() + Duration::from_secs(10);
    if !allow_terminal {
        while worker.terminal_state()? != (true, 0) {
            if Instant::now() >= deadline {
                return Err("file-policy control did not exit within the bound".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let denial: Value =
            serde_json::from_slice(&fs::read(workspace.join("file-policy-denial.json"))?)?;
        report["file_policy_denial"] = denial.clone();
        if denial["win32"] != 367
            || worker.exit_code()? != 0
            || workspace.join("grandchild-ready.json").exists()
        {
            return Err("file-policy control did not prove ERROR_CHILD_PROCESS_BLOCKED".into());
        }
        report["production_child_restriction"] = json!("PASS");
        return Ok(());
    }
    loop {
        if workspace.join("fixture-failure.json").is_file() {
            report["fixture_failure"] =
                serde_json::from_slice(&fs::read(workspace.join("fixture-failure.json"))?)?;
            return Err(
                p::Error("restricted runner could not establish its real terminal".into()).into(),
            );
        }
        if workspace.join("child-ready.json").is_file()
            && (!tree || workspace.join("grandchild-ready.json").is_file())
        {
            break;
        }
        if worker.terminal_state()?.0 {
            report["root_exit_code"] = json!(worker.exit_code()?);
            return Err(
                p::Error("restricted runner exited before terminal evidence".into()).into(),
            );
        }
        if Instant::now() >= deadline {
            return Err(p::Error("independent terminal observation deadline".into()).into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let runner: Value = serde_json::from_slice(&fs::read(workspace.join("runner-started.json"))?)?;
    let child: Value = serde_json::from_slice(&fs::read(workspace.join("child-ready.json"))?)?;
    assert_boundaries(&runner["boundary"])?;
    assert_boundaries(&child["boundary"])?;
    if child["stdin_terminal"] != true || child["stdout_terminal"] != true {
        return Err(p::Error("child streams are not a real terminal".into()).into());
    }
    report["terminal_streams"] = json!("PASS");
    if workspace.join("roundtrip.json").is_file() {
        report["terminal_and_roundtrip"] = json!("PASS");
    }
    let root_pid = runner["pid"]
        .as_u64()
        .ok_or_else(|| p::Error("fixture root PID missing".into()))? as u32;
    let rows = descendants(root_pid)?;
    report["os_process_inventory"] = json!(rows);
    let expected_child = child["pid"].as_u64().unwrap() as u32;
    for pid in [root_pid, expected_child] {
        if !rows
            .iter()
            .any(|row| row["ProcessId"].as_u64() == Some(pid as u64))
        {
            return Err(
                p::Error("fixture PID absent from independent process inventory".into()).into(),
            );
        }
    }
    if tree {
        let grandchild: Value =
            serde_json::from_slice(&fs::read(workspace.join("grandchild-ready.json"))?)?;
        assert_boundaries(&grandchild["boundary"])?;
        if grandchild["pid"] != child["grandchild"] {
            return Err(p::Error("grandchild binding differs".into()).into());
        }
        if !rows.iter().any(|row| row["ProcessId"] == grandchild["pid"]) {
            return Err(
                p::Error("grandchild absent from independent process inventory".into()).into(),
            );
        }
    }
    for row in &rows {
        let pid = row["ProcessId"].as_u64().unwrap() as u32;
        let process = match worker.observe_process(pid) {
            Ok(process) => process,
            Err(error) => {
                report["failed_process_observation"] = json!({
                    "pid": pid, "image_name": row["Name"], "error": error.to_string(),
                });
                return Err(error.into());
            }
        };
        let image = fs::canonicalize(&process.image)?;
        let fixed_fixture = fs::canonicalize(root.join("program/probe.exe"))?;
        let fixed_terminal =
            fs::canonicalize(super::system_windows_directory()?.join("System32/conhost.exe"))?;
        if image != fixed_fixture && image != fixed_terminal {
            return Err(p::Error("unexpected image in terminal process tree".into()).into());
        }
        if pid == root_pid && (process.created != root_created || process.image != root_image) {
            return Err(p::Error(
                "reported root does not match the original process handle".into(),
            )
            .into());
        }
        observed.push(process);
    }
    if worker.terminal_state()?.1 as usize != observed.len() {
        return Err(
            p::Error("Job accounting differs from independent terminal inventory".into()).into(),
        );
    }
    if !rows.iter().any(|row| {
        row["Name"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case("conhost.exe"))
    }) {
        report["terminal_host_isolation"] = json!("UNVERIFIABLE");
        return Err(
            p::Error("terminal host absent from independent process inventory".into()).into(),
        );
    }
    report["all_observed_tokens_and_job_membership"] = json!("PASS");
    report["terminal_host_isolation"] = json!("PASS");
    if let Some(case) = io_case {
        return io_cases::finish(root, worker, observed, report, deadline, case);
    }
    while !workspace.join("roundtrip.json").is_file() {
        if Instant::now() >= deadline || worker.terminal_state()?.0 {
            return Err("terminal roundtrip did not complete within the bound".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    report["terminal_and_roundtrip"] = json!("PASS");
    if tree {
        worker.terminate()?;
    } else {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(workspace.join("release-child"))?
            .sync_all()?;
        while worker.terminal_state()? != (true, 0) {
            if Instant::now() >= deadline {
                return Err(p::Error(
                    "normal terminal exit did not complete within the bound".into(),
                )
                .into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if worker.exit_code()? != 0 {
            return Err(p::Error("normal terminal root returned failure".into()).into());
        }
    }
    wait_for_observed(observed)?;
    report["original_tree_termination"] = json!("PASS");
    Ok(())
}

fn experiment(tree: bool, allow_terminal: bool, io_case: Option<io_cases::Case>) {
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/closure-inventory-evidence");
    fs::create_dir_all(&evidence).unwrap();
    let root = tempfile::Builder::new()
        .prefix("isolated-pty-")
        .tempdir_in(evidence)
        .unwrap()
        .keep();
    let workspace = root.join("workspace");
    for directory in [
        &workspace,
        &root.join("program"),
        &root.join("inputs"),
        &root.join("sibling"),
        &root.join("brain-like"),
    ] {
        fs::create_dir(directory).unwrap();
    }
    let source = fixture();
    let program = root.join("program/probe.exe");
    fs::copy(&source, &program).unwrap();
    let sentinel_paths = [
        root.join("sibling/sentinel"),
        root.join("brain-like/sentinel"),
    ];
    for path in &sentinel_paths {
        fs::write(path, b"private fixture sentinel").unwrap();
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let control = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    drop(listener.accept().unwrap());
    drop(control);
    listener.set_nonblocking(true).unwrap();
    let input = root.join("inputs/probe-input.json");
    durable(&input, &json!({"deny_paths": sentinel_paths, "listener": listener.local_addr().unwrap().to_string()})).unwrap();
    let permit = root.join("inputs/permit.json");
    durable(
        &permit,
        &json!({"schema_version": 1, "purpose": "component-mechanism-only"}),
    )
    .unwrap();
    let profile_name = format!("forme-job-{}", root.file_name().unwrap().to_string_lossy());
    let mut worker = SetupResources::new();
    let mut grants = Vec::new();
    let mut observed = Vec::new();
    let mut lease = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("probe-lease.jsonl"))
        .unwrap();
    let mut report = json!({
        "schema_version": 1, "tree_case": tree, "test_only_child_permission": allow_terminal,
        "io_case": io_case.map(io_cases::Case::name),
        "fixture_digest": p::sha256_content_digest(&fs::read(&source).unwrap()),
        "terminal_and_roundtrip": "NOT_RUN", "terminal_host_isolation": "NOT_RUN",
        "all_observed_tokens_and_job_membership": "NOT_RUN", "original_tree_termination": "NOT_RUN",
        "installed_acceptance": "NOT_RUN",
    });
    let result = (|| -> ProbeResult<()> {
        worker.create_profile(&profile_name)?;
        journal(&mut lease, &json!({"profile_created": profile_name}))?;
        acl::set_low_integrity(&workspace)?;
        grant(
            &workspace,
            SandboxTargetAccess::WorkspaceReadWrite,
            &worker,
            &mut grants,
            &mut lease,
        )?;
        grant(
            &program,
            SandboxTargetAccess::RegisteredProgramReadExecute,
            &worker,
            &mut grants,
            &mut lease,
        )?;
        grant(
            &input,
            SandboxTargetAccess::JobInputReadOnly,
            &worker,
            &mut grants,
            &mut lease,
        )?;
        grant(
            &permit,
            SandboxTargetAccess::JobInputReadOnly,
            &worker,
            &mut grants,
            &mut lease,
        )?;
        worker.create_job(&format!("Local\\ForMePtyProbe-{profile_name}"))?;
        let mut arguments = vec![if !allow_terminal {
            "--file-policy-control"
        } else if tree {
            "--runner-tree"
        } else {
            "--runner"
        }
        .into()];
        if let Some(case) = io_case {
            arguments = vec!["--io-runner".into(), case.name().into()];
        }
        let spec = WorkerLaunchSpec::new("sandbox:pty-mechanism-test", program, arguments)?;
        worker.launch(LaunchRequest {
            spec: &spec,
            workspace: &workspace,
            input: super::LaunchInput::Worker(
                super::super::windows_appcontainer_support::WorkerEnvironment {
                    workspace: &workspace,
                    job_path: &input,
                    permit_path: &permit,
                    execution_path: None,
                    file_target_path: None,
                    workspace_scope: "workspace:pty-mechanism-test",
                    deadline: super::super::windows_appcontainer_support::recovery_timestamp()
                        + 10_000,
                },
            ),
            pty_mechanism_probe: allow_terminal,
        })?;
        run_probe(
            &root,
            &mut worker,
            tree,
            allow_terminal,
            &mut observed,
            &mut report,
            io_case,
        )
    })();
    report["result"] = json!(if result.is_ok() { "PASS" } else { "FAIL" });
    if let Err(error) = &result {
        report["error"] = json!(error
            .to_string()
            .replace(root.to_string_lossy().as_ref(), "<probe-root>"));
    }
    let cleanup = (|| -> ProbeResult<()> {
        worker.terminate()?;
        if worker.terminal_state()? != (true, 0) {
            return Err(p::Error("original probe process tree did not terminate".into()).into());
        }
        wait_for_observed(&observed)?;
        report["cleanup_original_job_empty"] = json!(true);
        if let Some(profile) = worker.profile() {
            for grant in grants.iter().rev() {
                let mut revoke = grant.clone();
                revoke.operation = SandboxAclOperation::Revoke;
                revoke.recovery_stage = SandboxRecoveryStage::RevokeAclPending;
                revoke.operation_id = format!("revoke-{}", revoke.operation_id);
                journal(&mut lease, &json!({"intent": revoke}))?;
                let readback = acl::apply_acl(&revoke, profile)?;
                journal(&mut lease, &json!({"readback": readback}))?;
            }
            journal(
                &mut lease,
                &json!({"profile_deleted": acl::delete_profile(profile)?}),
            )?;
        }
        Ok(())
    })();
    report["cleanup"] = json!(if cleanup.is_ok() {
        "PASS"
    } else {
        "UNVERIFIABLE"
    });
    if let Err(error) = &cleanup {
        report["cleanup_error"] = json!(error.to_string());
    }
    report["listener_accepted_sandbox_connection"] = json!(listener.accept().is_ok());
    for path in &sentinel_paths {
        assert_eq!(fs::read(path).unwrap(), b"private fixture sentinel");
    }
    report["observed_processes"] = json!(observed
        .iter()
        .map(|process| json!({
            "pid": process.pid, "created": process.created,
            "image_name": Path::new(&process.image).file_name().unwrap().to_string_lossy(),
            "exited": process.exited().ok(),
        }))
        .collect::<Vec<_>>());
    durable(&root.join("mechanism-receipt.json"), &report).unwrap();
    println!(
        "mechanism receipt: {}",
        root.join("mechanism-receipt.json").display()
    );
    assert!(
        cleanup.is_ok(),
        "original probe cleanup failed: {cleanup:?}"
    );
    assert!(
        result.is_ok(),
        "restricted terminal mechanism failed: {result:?}"
    );
    assert_eq!(report["listener_accepted_sandbox_connection"], false);
}
