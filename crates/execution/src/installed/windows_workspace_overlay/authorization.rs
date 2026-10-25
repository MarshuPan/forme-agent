//! Derive the maximum write set from the already-bound final action.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use forme_protocol as p;

use super::super::reject_linked_ancestors;
use super::custody::{hash, semantic, unavailable, validate_name, Entry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WriteScope {
    File {
        path: String,
        operation: p::FileOperation,
        content: Option<p::SchemaDigest>,
    },
    Tree(String),
}

impl WriteScope {
    pub fn derive(root: &Path, execution: &p::WorkerExecutionSpec) -> p::Result<Self> {
        match (&execution.backend, &execution.intent.parameters) {
            (
                p::BackendKind::File,
                p::ActionParameters::File {
                    path,
                    operation,
                    content,
                },
            ) => {
                match operation {
                    p::FileOperation::Write if content.is_none() => return Err(unavailable()),
                    p::FileOperation::Read | p::FileOperation::Delete if content.is_some() => {
                        return Err(unavailable())
                    }
                    _ => {}
                }
                Ok(Self::File {
                    path: relative(root, path)?,
                    operation: *operation,
                    content: content.as_deref().map(hash),
                })
            }
            (
                p::BackendKind::Shell,
                p::ActionParameters::Shell {
                    cwd: Some(cwd),
                    network: false,
                    ..
                },
            ) => Ok(Self::Tree(relative(root, cwd)?)),
            (p::BackendKind::Pty, p::ActionParameters::Pty(spec)) => {
                Ok(Self::Tree(relative(root, &spec.cwd)?))
            }
            _ => Err(p::Error(
                "worker action has no supported private-workspace scope".into(),
            )),
        }
    }

    pub fn validate_changes(
        &self,
        before: &BTreeMap<String, Entry>,
        after: &BTreeMap<String, Entry>,
    ) -> p::Result<BTreeSet<String>> {
        let changed = before
            .keys()
            .chain(after.keys())
            .filter(|key| match (before.get(*key), after.get(*key)) {
                (Some(a), Some(b)) => {
                    a.directory != b.directory || a.bytes != b.bytes || a.digest != b.digest
                }
                _ => true,
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        // Type replacement is ambiguous (and could hide a directory subtree).
        if changed.iter().any(|key| {
            before
                .get(key)
                .zip(after.get(key))
                .is_some_and(|(a, b)| a.directory != b.directory)
        }) {
            return Err(unavailable());
        }
        match self {
            Self::Tree(prefix) => {
                if changed.iter().any(|key| {
                    !prefix.is_empty() && key != prefix && !key.starts_with(&format!("{prefix}/"))
                }) {
                    return Err(unavailable());
                }
            }
            Self::File {
                path,
                operation,
                content,
            } => {
                if changed.iter().any(|key| key != path) {
                    return Err(unavailable());
                }
                match operation {
                    p::FileOperation::Read if !changed.is_empty() => return Err(unavailable()),
                    p::FileOperation::Write => {
                        if !after
                            .get(path)
                            .is_some_and(|e| !e.directory && &e.digest == content)
                        {
                            return Err(unavailable());
                        }
                    }
                    p::FileOperation::Delete
                        if after.contains_key(path)
                            || before.get(path).is_some_and(|e| e.directory) =>
                    {
                        return Err(unavailable())
                    }
                    _ => {}
                }
            }
        }
        Ok(changed)
    }
}

pub(super) fn relative(root: &Path, input: &str) -> p::Result<String> {
    let path = Path::new(input);
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
    {
        return Err(unavailable());
    }
    reject_linked_ancestors(path)?;
    let resolved = if path.exists() {
        std::fs::canonicalize(path).map_err(|_| unavailable())?
    } else {
        let parent = std::fs::canonicalize(path.parent().ok_or_else(unavailable)?)
            .map_err(|_| unavailable())?;
        parent.join(path.file_name().ok_or_else(unavailable)?)
    };
    let relative = resolved.strip_prefix(root).map_err(|_| unavailable())?;
    let mut segments = Vec::new();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(unavailable());
        };
        let name = name.to_str().ok_or_else(unavailable)?;
        validate_name(name)?;
        segments.push(name);
    }
    Ok(segments.join("/"))
}

pub(super) fn content_manifest(entries: &BTreeMap<String, Entry>) -> p::Result<p::SchemaDigest> {
    p::canonical_digest(&("forme-workspace-content-v1", semantic(entries)))
}
