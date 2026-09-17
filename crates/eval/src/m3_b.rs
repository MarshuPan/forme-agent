use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

const ARTIFACT_TYPE: &str = "m3-b-long-horizon";
const DIGEST_DOMAIN: &[u8] = b"forme-m3-b-long-horizon-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LongHorizonArtifactDisposition {
    Completed,
    Limited,
    Cancelled,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongHorizonArtifactCheckpoint {
    pub schema_version: p::SchemaVersion,
    pub index: u16,
    pub run: p::RunId,
    pub checkpoint: p::GoalCheckpointRef,
    pub snapshot: p::EvolutionSnapshotRef,
    pub strategy_versions: Vec<p::StrategyVersionRef>,
    pub status: p::RunStatus,
    pub outward_run: Option<p::RunId>,
    pub outward_event_refs: Vec<p::EventId>,
    pub outward_event_kinds: Vec<p::EventKind>,
    pub event_refs: Vec<p::EventId>,
    pub event_kinds: Vec<p::EventKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongHorizonArtifactBody {
    pub schema_version: p::SchemaVersion,
    pub goal_frame: p::GoalFrameRef,
    pub scope: p::Scope,
    pub maximum_checkpoints: u16,
    pub disposition: LongHorizonArtifactDisposition,
    pub checkpoints: Vec<LongHorizonArtifactCheckpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableLongHorizonArtifact {
    pub schema_version: p::SchemaVersion,
    pub artifact_type: String,
    pub digest: p::SchemaDigest,
    pub body: LongHorizonArtifactBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M3BArtifactReceipt {
    pub schema_version: p::SchemaVersion,
    pub path: PathBuf,
    pub content_ref: p::ContentRef,
    pub digest: p::SchemaDigest,
}

pub struct M3BArtifactStore {
    root: PathBuf,
}

impl M3BArtifactStore {
    pub fn new(root: impl AsRef<Path>) -> p::Result<Self> {
        fs::create_dir_all(root.as_ref())
            .map_err(|error| p::Error(format!("failed to create M3-B artifact root: {error}")))?;
        let root = fs::canonicalize(root.as_ref())
            .map_err(|error| p::Error(format!("failed to resolve M3-B artifact root: {error}")))?;
        if !root.is_dir() {
            return Err(p::Error("M3-B artifact root is not a directory".into()));
        }
        Ok(Self { root })
    }

    pub fn write(&self, body: &LongHorizonArtifactBody) -> p::Result<M3BArtifactReceipt> {
        validate_body(body)?;
        let digest = digest_body(body)?;
        let artifact = PortableLongHorizonArtifact {
            schema_version: p::SchemaVersion(1),
            artifact_type: ARTIFACT_TYPE.into(),
            digest: p::SchemaDigest(format!("fnv64:{digest}")),
            body: body.clone(),
        };
        let value = serde_json::to_value(&artifact)
            .map_err(|error| p::Error(format!("failed to inspect M3-B artifact: {error}")))?;
        reject_sensitive_value(&value)?;
        let bytes = serde_json::to_vec_pretty(&artifact)
            .map_err(|error| p::Error(format!("failed to encode M3-B artifact: {error}")))?;
        let path = self.root.join(format!("long-horizon-{digest}.json"));
        if path.parent() != Some(self.root.as_path()) {
            return Err(p::Error("M3-B artifact path escapes its root".into()));
        }
        if path.exists() {
            let existing = fs::read(&path).map_err(|error| {
                p::Error(format!("failed to read existing M3-B artifact: {error}"))
            })?;
            if existing != bytes {
                return Err(p::Error(
                    "M3-B artifact digest path is bound to different content".into(),
                ));
            }
        } else {
            fs::write(&path, bytes)
                .map_err(|error| p::Error(format!("failed to write M3-B artifact: {error}")))?;
        }
        self.verify(path)
    }

    pub fn verify(&self, path: impl AsRef<Path>) -> p::Result<M3BArtifactReceipt> {
        let path = fs::canonicalize(path.as_ref())
            .map_err(|error| p::Error(format!("failed to resolve M3-B artifact: {error}")))?;
        if path.parent() != Some(self.root.as_path()) {
            return Err(p::Error(
                "M3-B artifact path escapes its configured root".into(),
            ));
        }
        let bytes = fs::read(&path)
            .map_err(|error| p::Error(format!("failed to read M3-B artifact: {error}")))?;
        let artifact: PortableLongHorizonArtifact = serde_json::from_slice(&bytes)
            .map_err(|error| p::Error(format!("failed to parse M3-B artifact: {error}")))?;
        if artifact.schema_version != p::SchemaVersion(1) || artifact.artifact_type != ARTIFACT_TYPE
        {
            return Err(p::Error(
                "M3-B artifact type or schema is unsupported".into(),
            ));
        }
        validate_body(&artifact.body)?;
        let digest = digest_body(&artifact.body)?;
        if artifact.digest != p::SchemaDigest(format!("fnv64:{digest}"))
            || path.file_name().and_then(|name| name.to_str())
                != Some(format!("long-horizon-{digest}.json").as_str())
        {
            return Err(p::Error(
                "M3-B artifact content digest or filename does not match".into(),
            ));
        }
        let value = serde_json::to_value(&artifact)
            .map_err(|error| p::Error(format!("failed to inspect M3-B artifact: {error}")))?;
        reject_sensitive_value(&value)?;
        Ok(M3BArtifactReceipt {
            schema_version: p::SchemaVersion(1),
            path,
            content_ref: p::ContentRef(format!("artifact:fnv64:{digest}")),
            digest: artifact.digest,
        })
    }

    pub fn verify_complete_set(&self) -> p::Result<M3BArtifactReceipt> {
        let entries = fs::read_dir(&self.root)
            .map_err(|error| p::Error(format!("failed to list M3-B artifact root: {error}")))?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|error| p::Error(format!("failed to inspect M3-B artifact root: {error}")))?;
        if entries.len() != 1
            || !entries[0]
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        {
            return Err(p::Error(
                "M3-B artifact root must contain exactly one regular artifact".into(),
            ));
        }
        self.verify(entries[0].path())
    }
}

fn validate_body(body: &LongHorizonArtifactBody) -> p::Result<()> {
    if body.schema_version != p::SchemaVersion(1)
        || body.goal_frame.0.trim().is_empty()
        || body.scope.0.trim().is_empty()
        || body.maximum_checkpoints == 0
        || body.checkpoints.len() != usize::from(body.maximum_checkpoints)
        || body.checkpoints.len() < 3
        || body.disposition != LongHorizonArtifactDisposition::Cancelled
    {
        return Err(p::Error(
            "M3-B long-horizon artifact boundary is incomplete".into(),
        ));
    }
    let runs = body
        .checkpoints
        .iter()
        .map(|checkpoint| &checkpoint.run)
        .collect::<BTreeSet<_>>();
    if runs.len() != body.checkpoints.len() {
        return Err(p::Error("M3-B checkpoint run is duplicated".into()));
    }
    let mut outward_count = 0;
    let mut saw_foreground_yield = false;
    for (index, checkpoint) in body.checkpoints.iter().enumerate() {
        if checkpoint.schema_version.0 == 0
            || usize::from(checkpoint.index) != index
            || checkpoint.run.0.trim().is_empty()
            || checkpoint.checkpoint.0.trim().is_empty()
            || checkpoint.snapshot.0.trim().is_empty()
            || checkpoint.strategy_versions.is_empty()
            || checkpoint
                .strategy_versions
                .iter()
                .any(|version| version.0.trim().is_empty())
            || checkpoint.status != p::RunStatus::Complete
            || checkpoint.event_refs.is_empty()
            || checkpoint.event_refs.len() != checkpoint.event_kinds.len()
        {
            return Err(p::Error("M3-B checkpoint artifact is incomplete".into()));
        }
        for required in [
            p::EventKind::SessionBound,
            p::EventKind::GoalFramed,
            p::EventKind::OrchestrationRouteCreated,
            p::EventKind::MemoryNodeAppended,
            p::EventKind::VerificationFinished,
            p::EventKind::RunComplete,
        ] {
            if !checkpoint.event_kinds.contains(&required) {
                return Err(p::Error(format!(
                    "M3-B checkpoint is missing required event {required:?}"
                )));
            }
        }
        saw_foreground_yield |= checkpoint.event_kinds.contains(&p::EventKind::RunWaiting)
            && checkpoint.event_kinds.contains(&p::EventKind::RunResumed);
        if checkpoint.outward_run.is_some() {
            outward_count += 1;
            if checkpoint.outward_event_refs.is_empty()
                || checkpoint.outward_event_refs.len() != checkpoint.outward_event_kinds.len()
                || !is_subsequence(
                    &checkpoint.outward_event_kinds,
                    &[
                        p::EventKind::ApprovalRequested,
                        p::EventKind::ApprovalResolved,
                        p::EventKind::CompetenceGateEvaluated,
                        p::EventKind::ActionPlanned,
                        p::EventKind::ActionStarted,
                        p::EventKind::ActionCompleted,
                        p::EventKind::VerificationFinished,
                    ],
                )
            {
                return Err(p::Error(
                    "M3-B outward artifact lacks its governed event sequence".into(),
                ));
            }
        } else if !checkpoint.outward_event_refs.is_empty()
            || !checkpoint.outward_event_kinds.is_empty()
        {
            return Err(p::Error(
                "M3-B checkpoint invents outward events without an outward run".into(),
            ));
        }
    }
    if !saw_foreground_yield || outward_count != 1 {
        return Err(p::Error(
            "M3-B artifact lacks foreground yield or exact outward action lineage".into(),
        ));
    }
    let first = &body.checkpoints[0];
    let middle = &body.checkpoints[1];
    let last = body.checkpoints.last().expect("at least three checkpoints");
    if first.snapshot == middle.snapshot
        || middle.snapshot == last.snapshot
        || first.strategy_versions != last.strategy_versions
        || first.strategy_versions == middle.strategy_versions
    {
        return Err(p::Error(
            "M3-B artifact does not prove next-checkpoint activation and rollback".into(),
        ));
    }
    Ok(())
}

fn is_subsequence(actual: &[p::EventKind], expected: &[p::EventKind]) -> bool {
    let mut cursor = 0;
    for expected_kind in expected {
        let Some(offset) = actual[cursor..]
            .iter()
            .position(|kind| kind == expected_kind)
        else {
            return false;
        };
        cursor += offset + 1;
    }
    true
}

fn digest_body(body: &LongHorizonArtifactBody) -> p::Result<String> {
    let bytes = serde_json::to_vec(body)
        .map_err(|error| p::Error(format!("failed to canonicalize M3-B artifact: {error}")))?;
    Ok(format!("{:016x}", fnv64(DIGEST_DOMAIN, &bytes)))
}

fn fnv64(domain: &[u8], bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in domain.iter().chain(bytes) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn reject_sensitive_value(value: &serde_json::Value) -> p::Result<()> {
    match value {
        serde_json::Value::String(value) => reject_sensitive_string(value),
        serde_json::Value::Array(values) => {
            for value in values {
                reject_sensitive_value(value)?;
            }
            Ok(())
        }
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                reject_sensitive_string(key)?;
                reject_sensitive_value(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn reject_sensitive_string(value: &str) -> p::Result<()> {
    let lower = value.to_ascii_lowercase();
    if [
        "resolvedsecret",
        "api_key",
        "apikey",
        "credential",
        "authorization:",
        "bearer ",
        "private-key",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || is_absolute_private_path(value)
    {
        return Err(p::Error(
            "M3-B artifact contains a sensitive marker or private absolute path".into(),
        ));
    }
    Ok(())
}

fn is_absolute_private_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}
