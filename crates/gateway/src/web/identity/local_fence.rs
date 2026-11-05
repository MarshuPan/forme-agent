//! Installed maintenance invalidates sessions without replacing the account.
#[cfg(test)]
use super::WebIdentityStore;
use super::{account, storage, storage_error, AuthError, AuthResult};
use forme_protocol as p;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Narrow local maintenance exception to the Web singleton. Does not create,
/// migrate or expose the database; SQLite serializes this exact transaction.
pub(crate) fn fence_installed_sessions(
    directory: &Path,
    install: &str,
    issuer: &[u8; 32],
    owner: &p::OwnerPrincipalRef,
    operation: &str,
    command_digest: &p::SchemaDigest,
) -> AuthResult<p::AuthRevision> {
    crate::web::config::check_path(directory).map_err(|_| AuthError::InvalidInput)?;
    let path = directory.join("identity.sqlite");
    for suffix in [
        "identity.sqlite",
        "identity.sqlite-wal",
        "identity.sqlite-shm",
        "identity.sqlite-journal",
    ] {
        crate::web::config::check_path(&directory.join(suffix))
            .map_err(|_| AuthError::RecoveryRequired)?;
    }
    let mut connection =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(storage_error)?;
    connection
        .busy_timeout(std::time::Duration::from_secs(2))
        .map_err(storage_error)?;
    connection
        .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA synchronous=FULL;")
        .map_err(storage_error)?;
    let app: i64 = connection
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .map_err(storage_error)?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(storage_error)?;
    let bound: (String, String) = connection
        .query_row(
            "SELECT install,claim_key FROM binding WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(storage_error)?;
    let expected_key = p::canonical_digest(&("web-claim-issuer-v1", issuer))
        .map_err(|_| AuthError::Unavailable)?;
    let integrity: String = connection
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(storage_error)?;
    if app != 0x46415531
        || !matches!(version, 6..=10)
        || bound != (install.to_owned(), expected_key.0)
        || integrity != "ok"
    {
        return Err(AuthError::RecoveryRequired);
    }
    apply(&mut connection, owner, operation, command_digest)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MaintenanceFence {
    schema_version: p::SchemaVersion,
    owner: p::OwnerPrincipalRef,
    command_digest: p::SchemaDigest,
    revision: p::AuthRevision,
}

#[cfg(test)]
impl WebIdentityStore {
    /// Only the OS-protected local maintenance composition may call this.
    /// Idempotency is command-bound; a lost receipt cannot rotate twice.
    pub(crate) fn fence_for_local_maintenance(
        &self,
        owner: &p::OwnerPrincipalRef,
        operation: &str,
        command_digest: &p::SchemaDigest,
    ) -> AuthResult<p::AuthRevision> {
        let mut db = self.database()?;
        apply(&mut db.connection, owner, operation, command_digest)
    }
}

fn apply(
    connection: &mut rusqlite::Connection,
    owner: &p::OwnerPrincipalRef,
    operation: &str,
    command_digest: &p::SchemaDigest,
) -> AuthResult<p::AuthRevision> {
    p::validate_product_id(operation).map_err(|_| AuthError::InvalidInput)?;
    p::validate_product_digest(command_digest).map_err(|_| AuthError::InvalidInput)?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(storage_error)?;
    let mut current = account(&tx)?.ok_or(AuthError::Unauthenticated)?;
    if current.owner != *owner {
        return Err(AuthError::Conflict);
    }
    let existing: Option<String> = tx
        .query_row(
            "SELECT value FROM auth_maintenance WHERE operation=?1",
            [operation],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(json) = existing {
        let record: MaintenanceFence =
            serde_json::from_str(&json).map_err(|_| AuthError::Conflict)?;
        if record.schema_version != p::SchemaVersion(1)
            || record.owner != *owner
            || record.command_digest != *command_digest
            || record.revision != current.revision()
        {
            return Err(AuthError::Conflict);
        }
        return Ok(record.revision);
    }
    let next = current
        .auth_epoch
        .checked_add(1)
        .ok_or(AuthError::RecoveryRequired)?;
    let source = p::canonical_digest(&(
        "forme-local-maintenance-fence-v1",
        operation,
        command_digest,
        owner,
    ))
    .map_err(|_| AuthError::Unavailable)?;
    storage::enqueue_fence(
        &tx,
        owner,
        &source.0,
        current.auth_epoch,
        None,
        next,
        super::now(),
    )?;
    current.auth_epoch = next;
    storage::save_account(&tx, &current)?;
    tx.execute("DELETE FROM sessions", [])
        .map_err(storage_error)?;
    tx.execute("DELETE FROM reauth", [])
        .map_err(storage_error)?;
    let revision = current.revision();
    let record = MaintenanceFence {
        schema_version: p::SchemaVersion(1),
        owner: owner.clone(),
        command_digest: command_digest.clone(),
        revision: revision.clone(),
    };
    let json = serde_json::to_string(&record).map_err(|_| AuthError::Unavailable)?;
    tx.execute(
        "INSERT INTO auth_maintenance(operation,value) VALUES(?1,?2)",
        rusqlite::params![operation, json],
    )
    .map_err(storage_error)?;
    tx.commit().map_err(storage_error)?;
    Ok(revision)
}
