//! One-shot installed file entry. Only the Host may commit its private copy.
#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_protocol as p;
use serde::de::DeserializeOwned;

const INPUT_LIMIT: u64 = 1_048_576;
const FILE_LIMIT: u64 = 16 * 1024 * 1024;

struct FileInvocation {
    job: p::WorkerJob,
    permit: p::WorkerPermit,
    execution: p::WorkerExecutionSpec,
    target: PathBuf,
    output: PathBuf,
}

/// Execute exactly one already-authorized File action in a Host-owned copy.
/// No listener, shell, child process, model, secret resolver or brain writer.
/// Failure is never retried here; the Host owns the original job's outcome.
pub fn run_installed_file_worker() -> p::Result<()> {
    let invocation = FileInvocation::load()?;
    let bytes = invocation.execute()?;
    invocation.check_deadline()?;
    let mut result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&invocation.output)
        .map_err(|_| unavailable())?;
    result
        .write_all(&bytes)
        .and_then(|()| result.sync_all())
        .map_err(|_| unavailable())
}

impl FileInvocation {
    fn load() -> p::Result<Self> {
        if std::env::var("FORME_WORKER_PROTOCOL_VERSION")
            .ok()
            .as_deref()
            != Some("1")
        {
            return Err(unavailable());
        }
        let job_path = environment_path("FORME_WORKER_JOB_PATH")?;
        let run = job_path.parent().ok_or_else(unavailable)?;
        let permit_path = environment_path("FORME_WORKER_PERMIT_PATH")?;
        let execution_path = environment_path("FORME_WORKER_EXECUTION_PATH")?;
        let mapping_path = environment_path("FORME_WORKER_FILE_TARGET_PATH")?;
        let workspace = environment_path("FORME_WORKER_WORKSPACE_PATH")?;
        let output = environment_path("FORME_WORKER_RESULT_PATH")?;
        if job_path != run.join("job.json")
            || permit_path != run.join("permit.json")
            || execution_path != run.join("execution.json")
            || mapping_path != run.join("file-target.json")
            || workspace != run.join("workspace")
            || output != run.join("results/result.bin")
        {
            return Err(unavailable());
        }
        let job: p::WorkerJob = read_input(run, &job_path)?;
        let permit: p::WorkerPermit = read_input(run, &permit_path)?;
        let execution: p::WorkerExecutionSpec = read_input(run, &execution_path)?;
        let mapping: p::WorkerFileTarget = read_input(run, &mapping_path)?;
        let now = now_ms()?;
        job.validate()?;
        permit.validate_for(&job, now)?;
        execution.validate_for(&job, &permit, now)?;
        mapping.validate_for(&job, &execution)?;
        if std::env::var("FORME_WORKER_WORKSPACE_SCOPE")
            .ok()
            .as_deref()
            != Some(job.workspace_scope.0.as_str())
            || std::env::var("FORME_WORKER_DEADLINE")
                .ok()
                .and_then(|v| v.parse::<i64>().ok())
                != Some(job.deadline)
            || !fs::symlink_metadata(&workspace).is_ok_and(|m| m.is_dir())
        {
            return Err(unavailable());
        }
        check_job_path(run, &workspace)?;
        check_job_path(run, &output)?;
        if output.try_exists().map_err(|_| unavailable())? {
            return Err(unavailable());
        }
        let target = workspace.join(&mapping.relative_path);
        check_job_path(run, &target)?;
        let parent = fs::canonicalize(target.parent().ok_or_else(unavailable)?)
            .map_err(|_| unavailable())?;
        let workspace = fs::canonicalize(workspace).map_err(|_| unavailable())?;
        if !parent.starts_with(&workspace) {
            return Err(unavailable());
        }
        Ok(Self {
            job,
            permit,
            execution,
            target,
            output,
        })
    }

    fn check_deadline(&self) -> p::Result<()> {
        let now = now_ms()?;
        self.permit.validate_for(&self.job, now)?;
        self.execution.validate_for(&self.job, &self.permit, now)
    }

    fn execute(&self) -> p::Result<Vec<u8>> {
        self.check_deadline()?;
        let p::ActionParameters::File {
            operation, content, ..
        } = &self.execution.intent.parameters
        else {
            return Err(unavailable());
        };
        let existing = match fs::symlink_metadata(&self.target) {
            Ok(metadata) if metadata.is_file() => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            _ => return Err(unavailable()),
        };
        match operation {
            p::FileOperation::Read if content.is_none() => {
                let metadata = existing.ok_or_else(unavailable)?;
                let limit = self.execution.output_budget_bytes.min(FILE_LIMIT);
                if metadata.len() > limit {
                    return Err(unavailable());
                }
                let mut bytes = Vec::new();
                checked_file(File::open(&self.target).map_err(|_| unavailable())?)?
                    .take(limit + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| unavailable())?;
                if bytes.len() as u64 != metadata.len() || std::str::from_utf8(&bytes).is_err() {
                    return Err(unavailable());
                }
                Ok(bytes)
            }
            p::FileOperation::Write => {
                let content = content
                    .as_ref()
                    .filter(|bytes| bytes.len() as u64 <= FILE_LIMIT)
                    .ok_or_else(unavailable)?;
                let output = self.acknowledgement(b"file write complete\n")?;
                let mut options = OpenOptions::new();
                options.read(true).write(true);
                if existing.is_none() {
                    options.create_new(true);
                }
                let mut file =
                    checked_file(options.open(&self.target).map_err(|_| unavailable())?)?;
                file.set_len(0).map_err(|_| unavailable())?;
                file.write_all(content)
                    .and_then(|()| file.sync_all())
                    .map_err(|_| unavailable())?;
                Ok(output)
            }
            p::FileOperation::Delete if content.is_none() => {
                existing.ok_or_else(unavailable)?;
                drop(checked_file(
                    File::open(&self.target).map_err(|_| unavailable())?,
                )?);
                let output = self.acknowledgement(b"file delete complete\n")?;
                fs::remove_file(&self.target).map_err(|_| unavailable())?;
                Ok(output)
            }
            _ => Err(unavailable()),
        }
    }

    fn acknowledgement(&self, bytes: &[u8]) -> p::Result<Vec<u8>> {
        if bytes.len() as u64 > self.execution.output_budget_bytes {
            return Err(unavailable());
        }
        Ok(bytes.to_vec())
    }
}

fn checked_file(file: File) -> p::Result<File> {
    let metadata = file.metadata().map_err(|_| unavailable())?;
    if !metadata.is_file() || metadata.len() > FILE_LIMIT {
        return Err(unavailable());
    }
    #[cfg(windows)]
    if winapi_util::file::information(&file)
        .map_err(|_| unavailable())?
        .number_of_links()
        != 1
    {
        return Err(unavailable());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(unavailable());
        }
    }
    Ok(file)
}

fn environment_path(name: &str) -> p::Result<PathBuf> {
    let path = std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(unavailable)?;
    if !path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(unavailable());
    }
    Ok(path)
}

fn read_input<T: DeserializeOwned>(run: &Path, path: &Path) -> p::Result<T> {
    check_job_path(run, path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| unavailable())?;
    if !metadata.is_file() || metadata.len() > INPUT_LIMIT {
        return Err(unavailable());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(INPUT_LIMIT + 1).read_to_end(&mut bytes))
        .map_err(|_| unavailable())?;
    if bytes.len() as u64 != metadata.len() {
        return Err(unavailable());
    }
    serde_json::from_slice(&bytes).map_err(|_| unavailable())
}

// Host has already checked and holds custody of the private ancestors. The
// worker can inspect only its readonly job root and the descendants it owns;
// requiring Host-private ancestor access would defeat the isolation boundary.
fn check_job_path(run: &Path, path: &Path) -> p::Result<()> {
    let relative = path.strip_prefix(run).map_err(|_| unavailable())?;
    let mut current = run.to_path_buf();
    let root = fs::symlink_metadata(run).map_err(|_| unavailable())?;
    if !root.is_dir() || is_link(&root) {
        return Err(unavailable());
    }
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(unavailable());
        }
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_link(&metadata) => return Err(unavailable()),
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(unavailable()),
        }
    }
    Ok(())
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

fn now_ms() -> p::Result<p::Timestamp> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|v| i64::try_from(v.as_millis()).ok())
        .ok_or_else(unavailable)
}

fn unavailable() -> p::Error {
    p::Error("installed file worker input or execution is unavailable or unverifiable".into())
}
