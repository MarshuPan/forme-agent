//! Write-ahead intent, handle-bound effects, and verified terminal receipt.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::time::{Duration, Instant};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::authorization::content_manifest;
use super::custody::{inspect, open_directory, open_file, unavailable, Tree};
use super::{acl, read_record, write_record, Baseline, PreparedOverlay, BASELINE, INTENT, RECEIPT};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Intent {
    schema_version: p::SchemaVersion,
    baseline_digest: p::SchemaDigest,
    result_digest: p::SchemaDigest,
    changes: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    schema_version: p::SchemaVersion,
    intent_digest: p::SchemaDigest,
    baseline_digest: p::SchemaDigest,
    result_digest: p::SchemaDigest,
}

pub(super) struct Preflight {
    original: Tree,
    overlay: Tree,
    intent: Intent,
    deadline: Instant,
}

pub(super) fn preflight(
    prepared: &PreparedOverlay,
    job: &p::WorkerJob,
    now: p::Timestamp,
) -> p::Result<Preflight> {
    let started = Instant::now();
    let baseline: Baseline = read_record(&prepared.run_dir.join(BASELINE))?;
    if baseline != prepared.baseline
        || baseline.schema_version != p::SchemaVersion(1)
        || &baseline.job != job
    {
        return Err(unavailable());
    }
    baseline.permit.validate_for(job, now)?;
    baseline
        .execution
        .validate_for(job, &baseline.permit, baseline.permit.issued_at)?;
    let remaining = u64::try_from(job.deadline.saturating_sub(now)).map_err(|_| unavailable())?;
    let deadline = started
        .checked_add(Duration::from_millis(remaining))
        .ok_or_else(unavailable)?;
    let overlay = Tree::open(&prepared.workspace(), &BTreeSet::new())?;
    if overlay.identity != baseline.overlay_identity {
        return Err(unavailable());
    }
    let changes = prepared
        .scope
        .validate_changes(&baseline.entries, &overlay.entries)?;
    let original = Tree::open(&baseline.original_root, &changes)?;
    if original.identity != baseline.original_identity || original.entries != baseline.entries {
        return Err(p::Error(
            "workspace changed after its snapshot; writeback rejected".into(),
        ));
    }
    if Instant::now() >= deadline {
        return Err(unavailable());
    }
    let intent = Intent {
        schema_version: p::SchemaVersion(1),
        baseline_digest: p::canonical_digest(&baseline)?,
        result_digest: content_manifest(&overlay.entries)?,
        changes,
    };
    Ok(Preflight {
        original,
        overlay,
        intent,
        deadline,
    })
}

pub(super) fn apply(
    prepared: &PreparedOverlay,
    mut state: Preflight,
    _now: p::Timestamp,
) -> p::Result<()> {
    write_record(&prepared.run_dir.join(INTENT), &state.intent)?;
    apply_files(&mut state)?;
    state.original.verify_final(&state.overlay.entries)?;
    if Instant::now() >= state.deadline {
        return Err(unavailable());
    }
    let receipt = Receipt {
        schema_version: p::SchemaVersion(1),
        intent_digest: p::canonical_digest(&state.intent)?,
        baseline_digest: state.intent.baseline_digest.clone(),
        result_digest: state.intent.result_digest.clone(),
    };
    write_record(&prepared.run_dir.join(RECEIPT), &receipt)?;
    validate_receipt(&prepared.baseline, &state.intent, &receipt)
}

fn apply_files(state: &mut Preflight) -> p::Result<()> {
    let mut removed = state
        .intent
        .changes
        .iter()
        .filter(|k| !state.overlay.entries.contains_key(*k))
        .cloned()
        .collect::<Vec<_>>();
    removed.sort_by_key(|key| std::cmp::Reverse(key.len()));
    for key in removed {
        if Instant::now() >= state.deadline {
            return Err(unavailable());
        }
        let file = state
            .original
            .handles
            .remove(&key)
            .ok_or_else(unavailable)?;
        acl::mark_deleted(&file)?;
        drop(file);
    }
    // Parent paths sort before descendants. All existing ancestors remain
    // pinned; new parents are opened immediately and retained through receipt.
    for key in &state.intent.changes {
        let Some(entry) = state.overlay.entries.get(key) else {
            continue;
        };
        if Instant::now() >= state.deadline {
            return Err(unavailable());
        }
        if entry.directory {
            if !state.original.handles.contains_key(key) {
                let path = state.original.root.join(key);
                fs::create_dir(&path).map_err(|_| unavailable())?;
                state
                    .original
                    .handles
                    .insert(key.clone(), open_directory(&path, false)?);
            }
            continue;
        }
        let data = state.overlay.contents(key)?;
        if !state.original.handles.contains_key(key) {
            let path = state.original.root.join(key);
            state
                .original
                .handles
                .insert(key.clone(), open_file(&path, true, true)?);
        }
        let mut file = state.original.handles.get(key).ok_or_else(unavailable)?;
        // Do not reopen by mutable path after validation. A partial write is
        // an Unknown intent, never a reason to retry this operation.
        file.seek(SeekFrom::Start(0)).map_err(|_| unavailable())?;
        file.write_all(&data).map_err(|_| unavailable())?;
        file.set_len(data.len() as u64).map_err(|_| unavailable())?;
        file.sync_all().map_err(|_| unavailable())?;
        let actual = inspect(file)?;
        if actual.digest != entry.digest || actual.bytes != entry.bytes {
            return Err(unavailable());
        }
    }
    Ok(())
}

pub(super) fn validate_receipt(
    baseline: &Baseline,
    intent: &Intent,
    receipt: &Receipt,
) -> p::Result<()> {
    if baseline.schema_version != p::SchemaVersion(1)
        || intent.schema_version != p::SchemaVersion(1)
        || receipt.schema_version != p::SchemaVersion(1)
        || intent.baseline_digest != p::canonical_digest(baseline)?
        || receipt.baseline_digest != intent.baseline_digest
        || receipt.intent_digest != p::canonical_digest(intent)?
        || receipt.result_digest != intent.result_digest
    {
        return Err(unavailable());
    }
    p::validate_product_digest(&intent.result_digest)?;
    Ok(())
}

pub(super) fn retained_receipt(
    baseline: &Baseline,
    receipt: &Receipt,
) -> super::super::sandbox_lease::SandboxWritebackReceipt {
    super::super::sandbox_lease::SandboxWritebackReceipt {
        schema_version: p::SchemaVersion(1),
        job_id: baseline.job.job_id.clone(),
        plan_digest: p::SchemaDigest(baseline.job.plan_digest.0.clone()),
        intent_digest: receipt.intent_digest.clone(),
        baseline_digest: receipt.baseline_digest.clone(),
        result_digest: receipt.result_digest.clone(),
    }
}

#[cfg(test)]
pub(super) fn leave_intent_for_test(
    prepared: &PreparedOverlay,
    job: &p::WorkerJob,
    now: p::Timestamp,
) {
    let state = preflight(prepared, job, now).expect("valid preflight");
    write_record(&prepared.run_dir.join(INTENT), &state.intent).expect("durable test intent");
}
