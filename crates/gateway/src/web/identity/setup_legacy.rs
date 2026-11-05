//! Persist the exact verified candidate alongside the original setup operation.
use super::*;
use rusqlite::{params, OptionalExtension};

pub(super) fn selected(
    db: &Connection,
    operation: &str,
) -> AuthResult<Option<p::BootstrapCandidateViewV2>> {
    let json: Option<String> = db
        .query_row(
            "SELECT value FROM setup_candidates_v2 WHERE operation=?1",
            [operation],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    json.map(|json| serde_json::from_str(&json).map_err(|_| AuthError::RecoveryRequired))
        .transpose()
}

pub(super) fn binding_v2(binding: &p::BootstrapSessionBinding) -> p::BootstrapSessionBindingV2 {
    p::BootstrapSessionBindingV2 {
        schema_version: p::SchemaVersion(2),
        operation_id: binding.operation_id.clone(),
        setup_digest: binding.setup_digest.clone(),
    }
}

impl WebIdentityStore {
    pub(crate) fn record_setup_candidate_v2(
        &self,
        setup: &setup::SetupAccess,
        candidate: &p::BootstrapCandidateViewV2,
    ) -> AuthResult<()> {
        let mut db = self.database()?;
        candidate
            .validate(&db.install)
            .map_err(|_| AuthError::InvalidInput)?;
        if candidate.status != p::BootstrapCandidateStatus::Available
            || candidate.kind == p::BootstrapCandidateKind::NewBrainTarget
        {
            return Err(AuthError::Conflict);
        }
        let tx = db.connection.transaction().map_err(storage_error)?;
        if account(&tx)?.is_some() {
            return Err(AuthError::Conflict);
        }
        if let Some(original) = selected(&tx, &setup.binding.operation_id)? {
            if original != *candidate {
                return Err(AuthError::Conflict);
            }
            return Ok(());
        }
        let count = tx.execute("UPDATE setup_sessions SET candidate=?1 WHERE operation=?2 AND digest=?3 AND active=1 AND expires>?4 AND (candidate IS NULL OR candidate=?1)",
            params![candidate.candidate_ref, setup.binding.operation_id, setup.binding.setup_digest.0, now()]).map_err(storage_error)?;
        if count != 1 {
            return Err(AuthError::Conflict);
        }
        tx.execute(
            "INSERT INTO setup_candidates_v2 VALUES (?1,?2)",
            params![
                setup.binding.operation_id,
                serde_json::to_string(candidate).map_err(|_| AuthError::Unavailable)?
            ],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }

    pub(super) fn pending_legacy_bootstrap(
        &self,
    ) -> AuthResult<Option<p::BootstrapClaimRequestV2>> {
        let row = {
            let db = self.database()?;
            if account(&db.connection)?.is_some() {
                return Ok(None);
            }
            db.connection.query_row("SELECT s.operation,s.digest,c.value FROM setup_sessions s JOIN setup_candidates_v2 c ON s.operation=c.operation WHERE s.active=1", [], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?))).optional().map_err(storage_error)?
        };
        let Some((operation, digest, json)) = row else {
            return Ok(None);
        };
        let Some(commitment) = self.pending_account_commitment(&operation)? else {
            return Ok(None);
        };
        let candidate: p::BootstrapCandidateViewV2 =
            serde_json::from_str(&json).map_err(|_| AuthError::RecoveryRequired)?;
        Ok(Some(p::BootstrapClaimRequestV2 {
            schema_version: p::SchemaVersion(2),
            session: p::BootstrapSessionBindingV2 {
                schema_version: p::SchemaVersion(2),
                operation_id: operation,
                setup_digest: p::SchemaDigest(digest),
            },
            candidate_ref: candidate.candidate_ref,
            expected_candidate_digest: candidate.candidate_digest,
            account_commitment: commitment,
        }))
    }
}
