//! Local administrator recovery for the single owner account.
//!
//! This path is intentionally not an HTTP endpoint. The caller must prove the
//! operating-system administrator boundary before invoking it. The transaction
//! preserves the owner and data-key identity, rotates the authentication epoch,
//! revokes every Web session, and leaves a durable fence for the core bridge.

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use super::{
    account, password::validate_password, storage, storage_error, AuthError, AuthResult,
    SensitivePassword, WebIdentityStore,
};
use forme_protocol as p;

const RECOVERY_KIND: &str = "local-administrator-password-reset-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryRecord {
    schema_version: p::SchemaVersion,
    operation: String,
    kind: String,
    owner: p::OwnerPrincipalRef,
    previous_credential_revision: u64,
    next_credential_revision: u64,
    previous_auth_epoch: u64,
    next_auth_epoch: u64,
    password_commitment: p::SchemaDigest,
    created_at: p::Timestamp,
}

impl RecoveryRecord {
    fn validate(&self) -> AuthResult<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.kind != RECOVERY_KIND
            || self.previous_credential_revision == 0
            || self.next_credential_revision
                != self
                    .previous_credential_revision
                    .checked_add(1)
                    .ok_or(AuthError::RecoveryRequired)?
            || self.previous_auth_epoch == 0
            || self.next_auth_epoch
                != self
                    .previous_auth_epoch
                    .checked_add(1)
                    .ok_or(AuthError::RecoveryRequired)?
            || self.created_at <= 0
        {
            return Err(AuthError::RecoveryRequired);
        }
        p::validate_product_id(&self.operation).map_err(|_| AuthError::RecoveryRequired)?;
        self.owner
            .validate()
            .map_err(|_| AuthError::RecoveryRequired)?;
        p::validate_product_digest(&self.password_commitment)
            .map_err(|_| AuthError::RecoveryRequired)
    }
}

impl WebIdentityStore {
    /// Reset the owner login from a caller that has already proved local OS
    /// administrator authority. This method is deliberately not reachable from
    /// the Web router and never changes OwnerId, the brain key, or data paths.
    pub fn recover_login_with_local_authority(
        &self,
        operation: &str,
        password: SensitivePassword,
    ) -> AuthResult<p::AuthRevision> {
        p::validate_product_id(operation).map_err(|_| AuthError::InvalidInput)?;
        validate_password(&password.0)?;
        let _job = self.password_slot()?;
        let replacement = super::PasswordRecord::new(&password.0)?;
        let password_commitment = p::canonical_digest(&(
            p::SchemaVersion(1),
            "forme-local-recovery-password-v1",
            &replacement,
        ))
        .map_err(|_| AuthError::Unavailable)?;
        let mut db = self.database()?;
        let install = db.install.clone();
        let now = super::now();
        let tx = db.connection.transaction().map_err(storage_error)?;

        let existing: Option<String> = tx
            .query_row(
                "SELECT value FROM auth_maintenance WHERE operation=?1",
                [operation],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some(json) = existing {
            let record: RecoveryRecord =
                serde_json::from_str(&json).map_err(|_| AuthError::RecoveryRequired)?;
            record.validate()?;
            let current = account(&tx)?.ok_or(AuthError::RecoveryRequired)?;
            if current.owner != record.owner
                || current.credential_revision != record.next_credential_revision
                || current.auth_epoch != record.next_auth_epoch
                || !current.password.verify(&password.0)?
            {
                return Err(AuthError::Conflict);
            }
            return Ok(p::AuthRevision {
                schema_version: p::SchemaVersion(1),
                credential_revision: record.next_credential_revision,
                auth_epoch: record.next_auth_epoch,
            });
        }

        let mut current = account(&tx)?.ok_or(AuthError::Unauthenticated)?;
        current
            .owner
            .validate()
            .map_err(|_| AuthError::RecoveryRequired)?;
        let next_epoch = current
            .auth_epoch
            .checked_add(1)
            .ok_or(AuthError::RecoveryRequired)?;
        let next_revision = current
            .credential_revision
            .checked_add(1)
            .ok_or(AuthError::RecoveryRequired)?;

        // A recovery has no browser session to use as its source. The stable
        // digest still gives the core fence a valid, auditable source identity
        // while the update remains global because target_session is None.
        let source_session = p::canonical_digest(&(
            p::SchemaVersion(1),
            "forme-local-recovery-source-v1",
            &install,
            operation,
            &current.owner,
            current.auth_epoch,
        ))
        .map_err(|_| AuthError::Unavailable)?;
        storage::enqueue_fence(
            &tx,
            &current.owner,
            &source_session.0,
            current.auth_epoch,
            None,
            next_epoch,
            now,
        )?;
        current.password = replacement;
        current.auth_epoch = next_epoch;
        current.credential_revision = next_revision;
        storage::save_account(&tx, &current)?;
        tx.execute("DELETE FROM sessions", [])
            .map_err(storage_error)?;
        tx.execute("DELETE FROM reauth", [])
            .map_err(storage_error)?;

        let record = RecoveryRecord {
            schema_version: p::SchemaVersion(1),
            operation: operation.to_owned(),
            kind: RECOVERY_KIND.to_owned(),
            owner: current.owner.clone(),
            previous_credential_revision: next_revision - 1,
            next_credential_revision: next_revision,
            previous_auth_epoch: next_epoch - 1,
            next_auth_epoch: next_epoch,
            password_commitment,
            created_at: now,
        };
        record.validate()?;
        let json = serde_json::to_string(&record).map_err(|_| AuthError::Unavailable)?;
        tx.execute(
            "INSERT INTO auth_maintenance(operation,value) VALUES(?1,?2)",
            rusqlite::params![operation, json],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(current.revision())
    }

    /// Returns the owner and revision without exposing password or key data.
    pub fn recovery_status(&self, operation: &str) -> AuthResult<Option<p::AuthRevision>> {
        p::validate_product_id(operation).map_err(|_| AuthError::InvalidInput)?;
        let db = self.database()?;
        let record: Option<String> = db
            .connection
            .query_row(
                "SELECT value FROM auth_maintenance WHERE operation=?1",
                [operation],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        record
            .map(|json| {
                let record: RecoveryRecord =
                    serde_json::from_str(&json).map_err(|_| AuthError::RecoveryRequired)?;
                record.validate()?;
                Ok(p::AuthRevision {
                    schema_version: p::SchemaVersion(1),
                    credential_revision: record.next_credential_revision,
                    auth_epoch: record.next_auth_epoch,
                })
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{SensitiveLoginRequest, SessionCookie, WebIdentityService};
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn store() -> (tempfile::TempDir, WebIdentityStore, SessionCookie) {
        let directory = tempfile::tempdir().unwrap();
        let signing = SigningKey::from_bytes(&[117; 32]);
        let store = WebIdentityStore::open(
            directory.path(),
            "installation:recovery-fixture",
            signing.verifying_key(),
        )
        .unwrap();
        let commitment = store
            .prepare_claim(
                "claim:recovery",
                "fixture",
                SensitivePassword::new("old-password-92".into()),
            )
            .unwrap();
        let receipt = p::WebClaimCommit {
            schema_version: p::SchemaVersion(1),
            install_ref: "installation:recovery-fixture".into(),
            operation_id: "claim:recovery".into(),
            candidate_ref: "data:recovery-fixture".into(),
            owner: p::OwnerPrincipalRef("owner:recovery-fixture".into()),
            account_commitment: commitment,
            expires_at: super::super::now() + 60_000,
        };
        let digest = p::canonical_digest(&("forme-web-claim-commit-v1", &receipt)).unwrap();
        let grant = p::SignedWebClaimCommit {
            schema_version: p::SchemaVersion(1),
            receipt,
            signature: signing.sign(digest.0.as_bytes()).to_bytes().to_vec(),
        };
        store.commit_claim(&grant).unwrap();
        let session = store
            .login_at(
                SensitiveLoginRequest {
                    login_name: "fixture".into(),
                    password: SensitivePassword::new("old-password-92".into()),
                },
                super::super::now(),
            )
            .unwrap();
        let _ = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        (directory, store, session)
    }

    #[test]
    fn local_recovery_rotates_epoch_and_is_idempotent() {
        let (_directory, store, session) = store();

        let first = store
            .recover_login_with_local_authority(
                "recovery:fixture",
                SensitivePassword::new("new-password".into()),
            )
            .unwrap();
        assert_eq!(first.credential_revision, 2);
        assert_eq!(first.auth_epoch, 2);
        assert!(store
            .authenticate(
                &session.cookie,
                super::super::WebRequestBinding {
                    mutation: false,
                    csrf: None,
                },
            )
            .is_err());

        let retry = store
            .recover_login_with_local_authority(
                "recovery:fixture",
                SensitivePassword::new("new-password".into()),
            )
            .unwrap();
        assert_eq!(retry, first);
        assert!(store
            .recover_login_with_local_authority(
                "recovery:fixture",
                SensitivePassword::new("another-password".into()),
            )
            .is_err());
    }
}
