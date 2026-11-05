//! A schema-2 account requires the pinned activation signature and original hash.
use super::*;
use ed25519_dalek::Signature;
use rusqlite::{params, OptionalExtension};

impl WebIdentityStore {
    pub fn commit_claim_activation(
        &self,
        grant: &p::SignedBootstrapClaimActivationV2,
    ) -> AuthResult<p::AuthRevision> {
        grant.validate().map_err(|_| AuthError::InvalidInput)?;
        let signature =
            Signature::from_slice(&grant.signature).map_err(|_| AuthError::Unauthenticated)?;
        let message =
            p::SignedBootstrapClaimActivationV2::signing_digest(&grant.receipt, &grant.activation)
                .map_err(|_| AuthError::InvalidInput)?;
        self.claim_key
            .verify_strict(message.0.as_bytes(), &signature)
            .map_err(|_| AuthError::Unauthenticated)?;
        let mut db = self.database()?;
        let receipt = &grant.receipt;
        if receipt.install_ref != db.install {
            return Err(AuthError::Unauthenticated);
        }
        let json = serde_json::to_string(grant).map_err(|_| AuthError::Unavailable)?;
        if let Some(existing) = account(&db.connection)? {
            let original: Option<String> = db
                .connection
                .query_row(
                    "SELECT value FROM activated_claims_v2 WHERE operation=?1",
                    [&receipt.operation_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            return if existing.claim_operation == receipt.operation_id
                && existing.claim_commitment == receipt.account_commitment
                && existing.claim_candidate == receipt.candidate_ref
                && existing.owner == receipt.owner_ref
                && original.as_deref() == Some(json.as_str())
            {
                Ok(existing.revision())
            } else {
                Err(AuthError::Conflict)
            };
        }
        let tx = db.connection.transaction().map_err(storage_error)?;
        let candidate = super::setup_legacy::selected(&tx, &receipt.operation_id)?
            .ok_or(AuthError::Conflict)?;
        candidate
            .validate(&receipt.install_ref)
            .map_err(|_| AuthError::RecoveryRequired)?;
        if candidate.candidate_ref != receipt.candidate_ref
            || candidate.candidate_digest != receipt.candidate_digest
            || candidate.kind != receipt.candidate_kind
            || candidate.owner_source != receipt.owner_source
            || candidate.owner_ref.as_ref() != Some(&receipt.owner_ref)
            || candidate.owner_binding_digest.as_ref() != Some(&receipt.owner_binding_digest)
        {
            return Err(AuthError::Conflict);
        }
        let prepared_json: String = tx
            .query_row("SELECT value FROM prepared WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage_error)?
            .ok_or(AuthError::Conflict)?;
        let prepared: super::account::PreparedAccount =
            serde_json::from_str(&prepared_json).map_err(|_| AuthError::RecoveryRequired)?;
        let expected = p::canonical_digest(&(
            p::SchemaVersion(1),
            &receipt.install_ref,
            &receipt.operation_id,
            &prepared.login_name,
            &prepared.password,
        ))
        .map_err(|_| AuthError::Unavailable)?;
        if prepared.schema_version != p::SchemaVersion(1)
            || prepared.operation_id != receipt.operation_id
            || prepared.commitment != receipt.account_commitment
            || expected != receipt.account_commitment
        {
            return Err(AuthError::Conflict);
        }
        let value = storage::Account {
            schema_version: p::SchemaVersion(1),
            account_ref: format!("web-account:{}", receipt.operation_id),
            owner: receipt.owner_ref.clone(),
            login_name: prepared.login_name,
            password: prepared.password,
            credential_revision: 1,
            auth_epoch: 1,
            claim_operation: receipt.operation_id.clone(),
            claim_candidate: receipt.candidate_ref.clone(),
            claim_commitment: receipt.account_commitment.clone(),
            created_at: now(),
        };
        let changed = tx.execute("UPDATE setup_sessions SET active=0 WHERE operation=?1 AND active=1 AND candidate=?2", params![receipt.operation_id, receipt.candidate_ref]).map_err(storage_error)?;
        if changed != 1 {
            return Err(AuthError::Conflict);
        }
        storage::save_account(&tx, &value)?;
        tx.execute(
            "INSERT INTO activated_claims_v2 VALUES (?1,?2)",
            params![receipt.operation_id, json],
        )
        .map_err(storage_error)?;
        tx.execute("DELETE FROM prepared", [])
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(value.revision())
    }
}
