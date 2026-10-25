#![cfg(any(target_os = "linux", target_os = "macos"))]
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::posix_worker_fs::{
    chown_recursive, copy_bounded, execution_account, private_file, read_record, same_contents,
    write_record, Entry, FileIdentity, Tree,
};
use super::reject_linked_ancestors;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Baseline {
    schema_version: p::SchemaVersion,
    job: p::WorkerJob,
    permit: p::WorkerPermit,
    execution: p::WorkerExecutionSpec,
    original_root: PathBuf,
    original_identity: FileIdentity,
    overlay_identity: FileIdentity,
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteIntent {
    schema_version: p::SchemaVersion,
    baseline_digest: p::SchemaDigest,
    result_digest: p::SchemaDigest,
    changes: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteReceipt {
    schema_version: p::SchemaVersion,
    intent_digest: p::SchemaDigest,
    baseline_digest: p::SchemaDigest,
    result_digest: p::SchemaDigest,
}

#[derive(Debug)]
pub(super) struct PreparedOverlay {
    run_dir: PathBuf,
    baseline: Baseline,
}

impl PreparedOverlay {
    pub(super) fn prepare(
        run_dir: &Path,
        original: &Path,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: Option<&p::WorkerExecutionSpec>,
        now: p::Timestamp,
    ) -> p::Result<Self> {
        permit.validate_for(job, now)?;
        let execution = execution
            .ok_or_else(|| p::Error("POSIX managed worker requires an execution spec".into()))?;
        execution.validate_for(job, permit, now)?;
        reject_linked_ancestors(run_dir)?;
        let source = Tree::capture(original)?;
        let workspace = run_dir.join("workspace");
        source.copy_to(&workspace)?;
        let overlay = Tree::capture(&workspace)?;
        let baseline = Baseline {
            schema_version: p::SchemaVersion(1),
            job: job.clone(),
            permit: permit.clone(),
            execution: execution.clone(),
            original_root: source.root.clone(),
            original_identity: source.identity.clone(),
            overlay_identity: overlay.identity,
            entries: source.entries.clone(),
        };
        write_record(&run_dir.join("overlay-baseline.json"), &baseline)?;
        chown_recursive(&workspace, execution_account())?;
        Ok(Self {
            run_dir: run_dir.to_path_buf(),
            baseline,
        })
    }

    pub(super) fn workspace(&self) -> PathBuf {
        self.run_dir.join("workspace")
    }

    pub(super) fn complete(&self, job: &p::WorkerJob, now: p::Timestamp) -> p::Result<()> {
        let intent_path = self.run_dir.join("overlay-writeback-intent.json");
        let receipt_path = self.run_dir.join("overlay-writeback-receipt.json");
        if intent_path.exists() || receipt_path.exists() {
            return Err(p::Error(
                "POSIX workspace writeback already has an outcome boundary".into(),
            ));
        }
        let baseline: Baseline = read_record(&self.run_dir.join("overlay-baseline.json"))?;
        if baseline != self.baseline || baseline.job != *job {
            return Err(p::Error(
                "POSIX workspace baseline binding is invalid".into(),
            ));
        }
        baseline.permit.validate_for(job, now)?;
        baseline
            .execution
            .validate_for(job, &baseline.permit, baseline.permit.issued_at)?;
        let original = Tree::capture(&baseline.original_root)?;
        if original.identity != baseline.original_identity
            || !same_contents(&original.entries, &baseline.entries)
        {
            return Err(p::Error(
                "POSIX workspace changed while the worker was running".into(),
            ));
        }
        let overlay = Tree::capture(&self.workspace())?;
        if overlay.identity != baseline.overlay_identity {
            return Err(p::Error("POSIX worker overlay identity changed".into()));
        }
        let changes = changed_entries(&baseline.entries, &overlay.entries)?;
        validate_write_scope(&baseline.execution.intent.parameters, &changes)?;
        let intent = WriteIntent {
            schema_version: p::SchemaVersion(1),
            baseline_digest: p::canonical_digest(&baseline)?,
            result_digest: content_digest(&overlay.entries)?,
            changes,
        };
        write_record(&intent_path, &intent)?;
        apply_changes(&baseline, &original, &overlay, &intent)?;
        let final_tree = Tree::capture(&baseline.original_root)?;
        if !same_contents(&final_tree.entries, &overlay.entries) {
            return Err(p::Error(
                "Linux workspace writeback verification failed".into(),
            ));
        }
        let receipt = WriteReceipt {
            schema_version: p::SchemaVersion(1),
            intent_digest: p::canonical_digest(&intent)?,
            baseline_digest: intent.baseline_digest.clone(),
            result_digest: intent.result_digest.clone(),
        };
        write_record(&receipt_path, &receipt)?;
        validate_receipt(&baseline, &intent, &receipt)
    }
}

fn changed_entries(
    baseline: &BTreeMap<String, Entry>,
    overlay: &BTreeMap<String, Entry>,
) -> p::Result<BTreeSet<String>> {
    let mut changed = BTreeSet::new();
    for key in baseline.keys().chain(overlay.keys()) {
        let differs = match (baseline.get(key), overlay.get(key)) {
            (Some(left), Some(right)) => !same_entry(left, right),
            _ => true,
        };
        if differs {
            changed.insert(key.clone());
        }
    }
    Ok(changed)
}

fn same_entry(left: &Entry, right: &Entry) -> bool {
    left.directory == right.directory && left.bytes == right.bytes && left.digest == right.digest
}

fn validate_write_scope(
    parameters: &p::ActionParameters,
    changes: &BTreeSet<String>,
) -> p::Result<()> {
    let (exact, prefix) = match parameters {
        p::ActionParameters::File {
            operation, path, ..
        } => {
            if *operation == p::FileOperation::Read {
                return if changes.is_empty() {
                    Ok(())
                } else {
                    Err(p::Error(
                        "POSIX worker read action changed its workspace".into(),
                    ))
                };
            }
            (true, normalize_relative(path)?)
        }
        p::ActionParameters::Shell { cwd, network, .. } => {
            if *network {
                return Err(p::Error(
                    "POSIX worker shell network access is not admitted".into(),
                ));
            }
            (false, normalize_relative(cwd.as_deref().unwrap_or(""))?)
        }
        p::ActionParameters::Pty(spec) => (false, normalize_relative(&spec.cwd)?),
        _ => {
            return Err(p::Error(
                "POSIX worker backend has no writeback contract".into(),
            ))
        }
    };
    if changes.iter().all(|key| {
        if exact {
            key == &prefix
        } else {
            prefix.is_empty() || key == &prefix || key.starts_with(&format!("{prefix}/"))
        }
    }) {
        Ok(())
    } else {
        Err(p::Error(
            "POSIX worker changed a path outside its approved scope".into(),
        ))
    }
}

fn normalize_relative(value: &str) -> p::Result<String> {
    if value.is_empty() {
        return Ok(String::new());
    }
    let mut parts = Vec::new();
    for component in Path::new(value).components() {
        match component {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .ok_or_else(|| p::Error("POSIX worker path is not valid UTF-8".into()))?,
            ),
            _ => {
                return Err(p::Error(
                    "POSIX worker path must be relative and normalized".into(),
                ))
            }
        }
    }
    Ok(parts.join("/"))
}

fn apply_changes(
    baseline: &Baseline,
    original: &Tree,
    overlay: &Tree,
    intent: &WriteIntent,
) -> p::Result<()> {
    let mut removed = intent
        .changes
        .iter()
        .filter(|key| !overlay.entries.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    removed.sort_by_key(|key| std::cmp::Reverse(key.len()));
    for key in removed {
        verify_original_entry(original, &baseline.entries, &key)?;
        let path = original.root.join(&key);
        let metadata = fs::symlink_metadata(&path).map_err(|_| writeback_error())?;
        if metadata.is_dir() {
            fs::remove_dir(&path).map_err(|_| writeback_error())?;
        } else {
            fs::remove_file(&path).map_err(|_| writeback_error())?;
        }
    }
    for key in &intent.changes {
        let Some(entry) = overlay.entries.get(key) else {
            continue;
        };
        if entry.directory {
            let path = original.root.join(key);
            if !path.exists() {
                fs::create_dir(&path).map_err(|_| writeback_error())?;
            }
            continue;
        }
        let target = original.root.join(key);
        if let Some(previous) = baseline.entries.get(key) {
            let current = fs::symlink_metadata(&target).map_err(|_| writeback_error())?;
            if current.is_dir()
                || super::posix_worker_fs::file_identity(&target)? != previous.identity
            {
                return Err(writeback_error());
            }
        } else if target.exists() {
            return Err(writeback_error());
        }
        let parent = target.parent().ok_or_else(writeback_error)?;
        fs::create_dir_all(parent).map_err(|_| writeback_error())?;
        let temp = parent.join(format!(".forme-write-{}", p::canonical_digest(key)?.0));
        let mut output = private_file(&temp)?;
        let mut input = File::open(overlay.root.join(key)).map_err(|_| writeback_error())?;
        copy_bounded(&mut input, &mut output, entry.bytes)?;
        output.sync_all().map_err(|_| writeback_error())?;
        fs::rename(&temp, &target).map_err(|_| writeback_error())?;
    }
    Ok(())
}

fn verify_original_entry(
    original: &Tree,
    baseline: &BTreeMap<String, Entry>,
    key: &str,
) -> p::Result<()> {
    let previous = baseline.get(key).ok_or_else(writeback_error)?;
    let current = original.entries.get(key).ok_or_else(writeback_error)?;
    if current.identity != previous.identity || current.directory != previous.directory {
        return Err(writeback_error());
    }
    Ok(())
}

fn content_digest(entries: &BTreeMap<String, Entry>) -> p::Result<p::SchemaDigest> {
    p::canonical_digest(
        &entries
            .iter()
            .map(|(key, entry)| (key, entry.directory, entry.bytes, &entry.digest))
            .collect::<Vec<_>>(),
    )
}

fn validate_receipt(
    baseline: &Baseline,
    intent: &WriteIntent,
    receipt: &WriteReceipt,
) -> p::Result<()> {
    if baseline.schema_version != p::SchemaVersion(1)
        || intent.schema_version != p::SchemaVersion(1)
        || receipt.schema_version != p::SchemaVersion(1)
        || intent.baseline_digest != p::canonical_digest(baseline)?
        || receipt.baseline_digest != intent.baseline_digest
        || receipt.intent_digest != p::canonical_digest(intent)?
        || receipt.result_digest != intent.result_digest
    {
        return Err(writeback_error());
    }
    Ok(())
}

fn writeback_error() -> p::Error {
    super::posix_worker_fs::writeback_error()
}
