use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use forme_protocol as p;

use crate::support::{
    cancelled, complete, emit_started, failed, monitor_child, ActiveCancellations, BoundedOutput,
    ProcessOutcome,
};
use crate::{
    planner::plan_with, planner::validate_plan_for, ActionBackend, ActionResult, BackendKind,
    CancelToken, EventSink, ExecutionPlan, OutputBudget,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSandbox {
    pub schema_version: p::SchemaVersion,
    pub available: bool,
    pub allowed_programs: Vec<String>,
    pub allowed_roots: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub network_allowed: bool,
}

pub struct ShellBackend {
    sandbox: ShellSandbox,
    canonical_roots: Vec<PathBuf>,
    budget: OutputBudget,
    timeout: p::DurationMs,
    active: ActiveCancellations,
}

impl ShellBackend {
    pub fn new(
        sandbox: ShellSandbox,
        budget: OutputBudget,
        timeout: p::DurationMs,
    ) -> p::Result<Self> {
        if !sandbox.available
            || sandbox.allowed_programs.is_empty()
            || sandbox.allowed_roots.is_empty()
            || budget.max_bytes == 0
            || timeout.0 == 0
        {
            return Err(p::Error("shell backend configuration is incomplete".into()));
        }
        let canonical_roots = sandbox
            .allowed_roots
            .iter()
            .map(|root| {
                std::fs::canonicalize(root).map_err(|error| {
                    p::Error(format!("failed to resolve shell root {root}: {error}"))
                })
            })
            .collect::<p::Result<Vec<_>>>()?;
        Ok(Self {
            sandbox,
            canonical_roots,
            budget,
            timeout,
            active: ActiveCancellations::default(),
        })
    }

    fn execute_inner(
        &self,
        plan: &ExecutionPlan,
        sink: &EventSink,
        token: &CancelToken,
    ) -> p::Result<ActionResult> {
        let p::ActionParameters::Shell {
            program,
            args,
            cwd,
            network,
        } = &plan.intent.parameters
        else {
            return Err(p::Error(
                "shell backend received non-shell parameters".into(),
            ));
        };
        if !self
            .sandbox
            .allowed_programs
            .iter()
            .any(|allowed| allowed == program)
        {
            return Err(p::Error("shell program is not allowlisted".into()));
        }
        if *network && !self.sandbox.network_allowed {
            return Err(p::Error(
                "shell action requested forbidden network access".into(),
            ));
        }
        let cwd = cwd.as_ref().ok_or_else(|| {
            p::Error("shell action requires an explicit working directory".into())
        })?;
        let canonical_cwd = std::fs::canonicalize(cwd).map_err(|error| {
            p::Error(format!(
                "failed to resolve shell working directory: {error}"
            ))
        })?;
        if !self
            .canonical_roots
            .iter()
            .any(|root| path_within(root, &canonical_cwd))
        {
            return Err(p::Error(
                "shell working directory is outside allowed roots".into(),
            ));
        }
        if self
            .sandbox
            .environment
            .iter()
            .any(|(name, _)| name.trim().is_empty())
        {
            return Err(p::Error("shell environment contains an empty name".into()));
        }

        emit_started(plan, sink)?;
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(&canonical_cwd)
            .env_clear()
            .envs(self.sandbox.environment.iter().cloned())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        validate_plan_for(plan, p::BackendKind::Shell)?;
        let mut child = command.spawn().map_err(|error| {
            failed(plan, sink, format!("failed to start shell action: {error}"))
        })?;
        let mut output = BoundedOutput::new(plan.budget.clone());
        match monitor_child(&mut child, plan.timeout, token, plan, sink, &mut output) {
            Ok(ProcessOutcome::Exited(status)) if status.success() => {
                complete(plan, sink, &output, None, None)
            }
            Ok(ProcessOutcome::Exited(status)) => Err(failed(
                plan,
                sink,
                format!("shell action exited with status {status}"),
            )),
            Ok(ProcessOutcome::Cancelled) => cancelled(plan, sink, &output),
            Ok(ProcessOutcome::TimedOut) => {
                Err(failed(plan, sink, "shell action exceeded its timeout"))
            }
            Err(error) => Err(failed(plan, sink, error.to_string())),
        }
    }
}

impl ActionBackend for ShellBackend {
    fn kind(&self) -> BackendKind {
        p::BackendKind::Shell
    }

    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        plan_with(intent, self.budget.clone(), self.timeout)
    }

    fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        token: CancelToken,
    ) -> p::Result<ActionResult> {
        validate_plan_for(&plan, p::BackendKind::Shell)?;
        let output = BoundedOutput::new(plan.budget.clone());
        if token.is_cancelled() {
            return cancelled(&plan, sink, &output);
        }
        self.active
            .register(plan.intent.intent_id.clone(), token.clone())?;
        let result = self.execute_inner(&plan, sink, &token);
        self.active.remove(&plan.intent.intent_id);
        result
    }

    fn cancel(&self, action: p::ActionId) -> p::Result<()> {
        self.active.cancel(&action)
    }
}

fn path_within(root: &Path, path: &Path) -> bool {
    path == root || path.starts_with(root)
}
