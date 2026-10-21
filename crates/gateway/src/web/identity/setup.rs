//! Finite browser setup scope, distinct from owner login and all brain access.
use rusqlite::{params, OptionalExtension};

use super::*;

const SETUP_TTL: i64 = 15 * 60 * 1000;

pub(crate) struct SetupCookie {
    pub cookie: SensitiveCookie,
    pub csrf: SensitiveCookie,
    pub binding: p::BootstrapSessionBinding,
}

pub(crate) struct SetupAccess {
    pub binding: p::BootstrapSessionBinding,
    pub candidate: Option<String>,
}

impl WebIdentityStore {
    /// Persist the cookie digest before requesting the broker reservation. A
    /// lost cookie cannot be reconstructed or replaced by a second browser.
    pub(crate) fn prepare_setup(
        &self,
        operation: &str,
        existing: Option<SensitiveCookie>,
    ) -> AuthResult<SetupCookie> {
        p::validate_product_id(operation).map_err(|_| AuthError::InvalidInput)?;
        let cookie = match existing {
            Some(cookie) => cookie,
            None => SensitiveCookie::new(random_token()?),
        };
        let digest = digest(cookie.expose_for_transport())?;
        let csrf = csrf_token(&cookie)?;
        let now = now();
        let mut db = self.database()?;
        let tx = db.connection.transaction().map_err(storage_error)?;
        if account(&tx)?.is_some() {
            return Err(AuthError::Conflict);
        }
        let row: Option<(String, i64)> = tx
            .query_row(
                "SELECT digest,expires FROM setup_sessions WHERE operation=?1",
                [operation],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some((expected, expires)) = row {
            if expected != digest {
                return Err(AuthError::Conflict);
            }
            if now >= expires {
                return Err(AuthError::Expired);
            }
        } else {
            tx.execute(
                "DELETE FROM setup_sessions WHERE active=0 AND expires<=?1",
                [now],
            )
            .map_err(storage_error)?;
            let count: i64 = tx
                .query_row("SELECT count(*) FROM setup_sessions", [], |r| r.get(0))
                .map_err(storage_error)?;
            if count >= 16 {
                return Err(AuthError::Busy);
            }
            let expires = now.checked_add(SETUP_TTL).ok_or(AuthError::Unavailable)?;
            tx.execute(
                "INSERT INTO setup_sessions VALUES (?1,?2,?3,0,NULL)",
                params![operation, digest, expires],
            )
            .map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)?;
        Ok(SetupCookie {
            cookie,
            csrf,
            binding: p::BootstrapSessionBinding {
                schema_version: p::SchemaVersion(1),
                operation_id: operation.into(),
                setup_digest: p::SchemaDigest(digest),
            },
        })
    }

    pub(crate) fn activate_setup(
        &self,
        setup: &SetupCookie,
        reserved: &p::BootstrapReservationView,
    ) -> AuthResult<()> {
        let now = now();
        if reserved.schema_version != p::SchemaVersion(1)
            || reserved.operation_id != setup.binding.operation_id
            || reserved.expires_at <= now
            || reserved.expires_at > now.saturating_add(SETUP_TTL)
        {
            return Err(AuthError::Conflict);
        }
        let mut db = self.database()?;
        let tx = db.connection.transaction().map_err(storage_error)?;
        if account(&tx)?.is_some() {
            return Err(AuthError::Conflict);
        }
        let changed = tx
            .execute(
                "UPDATE setup_sessions SET active=1,expires=?1 WHERE operation=?2 AND digest=?3",
                params![
                    reserved.expires_at,
                    reserved.operation_id,
                    setup.binding.setup_digest.0
                ],
            )
            .map_err(storage_error)?;
        if changed != 1 {
            return Err(AuthError::Conflict);
        }
        // A newly broker-confirmed reservation proves the old uncommitted
        // setup was locally rotated. Never discard prepared state beforehand.
        let prepared: Option<String> = tx
            .query_row("SELECT value FROM prepared WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage_error)?;
        if let Some(json) = prepared {
            let value: serde_json::Value =
                serde_json::from_str(&json).map_err(|_| AuthError::RecoveryRequired)?;
            if value
                .get("operation_id")
                .and_then(serde_json::Value::as_str)
                != Some(reserved.operation_id.as_str())
            {
                tx.execute("DELETE FROM prepared", [])
                    .map_err(storage_error)?;
            }
        }
        tx.execute(
            "DELETE FROM setup_sessions WHERE operation<>?1",
            [&reserved.operation_id],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(())
    }

    /// Remove only a locally prepared, not-yet-activated setup after the
    /// broker gave a definitive rejection. Unknown transport outcomes retain
    /// the row because the broker may already have durably reserved it.
    pub(crate) fn discard_unactivated_setup(&self, setup: &SetupCookie) -> AuthResult<()> {
        let db = self.database()?;
        db.connection
            .execute(
                "DELETE FROM setup_sessions WHERE operation=?1 AND digest=?2 AND active=0",
                params![setup.binding.operation_id, setup.binding.setup_digest.0],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn setup_access(
        &self,
        cookie: &SensitiveCookie,
        request: WebRequestBinding,
        allow_claimed: bool,
    ) -> AuthResult<SetupAccess> {
        if request.mutation {
            let csrf = request.csrf.ok_or(AuthError::CsrfRejected)?;
            if !equal_bytes(csrf.0.as_bytes(), csrf_token(cookie)?.0.as_bytes()) {
                return Err(AuthError::CsrfRejected);
            }
        }
        let digest = digest(cookie.expose_for_transport())?;
        let db = self.database()?;
        let (operation,expires,candidate): (String,i64,Option<String>) = db.connection.query_row(
            "SELECT operation,expires,candidate FROM setup_sessions WHERE digest=?1 AND active=1", [&digest],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(storage_error)?.ok_or(AuthError::Unauthenticated)?;
        let now = now();
        if now >= expires || expires > now.saturating_add(SETUP_TTL) {
            return Err(AuthError::Expired);
        }
        if let Some(current) = account(&db.connection)? {
            if !allow_claimed
                || current.claim_operation != operation
                || candidate.as_ref() != Some(&current.claim_candidate)
            {
                return Err(AuthError::Conflict);
            }
        }
        Ok(SetupAccess {
            binding: p::BootstrapSessionBinding {
                schema_version: p::SchemaVersion(1),
                operation_id: operation,
                setup_digest: p::SchemaDigest(digest),
            },
            candidate,
        })
    }

    pub(crate) fn record_setup_candidate(
        &self,
        setup: &SetupAccess,
        candidate: &p::BootstrapDataCandidateView,
    ) -> AuthResult<()> {
        if candidate.schema_version != p::SchemaVersion(1)
            || !candidate.selected
            || !candidate.available
        {
            return Err(AuthError::Conflict);
        }
        p::validate_product_id(&candidate.candidate_ref).map_err(|_| AuthError::InvalidInput)?;
        let db = self.database()?;
        if account(&db.connection)?.is_some() {
            return Err(AuthError::Conflict);
        }
        let changed = db.connection.execute("UPDATE setup_sessions SET candidate=?1 WHERE operation=?2 AND digest=?3 AND active=1 AND expires>?4 AND (candidate IS NULL OR candidate=?1)",
            params![candidate.candidate_ref,setup.binding.operation_id,setup.binding.setup_digest.0,now()]).map_err(storage_error)?;
        if changed != 1 {
            return Err(AuthError::Conflict);
        }
        Ok(())
    }

    pub(crate) fn pending_bootstrap(&self) -> AuthResult<Option<p::BootstrapCommitRequest>> {
        let row = {
            let db = self.database()?;
            if account(&db.connection)?.is_some() {
                return Ok(None);
            }
            db.connection.query_row("SELECT operation,digest,candidate FROM setup_sessions WHERE active=1 AND candidate IS NOT NULL", [],
                |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional().map_err(storage_error)?
        };
        let Some((operation, digest, candidate)) = row else {
            return Ok(None);
        };
        let Some(commitment) = self.pending_account_commitment(&operation)? else {
            return Ok(None);
        };
        Ok(Some(p::BootstrapCommitRequest {
            schema_version: p::SchemaVersion(1),
            session: p::BootstrapSessionBinding {
                schema_version: p::SchemaVersion(1),
                operation_id: operation,
                setup_digest: p::SchemaDigest(digest),
            },
            candidate_ref: candidate,
            account_commitment: commitment,
        }))
    }
}
