//! One OS-locked authentication writer, separate from all brain data.
use std::fs::{File, OpenOptions};
use std::path::Path;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{account, storage_error, AuthError, AuthResult, PasswordRecord};
use forme_protocol as p;

const APPLICATION_ID: i64 = 0x46415531;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Account {
    pub schema_version: p::SchemaVersion,
    pub account_ref: String,
    pub owner: p::OwnerPrincipalRef,
    pub login_name: String,
    pub password: PasswordRecord,
    pub credential_revision: u64,
    pub auth_epoch: u64,
    pub claim_operation: String,
    pub claim_candidate: String,
    pub claim_commitment: p::SchemaDigest,
    pub created_at: p::Timestamp,
}

impl Account {
    pub(super) fn revision(&self) -> p::AuthRevision {
        p::AuthRevision {
            schema_version: p::SchemaVersion(1),
            credential_revision: self.credential_revision,
            auth_epoch: self.auth_epoch,
        }
    }

    pub(super) fn view(&self, idle: i64, absolute: i64) -> p::WebSessionView {
        p::WebSessionView {
            schema_version: p::SchemaVersion(1),
            account_ref: self.account_ref.clone(),
            owner: self.owner.clone(),
            login_name: self.login_name.clone(),
            auth_epoch: self.auth_epoch,
            idle_expires_at: idle,
            absolute_expires_at: absolute,
        }
    }
}

pub(super) struct Database {
    pub connection: Connection,
    pub install: String,
    _lock: File,
}

impl Database {
    pub fn open(directory: &Path, install: &str, claim_key: &[u8; 32]) -> AuthResult<Self> {
        let key_digest = p::canonical_digest(&("web-claim-issuer-v1", claim_key))
            .map_err(|_| AuthError::Unavailable)?;
        check_path(directory)?;
        if !directory.is_dir() || directory.parent().is_none() {
            return Err(AuthError::InvalidInput);
        }
        let path = directory.join("identity.sqlite");
        let lock_path = directory.join("identity.lock");
        for candidate in [
            path.clone(),
            lock_path.clone(),
            directory.join("identity.sqlite-wal"),
            directory.join("identity.sqlite-shm"),
            directory.join("identity.sqlite-journal"),
        ] {
            check_path(&candidate)?;
            if candidate.exists() && !candidate.is_file() {
                return Err(AuthError::RecoveryRequired);
            }
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| AuthError::Unavailable)?;
        lock.try_lock().map_err(|_| AuthError::Busy)?;
        let existed = path.exists();
        if existed {
            let reader = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|_| AuthError::RecoveryRequired)?;
            let application: i64 = reader
                .pragma_query_value(None, "application_id", |r| r.get(0))
                .map_err(|_| AuthError::RecoveryRequired)?;
            let version: i64 = reader
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .map_err(|_| AuthError::RecoveryRequired)?;
            if application != APPLICATION_ID || version != 1 {
                return Err(AuthError::RecoveryRequired);
            }
        } else {
            // A crash before first commit leaves an explicit recovery case,
            // rather than reopening an unknown empty file as a new account.
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| AuthError::Unavailable)?
                .sync_all()
                .map_err(|_| AuthError::Unavailable)?;
        }
        let mut connection = Connection::open(&path).map_err(storage_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(2))
            .map_err(storage_error)?;
        if !existed {
            let tx = connection.transaction().map_err(storage_error)?;
            tx.execute_batch(
                "PRAGMA application_id = 1178686769; PRAGMA user_version = 1;
                 CREATE TABLE binding (singleton INTEGER PRIMARY KEY CHECK(singleton=1), install TEXT NOT NULL, claim_key TEXT NOT NULL);
                 CREATE TABLE account (singleton INTEGER PRIMARY KEY CHECK(singleton=1), value TEXT NOT NULL);
                 CREATE TABLE prepared (singleton INTEGER PRIMARY KEY CHECK(singleton=1), value TEXT NOT NULL);
                 CREATE TABLE sessions (digest TEXT PRIMARY KEY, csrf TEXT NOT NULL, epoch INTEGER NOT NULL,
                     issued INTEGER NOT NULL, seen INTEGER NOT NULL, idle INTEGER NOT NULL, absolute INTEGER NOT NULL);
                 CREATE TABLE reauth (digest TEXT PRIMARY KEY, session TEXT NOT NULL, operation TEXT NOT NULL,
                     epoch INTEGER NOT NULL, expires INTEGER NOT NULL);
                 CREATE INDEX reauth_session ON reauth(session);"
            ).map_err(storage_error)?;
            tx.execute(
                "INSERT INTO binding VALUES (1, ?1, ?2)",
                params![install, key_digest.0],
            )
            .map_err(storage_error)?;
            tx.commit().map_err(storage_error)?;
        }
        let bound: String = connection
            .query_row("SELECT install FROM binding WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .map_err(|_| AuthError::RecoveryRequired)?;
        if bound != install {
            return Err(AuthError::RecoveryRequired);
        }
        let bound_key: String = connection
            .query_row("SELECT claim_key FROM binding WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .map_err(|_| AuthError::RecoveryRequired)?;
        if bound_key != key_digest.0 {
            return Err(AuthError::RecoveryRequired);
        }
        let integrity: String = connection
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|_| AuthError::RecoveryRequired)?;
        if integrity != "ok" {
            return Err(AuthError::RecoveryRequired);
        }
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            PRAGMA secure_delete=ON; PRAGMA max_page_count=8192;
            PRAGMA trusted_schema=OFF;",
            )
            .map_err(storage_error)?;
        if let Some(owner) = account(&connection)? {
            owner
                .owner
                .validate()
                .map_err(|_| AuthError::RecoveryRequired)?;
            if owner.schema_version != p::SchemaVersion(1)
                || owner.auth_epoch == 0
                || owner.credential_revision == 0
            {
                return Err(AuthError::RecoveryRequired);
            }
        }
        Ok(Self {
            connection,
            install: install.into(),
            _lock: lock,
        })
    }
}

pub(super) fn save_account(connection: &Connection, value: &Account) -> AuthResult<()> {
    sql_epoch(value.auth_epoch)?;
    let json = serde_json::to_string(value).map_err(|_| AuthError::Unavailable)?;
    connection.execute("INSERT INTO account VALUES (1, ?1) ON CONFLICT(singleton) DO UPDATE SET value=excluded.value", [json])
        .map_err(storage_error)?;
    Ok(())
}

/// Recheck the live epoch/session inside each mutation transaction. A previously
/// authenticated Rust value is not a reusable authority after logout or reset.
pub(super) fn check_owner(
    connection: &Connection,
    owner: &super::AuthenticatedWebOwner,
    now: i64,
) -> AuthResult<Account> {
    let current = account(connection)?.ok_or(AuthError::Unauthenticated)?;
    let valid: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE digest=?1 AND csrf=?2 AND epoch=?3
            AND issued<=?4 AND seen<=?4 AND idle>?4 AND absolute>?4)",
            params![
                owner.session_digest,
                owner.csrf_digest,
                sql_epoch(current.auth_epoch)?,
                now
            ],
            |r| r.get(0),
        )
        .map_err(storage_error)?;
    if !valid || owner.view.owner != current.owner || owner.view.auth_epoch != current.auth_epoch {
        return Err(AuthError::Unauthenticated);
    }
    Ok(current)
}

pub(super) fn claim_proof(
    connection: &Connection,
    owner: &super::AuthenticatedWebOwner,
    proof: &super::ReauthenticationProof,
    operation: &p::SchemaDigest,
    now: i64,
) -> AuthResult<()> {
    p::validate_product_digest(operation).map_err(|_| AuthError::InvalidInput)?;
    let token = super::digest(&proof.0 .0)?;
    let valid: Option<String> = connection.query_row(
        "DELETE FROM reauth WHERE digest=?1 AND session=?2 AND operation=?3 AND epoch=?4 AND expires>?5 RETURNING digest",
        params![token, owner.session_digest, operation.0, sql_epoch(owner.view.auth_epoch)?, now], |r| r.get(0)
    ).optional().map_err(storage_error)?;
    valid.map(|_| ()).ok_or(AuthError::Unauthenticated)
}

fn check_path(path: &Path) -> AuthResult<()> {
    if !path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(AuthError::InvalidInput);
    }
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(AuthError::RecoveryRequired);
                    }
                }
                if metadata.file_type().is_symlink() {
                    return Err(AuthError::RecoveryRequired);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(AuthError::Unavailable),
        }
    }
    Ok(())
}

pub(super) fn sql_epoch(value: u64) -> AuthResult<i64> {
    i64::try_from(value)
        .ok()
        .filter(|epoch| *epoch > 0)
        .ok_or(AuthError::RecoveryRequired)
}
