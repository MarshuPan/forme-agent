//! Broker-bound account creation; no browser-supplied owner or key is trusted.
use ed25519_dalek::Signature;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use super::{
    account, password::validate_password, storage, storage_error, AuthError, AuthResult,
    PasswordRecord, SensitivePassword, WebIdentityStore,
};
use forme_protocol as p;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedAccount {
    schema_version: p::SchemaVersion,
    operation_id: String,
    login_name: String,
    password: PasswordRecord,
    commitment: p::SchemaDigest,
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
        if account(&db.connection)?.is_some() {
            return Err(AuthError::Conflict);
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
        password: SensitivePassword,
    ) -> AuthResult<super::PreparedPasswordChange> {
        self.prepare_password_change_with_time(owner, password, None)
    }

    #[cfg(test)]
    pub(super) fn prepare_password_change_at(
        &self,
        owner: &super::AuthenticatedWebOwner,
        password: SensitivePassword,
        now: p::Timestamp,
    ) -> AuthResult<super::PreparedPasswordChange> {
        self.prepare_password_change_with_time(owner, password, Some(now))
    }

    fn prepare_password_change_with_time(
        &self,
        owner: &super::AuthenticatedWebOwner,
        password: SensitivePassword,
        supplied_time: Option<p::Timestamp>,
    ) -> AuthResult<super::PreparedPasswordChange> {
        let _job = self.password_slot()?;
        let current = {
            let db = self.database()?;
            storage::check_owner(
                &db.connection,
                owner,
                supplied_time.unwrap_or_else(super::now),
            )?
        };
        let replacement = PasswordRecord::new(&password.0)?;
        let operation = p::canonical_digest(&(
            p::SchemaVersion(1),
            "web-password-change-v1",
            &current.owner,
            &current.account_ref,
            current.credential_revision,
            current.auth_epoch,
            &replacement,
        ))
        .map_err(|_| AuthError::Unavailable)?;
        Ok(super::PreparedPasswordChange {
            password: replacement,
            operation,
            account_ref: current.account_ref,
            credential_revision: current.credential_revision,
            auth_epoch: current.auth_epoch,
        })
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
        storage::claim_proof(&tx, owner, &command.proof, &command.operation_digest, now)?;
        current.password = prepared.password;
        current.auth_epoch = current
            .auth_epoch
            .checked_add(1)
            .ok_or(AuthError::RecoveryRequired)?;
        current.credential_revision = current
            .credential_revision
            .checked_add(1)
            .ok_or(AuthError::RecoveryRequired)?;
        storage::save_account(&tx, &current)?;
        tx.execute("DELETE FROM sessions", [])
            .map_err(storage_error)?;
        tx.execute("DELETE FROM reauth", [])
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(current.revision())
    }
}
