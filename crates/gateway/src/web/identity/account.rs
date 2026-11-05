//! Broker-bound account creation; no browser-supplied owner or key is trusted.
use ed25519_dalek::Signature;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use super::{
    account, password::validate_password, storage, storage_error, AuthError, AuthResult,
    PasswordRecord, SensitivePassword, WebIdentityStore,
};
use forme_protocol as p;

pub(super) const PASSWORD_CANDIDATE_TTL: i64 = 24 * 60 * 60 * 1000;
const MAX_PENDING_PASSWORD_CANDIDATES: i64 = 64;

struct PasswordChangeCommandRecord {
    owner: String,
    session: String,
    account_ref: String,
    credential_revision: i64,
    auth_epoch: i64,
    candidate: Option<String>,
    operation_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreparedAccount {
    pub(super) schema_version: p::SchemaVersion,
    pub(super) operation_id: String,
    pub(super) login_name: String,
    pub(super) password: PasswordRecord,
    pub(super) commitment: p::SchemaDigest,
}

impl WebIdentityStore {
    /// Internal setup adapter only: prepare after broker reserves the setup
    /// operation. This is not an anonymous HTTP registration endpoint.
    pub fn prepare_claim(
        &self,
        operation: &str,
        login: &str,
        password: SensitivePassword,
    ) -> AuthResult<p::SchemaDigest> {
        p::validate_product_id(operation).map_err(|_| AuthError::InvalidInput)?;
        if !(3..=64).contains(&login.len())
            || !login
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(AuthError::InvalidInput);
        }
        validate_password(&password.0)?;
        let _job = self.password_slot()?;
        let mut db = self.database()?;
        if let Some(current) = account(&db.connection)? {
            return if current.claim_operation == operation
                && current.login_name == login
                && current.password.verify(&password.0)?
            {
                Ok(current.claim_commitment)
            } else {
                Err(AuthError::Conflict)
            };
        }
        let existing: Option<String> = db
            .connection
            .query_row("SELECT value FROM prepared WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage_error)?;
        if let Some(existing) = existing {
            let existing: PreparedAccount =
                serde_json::from_str(&existing).map_err(|_| AuthError::RecoveryRequired)?;
            return if existing.operation_id == operation
                && existing.login_name == login
                && existing.password.verify(&password.0)?
            {
                Ok(existing.commitment)
            } else {
                Err(AuthError::Conflict)
            };
        }
        let hash = PasswordRecord::new(&password.0)?;
        let commitment =
            p::canonical_digest(&(p::SchemaVersion(1), &db.install, operation, login, &hash))
                .map_err(|_| AuthError::Unavailable)?;
        let prepared = PreparedAccount {
            schema_version: p::SchemaVersion(1),
            operation_id: operation.into(),
            login_name: login.into(),
            password: hash,
            commitment: commitment.clone(),
        };
        let json = serde_json::to_string(&prepared).map_err(|_| AuthError::Unavailable)?;
        let tx = db.connection.transaction().map_err(storage_error)?;
        tx.execute("INSERT INTO prepared VALUES (1, ?1)", [json])
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(commitment)
    }

    /// Only a pinned broker signature over a committed verified-data claim can
    /// create the account. Retries return the same revision, never another owner.
    pub fn commit_claim(&self, grant: &p::SignedWebClaimCommit) -> AuthResult<p::AuthRevision> {
        self.commit_claim_with_time(grant, None)
    }

    pub(super) fn pending_account_commitment(
        &self,
        operation: &str,
    ) -> AuthResult<Option<p::SchemaDigest>> {
        let db = self.database()?;
        let json: Option<String> = db
            .connection
            .query_row("SELECT value FROM prepared WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage_error)?;
        let Some(json) = json else {
            return Ok(None);
        };
        let prepared: PreparedAccount =
            serde_json::from_str(&json).map_err(|_| AuthError::RecoveryRequired)?;
        if prepared.schema_version != p::SchemaVersion(1) {
            return Err(AuthError::RecoveryRequired);
        }
        Ok((prepared.operation_id == operation).then_some(prepared.commitment))
    }

    #[cfg(test)]
    pub(super) fn commit_claim_at(
        &self,
        grant: &p::SignedWebClaimCommit,
        now: p::Timestamp,
    ) -> AuthResult<p::AuthRevision> {
        self.commit_claim_with_time(grant, Some(now))
    }

    fn commit_claim_with_time(
        &self,
        grant: &p::SignedWebClaimCommit,
        supplied_time: Option<p::Timestamp>,
    ) -> AuthResult<p::AuthRevision> {
        if grant.schema_version != p::SchemaVersion(1) || grant.receipt.validate().is_err() {
            return Err(AuthError::InvalidInput);
        }
        let signature =
            Signature::from_slice(&grant.signature).map_err(|_| AuthError::Unauthenticated)?;
        let message = p::canonical_digest(&("forme-web-claim-commit-v1", &grant.receipt))
            .map_err(|_| AuthError::Unavailable)?;
        self.claim_key
            .verify_strict(message.0.as_bytes(), &signature)
            .map_err(|_| AuthError::Unauthenticated)?;
        let mut db = self.database()?;
        let now = supplied_time.unwrap_or_else(super::now);
        let receipt = &grant.receipt;
        if receipt.install_ref != db.install {
            return Err(AuthError::Unauthenticated);
        }
        if let Some(existing) = account(&db.connection)? {
            return if existing.claim_operation == receipt.operation_id
                && existing.claim_commitment == receipt.account_commitment
                && existing.claim_candidate == receipt.candidate_ref
                && existing.owner == receipt.owner
            {
                Ok(existing.revision())
            } else {
                Err(AuthError::Conflict)
            };
        }
        if now < 0
            || now >= receipt.expires_at
            || receipt.expires_at.saturating_sub(now) > 30 * 60 * 1000
        {
            return Err(AuthError::Expired);
        }
        let tx = db.connection.transaction().map_err(storage_error)?;
        let json: String = tx
            .query_row("SELECT value FROM prepared WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage_error)?
            .ok_or(AuthError::Conflict)?;
        let prepared: PreparedAccount =
            serde_json::from_str(&json).map_err(|_| AuthError::RecoveryRequired)?;
        if prepared.schema_version != p::SchemaVersion(1)
            || prepared.operation_id != receipt.operation_id
            || prepared.commitment != receipt.account_commitment
        {
            return Err(AuthError::Conflict);
        }
        let value = storage::Account {
            schema_version: p::SchemaVersion(1),
            account_ref: format!("web-account:{}", receipt.operation_id),
            owner: receipt.owner.clone(),
            login_name: prepared.login_name,
            password: prepared.password,
            credential_revision: 1,
            auth_epoch: 1,
            claim_operation: receipt.operation_id.clone(),
            claim_candidate: receipt.candidate_ref.clone(),
            claim_commitment: receipt.account_commitment.clone(),
            created_at: now,
        };
        storage::save_account(&tx, &value)?;
        tx.execute("DELETE FROM prepared", [])
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(value.revision())
    }

    pub fn prepare_password_change(
        &self,
        owner: &super::AuthenticatedWebOwner,
        operation_id: &str,
        password: SensitivePassword,
    ) -> AuthResult<super::PreparedPasswordChange> {
        self.prepare_password_change_with_time(owner, operation_id, password, None)
    }

    #[cfg(test)]
    pub(super) fn prepare_password_change_at(
        &self,
        owner: &super::AuthenticatedWebOwner,
        operation_id: &str,
        password: SensitivePassword,
        now: p::Timestamp,
    ) -> AuthResult<super::PreparedPasswordChange> {
        self.prepare_password_change_with_time(owner, operation_id, password, Some(now))
    }

    fn prepare_password_change_with_time(
        &self,
        owner: &super::AuthenticatedWebOwner,
        operation_id: &str,
        password: SensitivePassword,
        supplied_time: Option<p::Timestamp>,
    ) -> AuthResult<super::PreparedPasswordChange> {
        p::validate_product_id(operation_id).map_err(|_| AuthError::InvalidInput)?;
        let _job = self.password_slot()?;
        let now = supplied_time.unwrap_or_else(super::now);
        let (current, existing) = self.load_password_change_snapshot(owner, operation_id, now)?;
        if let Some(record) = existing {
            return prepared_password_change_from_record(
                owner,
                &current,
                operation_id,
                password,
                record,
            );
        }

        let replacement = PasswordRecord::new(&password.0)?;
        let operation = password_change_digest(operation_id, &current, &replacement)?;
        self.persist_password_change_candidate(
            owner,
            operation_id,
            &current,
            &replacement,
            &operation,
            now,
        )?;
        Ok(super::PreparedPasswordChange {
            operation_id: operation_id.into(),
            password: replacement,
            operation,
            account_ref: current.account_ref,
            credential_revision: current.credential_revision,
            auth_epoch: current.auth_epoch,
        })
    }

    fn load_password_change_snapshot(
        &self,
        owner: &super::AuthenticatedWebOwner,
        operation_id: &str,
        now: p::Timestamp,
    ) -> AuthResult<(storage::Account, Option<PasswordChangeCommandRecord>)> {
        let mut db = self.database()?;
        let tx = db.connection.transaction().map_err(storage_error)?;
        let current = storage::check_owner(&tx, owner, now)?;
        tx.execute(
            "UPDATE password_change_commands SET candidate=NULL
             WHERE candidate IS NOT NULL AND created<=?1",
            [now.saturating_sub(PASSWORD_CANDIDATE_TTL)],
        )
        .map_err(storage_error)?;
        let existing = password_change_command(&tx, operation_id)?;
        tx.commit().map_err(storage_error)?;
        Ok((current, existing))
    }

    fn persist_password_change_candidate(
        &self,
        owner: &super::AuthenticatedWebOwner,
        operation_id: &str,
        current: &storage::Account,
        replacement: &PasswordRecord,
        operation: &p::SchemaDigest,
        now: p::Timestamp,
    ) -> AuthResult<()> {
        let candidate = serde_json::to_string(replacement).map_err(|_| AuthError::Unavailable)?;
        let mut db = self.database()?;
        let tx = db.connection.transaction().map_err(storage_error)?;
        let verified = storage::check_owner(&tx, owner, now)?;
        if verified.account_ref != current.account_ref
            || verified.credential_revision != current.credential_revision
            || verified.auth_epoch != current.auth_epoch
        {
            return Err(AuthError::Conflict);
        }
        let pending: i64 = tx
            .query_row(
                "SELECT count(*) FROM password_change_commands WHERE candidate IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if pending >= MAX_PENDING_PASSWORD_CANDIDATES {
            return Err(AuthError::Busy);
        }
        tx.execute(
            "INSERT INTO password_change_commands
             (operation,owner,session,account_ref,credential_revision,auth_epoch,
              candidate,operation_digest,created)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                operation_id,
                p::sha256_content_digest(current.owner.0.as_bytes()).0,
                owner.session_digest,
                current.account_ref,
                storage::sql_epoch(current.credential_revision)?,
                storage::sql_epoch(current.auth_epoch)?,
                candidate,
                operation.0,
                now,
            ],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }

    fn validate_password_change_record(
        &self,
        tx: &rusqlite::Transaction<'_>,
        owner: &super::AuthenticatedWebOwner,
        current: &storage::Account,
        prepared: &super::PreparedPasswordChange,
    ) -> AuthResult<String> {
        let record = password_change_command(tx, &prepared.operation_id)?
            .ok_or(AuthError::RecoveryRequired)?;
        let owner_digest = validate_password_change_binding(owner, current, &record)?;
        let expected_candidate =
            serde_json::to_string(&prepared.password).map_err(|_| AuthError::Unavailable)?;
        if record.candidate.as_deref() != Some(expected_candidate.as_str())
            || record.operation_digest != prepared.operation.0
        {
            return Err(AuthError::RecoveryRequired);
        }
        Ok(owner_digest)
    }

    /// The exact prepared hash, operation proof, epoch change and all-session
    /// revocation commit together. No caller can replace the password after reauth.
    pub fn change_password(
        &self,
        owner: &super::AuthenticatedWebOwner,
        command: super::RevokeSessions,
        prepared: super::PreparedPasswordChange,
    ) -> AuthResult<p::AuthRevision> {
        self.change_password_with_time(owner, command, prepared, None)
    }

    #[cfg(test)]
    pub(super) fn change_password_at(
        &self,
        owner: &super::AuthenticatedWebOwner,
        command: super::RevokeSessions,
        prepared: super::PreparedPasswordChange,
        now: p::Timestamp,
    ) -> AuthResult<p::AuthRevision> {
        self.change_password_with_time(owner, command, prepared, Some(now))
    }

    fn change_password_with_time(
        &self,
        owner: &super::AuthenticatedWebOwner,
        command: super::RevokeSessions,
        prepared: super::PreparedPasswordChange,
        supplied_time: Option<p::Timestamp>,
    ) -> AuthResult<p::AuthRevision> {
        if command.operation_digest != prepared.operation {
            return Err(AuthError::Conflict);
        }
        let mut db = self.database()?;
        let now = supplied_time.unwrap_or_else(super::now);
        let tx = db.connection.transaction().map_err(storage_error)?;
        let mut current = storage::check_owner(&tx, owner, now)?;
        if current.account_ref != prepared.account_ref
            || current.credential_revision != prepared.credential_revision
            || current.auth_epoch != prepared.auth_epoch
        {
            return Err(AuthError::Conflict);
        }
        let owner_digest = self.validate_password_change_record(&tx, owner, &current, &prepared)?;
        storage::claim_proof(&tx, owner, &command.proof, &command.operation_digest, now)?;
        let next_epoch = current
            .auth_epoch
            .checked_add(1)
            .ok_or(AuthError::RecoveryRequired)?;
        storage::enqueue_fence(
            &tx,
            &current.owner,
            &owner.session_digest,
            current.auth_epoch,
            None,
            next_epoch,
            now,
        )?;
        current.password = prepared.password;
        current.auth_epoch = next_epoch;
        current.credential_revision = current
            .credential_revision
            .checked_add(1)
            .ok_or(AuthError::RecoveryRequired)?;
        storage::save_account(&tx, &current)?;
        tx.execute("DELETE FROM sessions", [])
            .map_err(storage_error)?;
        tx.execute("DELETE FROM reauth", [])
            .map_err(storage_error)?;
        tx.execute(
            "UPDATE password_change_commands SET candidate=NULL WHERE owner=?1",
            [&owner_digest],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(current.revision())
    }
}

fn password_change_command(
    connection: &rusqlite::Connection,
    operation_id: &str,
) -> AuthResult<Option<PasswordChangeCommandRecord>> {
    connection
        .query_row(
            "SELECT owner,session,account_ref,credential_revision,auth_epoch,
                    candidate,operation_digest
             FROM password_change_commands WHERE operation=?1",
            [operation_id],
            |row| {
                Ok(PasswordChangeCommandRecord {
                    owner: row.get(0)?,
                    session: row.get(1)?,
                    account_ref: row.get(2)?,
                    credential_revision: row.get(3)?,
                    auth_epoch: row.get(4)?,
                    candidate: row.get(5)?,
                    operation_digest: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(storage_error)
}

fn validate_password_change_binding(
    owner: &super::AuthenticatedWebOwner,
    current: &storage::Account,
    record: &PasswordChangeCommandRecord,
) -> AuthResult<String> {
    let owner_digest = p::sha256_content_digest(current.owner.0.as_bytes()).0;
    if record.owner != owner_digest
        || record.session != owner.session_digest
        || record.account_ref != current.account_ref
        || record.credential_revision != storage::sql_epoch(current.credential_revision)?
        || record.auth_epoch != storage::sql_epoch(current.auth_epoch)?
    {
        return Err(AuthError::Conflict);
    }
    Ok(owner_digest)
}

fn password_change_digest(
    operation_id: &str,
    current: &storage::Account,
    replacement: &PasswordRecord,
) -> AuthResult<p::SchemaDigest> {
    p::canonical_digest(&(
        p::SchemaVersion(1),
        "web-password-change-v1",
        operation_id,
        &current.owner,
        &current.account_ref,
        current.credential_revision,
        current.auth_epoch,
        replacement,
    ))
    .map_err(|_| AuthError::Unavailable)
}

fn prepared_password_change_from_record(
    owner: &super::AuthenticatedWebOwner,
    current: &storage::Account,
    operation_id: &str,
    password: SensitivePassword,
    record: PasswordChangeCommandRecord,
) -> AuthResult<super::PreparedPasswordChange> {
    validate_password_change_binding(owner, current, &record)?;
    let candidate = record
        .candidate
        .ok_or(AuthError::Conflict)
        .and_then(|value| {
            serde_json::from_str::<PasswordRecord>(&value).map_err(|_| AuthError::RecoveryRequired)
        })?;
    if !candidate.verify(&password.0)? {
        return Err(AuthError::Conflict);
    }
    let operation = password_change_digest(operation_id, current, &candidate)?;
    if operation.0 != record.operation_digest {
        return Err(AuthError::RecoveryRequired);
    }
    Ok(super::PreparedPasswordChange {
        operation_id: operation_id.into(),
        password: candidate,
        operation,
        account_ref: current.account_ref.clone(),
        credential_revision: current.credential_revision,
        auth_epoch: current.auth_epoch,
    })
}
