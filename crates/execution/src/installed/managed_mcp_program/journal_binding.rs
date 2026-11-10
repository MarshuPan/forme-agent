//! Original log identity and ancestor custody; partial initialization is closed.
use super::{invalid, Identity};
use crate::installed::{
    management_repair::storage::ReadCustody, sandbox_lease::SandboxFileIdentity,
    windows_appcontainer_acl as acl,
};
use forme_protocol as p;
use serde::{Deserialize, Serialize};
use std::{fs::File, path::Path};

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema_version: p::SchemaVersion,
    identity_digest: p::SchemaDigest,
    journal_identity: SandboxFileIdentity,
}
pub(super) fn preflight(path: &Path) -> p::Result<()> {
    let binding = path.with_extension("binding.json");
    crate::installed::reject_linked_ancestors(&binding)?;
    if binding.try_exists().map_err(|_| invalid())? && !path.try_exists().map_err(|_| invalid())? {
        return Err(invalid());
    }
    Ok(())
}
pub(super) fn pin(
    path: &Path,
    file: &File,
    identity: &Identity,
    fresh: bool,
) -> p::Result<ReadCustody> {
    let expected = Binding {
        schema_version: p::SchemaVersion(1),
        identity_digest: p::canonical_digest(&(
            "forme.managed-mcp.program.log-binding.v1",
            identity,
        ))?,
        journal_identity: acl::inspect_file(file)?.0,
    };
    let binding = path.with_extension("binding.json");
    if fresh {
        crate::installed::process::write_private_json(&binding, &expected)?;
    }
    let mut custody = ReadCustody::default();
    let actual: Binding = custody.json(&binding, 8192)?;
    if actual != expected || acl::file_identity(path)? != expected.journal_identity {
        return Err(invalid());
    }
    Ok(custody)
}
