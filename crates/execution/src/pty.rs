use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, Instant};

use forme_protocol as p;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

#[path = "pty_io.rs"]
mod io;

use crate::support::{
    cancelled, complete_external, emit_started, fail_unless_terminal, failed, ActiveCancellations,
    BoundedOutput,
};
use crate::{
    plan_with, validate_plan_for, ActionBackend, ActionResult, BackendKind, CancelToken,
    ContentResolver, EventSink, ExecutionPlan, OutputBudget, SecretResolver,
};

pub struct PtyBackend {
    allowed_programs: Vec<PathBuf>,
    allowed_roots: Vec<PathBuf>,
    secrets: Arc<dyn SecretResolver>,
    contents: Arc<dyn ContentResolver>,
    budget: OutputBudget,
    timeout: p::DurationMs,
    active: ActiveCancellations,
}

impl PtyBackend {
    pub fn new(
        allowed_programs: Vec<PathBuf>,
        allowed_roots: Vec<PathBuf>,
        secrets: Arc<dyn SecretResolver>,
        contents: Arc<dyn ContentResolver>,
        budget: OutputBudget,
        timeout: p::DurationMs,
    ) -> p::Result<Self> {
        if allowed_programs.is_empty()
            || allowed_roots.is_empty()
            || budget.max_bytes == 0
            || timeout.0 == 0
        {
            return Err(p::Error("PTY backend configuration is incomplete".into()));
        }
        let allowed_programs = allowed_programs
            .into_iter()
            .map(|path| canonical_file(&path, "PTY program"))
            .collect::<p::Result<Vec<_>>>()?;
        let allowed_roots = allowed_roots
            .into_iter()
            .map(|path| canonical_directory(&path, "PTY root"))
            .collect::<p::Result<Vec<_>>>()?;
        Ok(Self {
            allowed_programs,
            allowed_roots,
            secrets,
            contents,
            budget,
            timeout,
            active: ActiveCancellations::default(),
        })
    }

    fn validate_spec(&self, spec: &p::PtyActionSpec) -> p::Result<(PathBuf, PathBuf)> {
        if spec.schema_version.0 == 0
            || !(1..=500).contains(&spec.cols)
            || !(1..=200).contains(&spec.rows)
        {
            return Err(p::Error("PTY dimensions or schema are invalid".into()));
        }
        let program = canonical_file(Path::new(&spec.program), "PTY program")?;
        let cwd = canonical_directory(Path::new(&spec.cwd), "PTY working directory")?;
        if !self.allowed_programs.contains(&program)
            || !self.allowed_roots.iter().any(|root| cwd.starts_with(root))
        {
            return Err(p::Error(
                "PTY program or working directory is outside policy".into(),
            ));
        }
        let mut names = BTreeSet::new();
        for binding in &spec.environment {
            if binding.schema_version.0 == 0
                || !valid_environment_name(&binding.name)
                || binding.value.0.trim().is_empty()
                || !names.insert(binding.name.as_str())
            {
                return Err(p::Error("PTY secret binding is invalid".into()));
            }
        }
        Ok((program, cwd))
    }
}

impl ActionBackend for PtyBackend {
    fn kind(&self) -> BackendKind {
        p::BackendKind::Pty
    }

    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        let p::ActionParameters::Pty(spec) = &intent.parameters else {
            return Err(p::Error("PTY backend requires PTY parameters".into()));
        };
        self.validate_spec(spec)?;
        plan_with(intent, self.budget.clone(), self.timeout)
    }

    fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        cancel: CancelToken,
    ) -> p::Result<ActionResult> {
        validate_plan_for(&plan, self.kind())?;
        let p::ActionParameters::Pty(spec) = &plan.intent.parameters else {
            return Err(p::Error("PTY plan has incompatible parameters".into()));
        };
        let (program, cwd) = self.validate_spec(spec)?;
        self.active
            .register(plan.intent.intent_id.clone(), cancel.clone())?;
        let result = self
            .execute_inner(&plan, sink, &cancel, spec, &program, &cwd)
            .map_err(|error| fail_unless_terminal(&plan, sink, error));
        self.active.remove(&plan.intent.intent_id);
        result
    }

    fn cancel(&self, action: p::ActionId) -> p::Result<()> {
        self.active.cancel(&action)
    }
}

impl PtyBackend {
    fn execute_inner(
        &self,
        plan: &ExecutionPlan,
        sink: &EventSink,
        cancel: &CancelToken,
        spec: &p::PtyActionSpec,
        program: &Path,
        cwd: &Path,
    ) -> p::Result<ActionResult> {
        let mut output = BoundedOutput::new(plan.budget.clone());
        if cancel.is_cancelled() {
            return cancelled(plan, sink, &output);
        }
        let admission = io::Admission::acquire()?;
        let started = Instant::now();
        let system = native_pty_system();
        let pair = system
            .openpty(PtySize {
                rows: spec.rows,
                cols: spec.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|_| p::Error("PTY could not be opened".into()))?;
        let mut command = CommandBuilder::new(program);
        command.args(&spec.args);
        command.cwd(cwd);
        command.env_clear();
        let resolved = spec
            .environment
            .iter()
            .map(|binding| {
                self.secrets
                    .resolve(&binding.value)
                    .map(|secret| (binding.name.clone(), secret))
            })
            .collect::<p::Result<Vec<_>>>()?;
        let mut resolved_input = None;
        let input = match &spec.input {
            Some(p::ExternalInput::Literal(value)) => Some(value.as_bytes().to_vec()),
            Some(p::ExternalInput::Content(reference)) => Some(self.contents.resolve(reference)?),
            Some(p::ExternalInput::Secret(reference)) => {
                let secret = self.secrets.resolve(reference)?;
                let value = secret.expose().as_bytes().to_vec();
                resolved_input = Some(secret);
                Some(value)
            }
            None => None,
        };
        let mut redactor = SecretRedactor::new(
            resolved
                .iter()
                .map(|(_, secret)| secret.expose().as_bytes().to_vec())
                .chain(
                    resolved_input
                        .iter()
                        .map(|secret| secret.expose().as_bytes().to_vec()),
                )
                .collect(),
        );
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        #[cfg(windows)]
        if let Some(comspec) = std::env::var_os("ComSpec") {
            command.env("ComSpec", comspec);
        }
        command.env("TERM", "dumb");
        command.env("NO_COLOR", "1");
        for (name, secret) in &resolved {
            command.env(name, secret.expose());
        }
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|_| p::Error("PTY reader could not be cloned".into()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|_| p::Error("PTY writer could not be obtained".into()))?;
        if started.elapsed() >= Duration::from_millis(plan.timeout.0) {
            return Err(p::Error("PTY setup exceeded its timeout".into()));
        }
        // The observer persists the start fact. A rejected append must leave
        // the original program unstarted, including at this cancellation cut.
        emit_started(plan, sink)?;
        if cancel.is_cancelled() {
            return cancelled(plan, sink, &output);
        }
        if started.elapsed() >= Duration::from_millis(plan.timeout.0) {
            return Err(p::Error("PTY start acceptance exceeded its timeout".into()));
        }
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|_| p::Error("PTY child could not be started".into()))?;
        drop(pair.slave);

        let io = io::TerminalIo::start(reader, writer, input);
        let mut child = child;
        let mut decoder = TerminalDecoder::default();
        let terminal = (|| -> p::Result<PtyTerminal> {
            loop {
                if cancel.is_cancelled() {
                    return Ok(PtyTerminal::Cancelled);
                }
                if started.elapsed() >= Duration::from_millis(plan.timeout.0) {
                    return Ok(PtyTerminal::TimedOut);
                }
                if io.write_failed.load(Ordering::SeqCst) {
                    return Err(p::Error("PTY input could not be written".into()));
                }
                match child.try_wait() {
                    Ok(Some(status)) => return Ok(PtyTerminal::Exited(status)),
                    Ok(None) => {}
                    Err(_) => return Ok(PtyTerminal::Unknown),
                }
                match io.output.recv_timeout(Duration::from_millis(10)) {
                    Ok(io::ReadEvent::Bytes(bytes)) => {
                        let (bytes, queries) = decoder.push(&bytes);
                        for _ in 0..queries {
                            io.reply(b"\x1b[1;1R")?;
                        }
                        push_pty_bytes(&bytes, &mut redactor, &mut output, plan, sink)?;
                    }
                    Ok(io::ReadEvent::Failed) => {
                        return Err(p::Error("PTY output read failed".into()))
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                }
            }
        })();
        let input_complete = io.input_complete.load(Ordering::SeqCst);
        let mut output_failure = None;
        let cleanup = io.shutdown(child, pair.master, admission, |bytes| {
            if terminal.is_ok() && output_failure.is_none() {
                let (bytes, _) = decoder.push(bytes);
                if let Err(error) = push_pty_bytes(&bytes, &mut redactor, &mut output, plan, sink) {
                    output_failure = Some(error);
                }
            }
            Ok(())
        });
        let terminal = if cleanup.is_err() {
            PtyTerminal::Unknown
        } else {
            terminal?
        };
        if matches!(terminal, PtyTerminal::Unknown) {
            return unknown_pty_outcome(plan, sink);
        }
        if let Some(error) = output_failure {
            return Err(error);
        }
        if matches!(terminal, PtyTerminal::Exited(_)) && !input_complete {
            return Err(p::Error("PTY exited before its input was written".into()));
        }
        push_pty_bytes(&decoder.pending, &mut redactor, &mut output, plan, sink)?;
        let tail = redactor.finish();
        if !tail.is_empty() && !output.truncated() {
            output.push(&tail, plan, sink)?;
        }

        match terminal {
            PtyTerminal::Exited(status) if status.success() => complete_external(
                plan,
                sink,
                &output,
                p::ExternalActionReceipt {
                    schema_version: p::SchemaVersion(1),
                    action: plan.intent.intent_id.clone(),
                    content_ref: None,
                    content_digest: None,
                    trust: p::TrustTier::Untrusted,
                    effect: p::EffectStatus::Committed,
                    probe_hint: None,
                },
            ),
            PtyTerminal::Exited(status) => Err(failed(
                plan,
                sink,
                format!("PTY child exited with code {}", status.exit_code()),
            )),
            PtyTerminal::Cancelled => cancelled(plan, sink, &output),
            PtyTerminal::TimedOut => Err(failed(plan, sink, "PTY action exceeded its timeout")),
            PtyTerminal::Unknown => unknown_pty_outcome(plan, sink),
        }
    }
}

fn unknown_pty_outcome(plan: &ExecutionPlan, sink: &EventSink) -> p::Result<ActionResult> {
    sink.emit(p::EventPayload::ActionOutcomeUnknown(
        p::ActionOutcomeUnknownPayload {
            intent_id: plan.intent.intent_id.clone(),
            probe_hint: p::ProbeHintRef("manual-process-status-review".into()),
            remote_lease: None,
        },
    ))?;
    Err(p::Error("PTY child outcome became unknown".into()))
}

fn push_pty_bytes(
    bytes: &[u8],
    redactor: &mut SecretRedactor,
    output: &mut BoundedOutput,
    plan: &ExecutionPlan,
    sink: &EventSink,
) -> p::Result<()> {
    if output.truncated() {
        return Ok(());
    }
    let safe = redactor.push(bytes);
    output.push(&safe, plan, sink)
}

#[derive(Default)]
struct TerminalDecoder {
    pending: Vec<u8>,
}

impl TerminalDecoder {
    fn push(&mut self, bytes: &[u8]) -> (Vec<u8>, usize) {
        const QUERY: &[u8] = b"\x1b[6n";
        self.pending.extend_from_slice(bytes);
        let mut output = Vec::with_capacity(self.pending.len());
        let mut queries = 0;
        let mut offset = 0;
        while offset < self.pending.len() {
            let remaining = &self.pending[offset..];
            if remaining.starts_with(QUERY) {
                offset += QUERY.len();
                queries += 1;
            } else if QUERY.starts_with(remaining) {
                break;
            } else {
                output.push(self.pending[offset]);
                offset += 1;
            }
        }
        self.pending.drain(..offset);
        (output, queries)
    }
}

struct SecretRedactor {
    patterns: Vec<Vec<u8>>,
    pending: Vec<u8>,
    retained_tail: usize,
}

impl SecretRedactor {
    fn new(mut patterns: Vec<Vec<u8>>) -> Self {
        patterns.retain(|pattern| !pattern.is_empty());
        patterns.sort_by_key(|pattern| std::cmp::Reverse(pattern.len()));
        patterns.dedup();
        let retained_tail = patterns
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(1)
            .saturating_sub(1);
        Self {
            patterns,
            pending: Vec::new(),
            retained_tail,
        }
    }

    fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(bytes);
        self.redact_complete_matches();
        let emit = self.pending.len().saturating_sub(self.retained_tail);
        self.pending.drain(..emit).collect()
    }

    fn finish(mut self) -> Vec<u8> {
        self.redact_complete_matches();
        self.pending
    }

    fn redact_complete_matches(&mut self) {
        for pattern in &self.patterns {
            let mut offset = 0;
            while offset + pattern.len() <= self.pending.len() {
                if self.pending[offset..offset + pattern.len()] == pattern[..] {
                    self.pending[offset..offset + pattern.len()].fill(b'*');
                    offset += pattern.len();
                } else {
                    offset += 1;
                }
            }
        }
    }
}

enum PtyTerminal {
    Exited(portable_pty::ExitStatus),
    Cancelled,
    TimedOut,
    Unknown,
}

fn canonical_file(path: &Path, label: &str) -> p::Result<PathBuf> {
    let path = path
        .canonicalize()
        .map_err(|_| p::Error(format!("{label} cannot be canonicalized")))?;
    if !path.is_file() {
        return Err(p::Error(format!("{label} is not a file")));
    }
    Ok(path)
}

fn canonical_directory(path: &Path, label: &str) -> p::Result<PathBuf> {
    let path = path
        .canonicalize()
        .map_err(|_| p::Error(format!("{label} cannot be canonicalized")))?;
    if !path.is_dir() {
        return Err(p::Error(format!("{label} is not a directory")));
    }
    Ok(path)
}

fn valid_environment_name(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::TerminalDecoder;

    #[test]
    fn split_and_coalesced_terminal_queries_preserve_neighboring_output() {
        let mut decoder = TerminalDecoder::default();
        assert_eq!(decoder.push(b"before\x1b["), (b"before".to_vec(), 0));
        assert_eq!(
            decoder.push(b"6nafter\x1b[6nend"),
            (b"afterend".to_vec(), 2)
        );
        assert!(decoder.pending.is_empty());
        assert_eq!(decoder.push(b"\x1b[3mcolor"), (b"\x1b[3mcolor".to_vec(), 0));
    }
}
