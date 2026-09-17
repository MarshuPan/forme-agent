use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use crate::{RemoteInnerDriver, RemoteInnerOutcome};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryMutationState {
    pub schema_version: p::SchemaVersion,
    pub ordinal: u64,
    pub value_digest: p::SchemaDigest,
}

/// Narrow repository-owned golden driver. It accepts one logical File/Write
/// operation and commits only a digest plus monotonic ordinal to its local
/// state file; the authority never receives a host path or raw output.
pub struct RepositoryMutationDriver {
    logical_path: String,
    state_path: PathBuf,
    lock: Mutex<()>,
    calls: AtomicUsize,
}

impl RepositoryMutationDriver {
    pub fn new(logical_path: impl Into<String>, state_path: impl AsRef<Path>) -> p::Result<Self> {
        let logical_path = logical_path.into();
        let state_path = state_path.as_ref().to_path_buf();
        if logical_path.trim().is_empty()
            || Path::new(&logical_path).is_absolute()
            || logical_path.split(['/', '\\']).any(|part| part == "..")
            || state_path.as_os_str().is_empty()
            || state_path.parent().is_none_or(|parent| !parent.is_dir())
        {
            return Err(p::Error(
                "repository mutation fixture configuration is incomplete".into(),
            ));
        }
        Ok(Self {
            logical_path,
            state_path,
            lock: Mutex::new(()),
            calls: AtomicUsize::new(0),
        })
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    pub fn state(&self) -> p::Result<Option<RepositoryMutationState>> {
        if !self.state_path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&self.state_path)
            .map_err(|_| p::Error("repository mutation state cannot be read".into()))?;
        let state: RepositoryMutationState = serde_json::from_slice(&bytes)
            .map_err(|_| p::Error("repository mutation state is malformed".into()))?;
        if state.schema_version.0 == 0
            || state.ordinal == 0
            || state.value_digest.0.trim().is_empty()
        {
            return Err(p::Error("repository mutation state is incomplete".into()));
        }
        Ok(Some(state))
    }
}

impl RemoteInnerDriver for RepositoryMutationDriver {
    fn execute(
        &self,
        operation: &p::RemoteOperation,
        credential: Option<&crate::ResolvedSecret>,
    ) -> p::Result<RemoteInnerOutcome> {
        operation.validate()?;
        if credential.is_some() {
            return Err(p::Error(
                "repository mutation fixture does not accept credentials".into(),
            ));
        }
        let p::ActionParameters::File {
            operation: p::FileOperation::Write,
            path,
            content: Some(content),
        } = &operation.parameters
        else {
            return Err(p::Error(
                "repository mutation driver accepts only a bounded file write".into(),
            ));
        };
        if operation.backend != p::BackendKind::File
            || path != &self.logical_path
            || content.is_empty()
            || content.len() > 65_536
        {
            return Err(p::Error(
                "repository mutation operation is outside its profile".into(),
            ));
        }
        let _guard = self
            .lock
            .lock()
            .map_err(|_| p::Error("repository mutation state is unavailable".into()))?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        let previous = self.state()?.map(|state| state.ordinal).unwrap_or(0);
        let ordinal = previous
            .checked_add(1)
            .ok_or_else(|| p::Error("repository mutation ordinal is exhausted".into()))?;
        let value_digest = p::canonical_digest(content)?;
        let state = RepositoryMutationState {
            schema_version: p::M4_SCHEMA_VERSION,
            ordinal,
            value_digest: value_digest.clone(),
        };
        let bytes = serde_json::to_vec(&state)
            .map_err(|_| p::Error("repository mutation state cannot be encoded".into()))?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.state_path)
            .map_err(|_| p::Error("repository mutation state cannot be opened".into()))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| p::Error("repository mutation state cannot be committed".into()))?;
        Ok(RemoteInnerOutcome {
            outcome: p::RemoteReceiptOutcome::Completed,
            result_digest: Some(value_digest),
            observations: vec![p::EvidenceRef(format!("mutation:ordinal:{ordinal}"))],
        })
    }
}
