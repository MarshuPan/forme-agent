use std::path::PathBuf;
use std::time::{Duration, Instant};

use forme_protocol as p;

use crate::support::{
    cancelled, complete, emit_started, failed, ActiveCancellations, BoundedOutput,
};
use crate::{
    planner::plan_with, planner::validate_plan_for, ActionBackend, ActionResult, BackendKind,
    CancelToken, EventSink, ExecutionPlan, FileDiff, FileRollback, OutputBudget,
};

pub struct FileBackend {
    roots: Vec<PathBuf>,
    budget: OutputBudget,
    timeout: p::DurationMs,
    active: ActiveCancellations,
}

impl FileBackend {
    pub fn new(
        roots: Vec<String>,
        budget: OutputBudget,
        timeout: p::DurationMs,
    ) -> p::Result<Self> {
        if roots.is_empty() || budget.max_bytes == 0 || timeout.0 == 0 {
            return Err(p::Error("file backend configuration is incomplete".into()));
        }
        let roots = roots
            .into_iter()
            .map(|root| {
                let root = std::fs::canonicalize(&root).map_err(|error| {
                    p::Error(format!("failed to resolve file root {root}: {error}"))
                })?;
                if !root.is_dir() {
                    return Err(p::Error("file root is not a directory".into()));
                }
                Ok(root)
            })
            .collect::<p::Result<Vec<_>>>()?;
        Ok(Self {
            roots,
            budget,
            timeout,
            active: ActiveCancellations::default(),
        })
    }

    fn resolve_target(&self, requested: &str) -> p::Result<PathBuf> {
        let requested = PathBuf::from(requested);
        let resolved = if requested.exists() {
            std::fs::canonicalize(&requested)
                .map_err(|error| p::Error(format!("failed to resolve file target: {error}")))?
        } else {
            let parent = requested
                .parent()
                .ok_or_else(|| p::Error("file target has no parent".into()))?;
            let file_name = requested
                .file_name()
                .ok_or_else(|| p::Error("file target has no final component".into()))?;
            std::fs::canonicalize(parent)
                .map_err(|error| p::Error(format!("failed to resolve file parent: {error}")))?
                .join(file_name)
        };
        if !self
            .roots
            .iter()
            .any(|root| resolved == *root || resolved.starts_with(root))
        {
            return Err(p::Error("file target is outside allowed roots".into()));
        }
        Ok(resolved)
    }

    fn check_interrupt(
        &self,
        plan: &ExecutionPlan,
        sink: &EventSink,
        token: &CancelToken,
        output: &BoundedOutput,
        started: Instant,
    ) -> p::Result<Option<ActionResult>> {
        if token.is_cancelled() {
            return cancelled(plan, sink, output).map(Some);
        }
        if started.elapsed() >= Duration::from_millis(plan.timeout.0) {
            return Err(failed(plan, sink, "file action exceeded its timeout"));
        }
        Ok(None)
    }
}

impl ActionBackend for FileBackend {
    fn kind(&self) -> BackendKind {
        p::BackendKind::File
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
        validate_plan_for(&plan, p::BackendKind::File)?;
        let p::ActionParameters::File {
            operation,
            path,
            content,
        } = &plan.intent.parameters
        else {
            return Err(p::Error("file backend received non-file parameters".into()));
        };
        match operation {
            p::FileOperation::Read | p::FileOperation::Delete if content.is_some() => {
                return Err(p::Error(
                    "file read/delete cannot carry replacement content".into(),
                ));
            }
            p::FileOperation::Write if content.is_none() => {
                return Err(p::Error("file write requires content".into()));
            }
            _ => {}
        }
        let target = self.resolve_target(path)?;
        let mut output = BoundedOutput::new(plan.budget.clone());
        if token.is_cancelled() {
            return cancelled(&plan, sink, &output);
        }
        self.active
            .register(plan.intent.intent_id.clone(), token.clone())?;
        let started = Instant::now();
        let result = (|| {
            emit_started(&plan, sink)?;
            if let Some(cancelled) = self.check_interrupt(&plan, sink, &token, &output, started)? {
                return Ok(cancelled);
            }
            match operation {
                p::FileOperation::Read => {
                    let bytes = std::fs::read(&target).map_err(|error| {
                        failed(&plan, sink, format!("failed to read file: {error}"))
                    })?;
                    output
                        .push(&bytes, &plan, sink)
                        .map_err(|error| failed(&plan, sink, error.to_string()))?;
                    complete(&plan, sink, &output, None, None)
                }
                p::FileOperation::Write | p::FileOperation::Delete => {
                    let previous = if target.exists() {
                        Some(std::fs::read(&target).map_err(|error| {
                            failed(
                                &plan,
                                sink,
                                format!("failed to read existing file: {error}"),
                            )
                        })?)
                    } else {
                        None
                    };
                    if *operation == p::FileOperation::Delete && previous.is_none() {
                        return Err(failed(&plan, sink, "file delete target does not exist"));
                    }
                    let next_bytes = content.as_ref().map_or(0, Vec::len) as u64;
                    let summary = format!(
                        "file {:?}: {} bytes -> {} bytes at {}",
                        operation,
                        previous.as_ref().map_or(0, Vec::len),
                        next_bytes,
                        target.display()
                    );
                    let diff = FileDiff {
                        schema_version: p::SchemaVersion(1),
                        path: target.to_string_lossy().into_owned(),
                        operation: *operation,
                        previous_bytes: previous.as_ref().map_or(0, Vec::len) as u64,
                        next_bytes,
                        summary: summary.clone(),
                    };
                    output
                        .push(summary.as_bytes(), &plan, sink)
                        .map_err(|error| failed(&plan, sink, error.to_string()))?;
                    if let Some(cancelled) =
                        self.check_interrupt(&plan, sink, &token, &output, started)?
                    {
                        return Ok(cancelled);
                    }
                    validate_plan_for(&plan, p::BackendKind::File)?;
                    match operation {
                        p::FileOperation::Write => {
                            let Some(content) = content else {
                                return Err(failed(
                                    &plan,
                                    sink,
                                    "file write content disappeared before execution",
                                ));
                            };
                            std::fs::write(&target, content).map_err(|error| {
                                failed(&plan, sink, format!("failed to write file: {error}"))
                            })?
                        }
                        p::FileOperation::Delete => {
                            std::fs::remove_file(&target).map_err(|error| {
                                failed(&plan, sink, format!("failed to delete file: {error}"))
                            })?
                        }
                        p::FileOperation::Read => {
                            return Err(failed(
                                &plan,
                                sink,
                                "file operation changed during execution",
                            ));
                        }
                    }
                    let rollback = FileRollback {
                        schema_version: p::SchemaVersion(1),
                        path: target.to_string_lossy().into_owned(),
                        remove_if_previously_absent: previous.is_none(),
                        previous,
                    };
                    complete(&plan, sink, &output, Some(diff), Some(rollback))
                }
            }
        })();
        self.active.remove(&plan.intent.intent_id);
        result
    }

    fn cancel(&self, action: p::ActionId) -> p::Result<()> {
        self.active.cancel(&action)
    }
}
