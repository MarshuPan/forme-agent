//! Expiry, credential CAS and operation-bound one-shot reauthentication.
use rusqlite::{params, OptionalExtension};

use super::{
    account, csrf_token, digest, equal_bytes, now, random_token, storage, storage_error, AuthError,
    AuthResult, AuthenticatedWebOwner, ReauthenticationProof, RevokeSessions, SensitiveCookie,
    SensitiveLoginRequest, SensitivePassword, SessionCookie, WebIdentityService, WebIdentityStore,
    WebRequestBinding, ABSOLUTE_TTL, IDLE_TTL, REAUTH_TTL,
};
use forme_protocol as p;

impl WebIdentityService for WebIdentityStore {
    fn login(&self, request: SensitiveLoginRequest) -> AuthResult<SessionCookie> {
        self.login_with_time(request, None, None)
    }

    fn authenticate(
        &self,
        cookie: &SensitiveCookie,
        request: WebRequestBinding,
    ) -> AuthResult<AuthenticatedWebOwner> {
        self.authenticate_with_time(cookie, request, None)
    }

    fn revoke(
        &self,
        owner: &AuthenticatedWebOwner,
        command: RevokeSessions,
    ) -> AuthResult<p::AuthRevision> {
        self.revoke_with_time(owner, command, None)
    }
}

impl WebIdentityStore {
    /// Verify the exact one-shot proof before coordinating a remote auth fence.
    /// This transaction intentionally rolls back; the final revoke consumes it.
    pub(crate) fn validate_revocation(
        &self,
        owner: &AuthenticatedWebOwner,
        command: &RevokeSessions,
    ) -> AuthResult<()> {
        if command.operation_digest != Self::revoke_operation(owner)? {
            return Err(AuthError::Conflict);
        }
        let mut db = self.database()?;
        let tx = db.connection.transaction().map_err(storage_error)?;
        let now = now();
        storage::check_owner(&tx, owner, now)?;
        storage::claim_proof(&tx, owner, &command.proof, &command.operation_digest, now)?;
        tx.rollback().map_err(storage_error)
    }

    pub fn revoke_operation(owner: &AuthenticatedWebOwner) -> AuthResult<p::SchemaDigest> {
        p::canonical_digest(&(
            p::SchemaVersion(1),
            "web-revoke-all-sessions-v1",
            &owner.view.account_ref,
            &owner.view.owner,
            owner.view.auth_epoch,
        ))
        .map_err(|_| AuthError::Unavailable)
    }

    #[cfg(test)]
    pub(super) fn login_at(
        &self,
        request: SensitiveLoginRequest,
        now: i64,
    ) -> AuthResult<SessionCookie> {
        self.login_with_time(request, Some(now), None)
    }

    pub fn login_challenged(
        &self,
        request: SensitiveLoginRequest,
        binding: super::LoginChallengeBinding<'_>,
    ) -> AuthResult<SessionCookie> {
        self.login_with_time(request, None, Some(binding))
    }

    pub(super) fn login_with_time(
        &self,
        request: SensitiveLoginRequest,
        supplied_time: Option<i64>,
        binding: Option<super::LoginChallengeBinding<'_>>,
    ) -> AuthResult<SessionCookie> {
        let _job = self.password_slot()?;
        if let Some(binding) = binding.as_ref() {
            let db = self.database()?;
            super::challenge::check(&db.connection, binding, supplied_time.unwrap_or_else(now))?;
        }
        let snapshot = account(&self.database()?.connection)?;
        let password = snapshot
            .as_ref()
            .map(|account| &account.password)
            .unwrap_or(&self.dummy);
        let verified = password.verify(&request.password.0)?;
        let expected = snapshot.as_ref().ok_or(AuthError::Unauthenticated)?;
        if !verified
            || !equal_bytes(
                expected.login_name.as_bytes(),
                request.login_name.as_bytes(),
            )
        {
            return Err(AuthError::Unauthenticated);
        }
        let cookie = SensitiveCookie::new(random_token()?);
        let csrf = csrf_token(&cookie)?;
        let session_digest = digest(&cookie.0)?;
        let csrf_digest = digest(&csrf.0)?;
        let mut db = self.database()?;
        let now = supplied_time.unwrap_or_else(now);
        let absolute = now
            .checked_add(ABSOLUTE_TTL)
            .filter(|_| now > 0)
            .ok_or(AuthError::InvalidInput)?;
        let idle = now.saturating_add(IDLE_TTL).min(absolute);
        let tx = db.connection.transaction().map_err(storage_error)?;
        let current = account(&tx)?.ok_or(AuthError::Unauthenticated)?;
        if current.credential_revision != expected.credential_revision
            || current.auth_epoch != expected.auth_epoch
        {
            return Err(AuthError::Conflict);
        }
        if let Some(binding) = binding.as_ref() {
            if let Some(session) = super::challenge::check(&tx, binding, now)? {
                return super::challenge::existing_session(&tx, &current, binding, &session, now);
            }
        }
        tx.execute(
            "DELETE FROM sessions WHERE idle<=?1 OR absolute<=?1 OR epoch<>?2",
            params![now, storage::sql_epoch(current.auth_epoch)?],
        )
        .map_err(storage_error)?;
        tx.execute(
            "DELETE FROM reauth WHERE expires<=?1 OR session NOT IN (SELECT digest FROM sessions)",
            [now],
        )
        .map_err(storage_error)?;
        let count: u32 = tx
            .query_row("SELECT count(*) FROM sessions", [], |r| r.get(0))
            .map_err(storage_error)?;
        if count >= 64 {
            return Err(AuthError::Busy);
        }
        tx.execute(
            "INSERT INTO sessions VALUES (?1,?2,?3,?4,?4,?5,?6)",
            params![
                session_digest,
                csrf_digest,
                storage::sql_epoch(current.auth_epoch)?,
                now,
                idle,
                absolute
            ],
        )
        .map_err(storage_error)?;
        if let Some(binding) = binding.as_ref() {
            tx.execute(
                "UPDATE login_challenges SET session=?1 WHERE digest=?2 AND session IS NULL",
                params![session_digest, binding.attempt.0],
            )
            .map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)?;
        Ok(SessionCookie {
            cookie,
            csrf,
            view: current.view(idle, absolute),
        })
    }

    #[cfg(test)]
    pub(super) fn authenticate_at(
        &self,
        cookie: &SensitiveCookie,
        request: WebRequestBinding,
        now: i64,
    ) -> AuthResult<AuthenticatedWebOwner> {
        self.authenticate_with_time(cookie, request, Some(now))
    }

    fn authenticate_with_time(
        &self,
        cookie: &SensitiveCookie,
        request: WebRequestBinding,
        supplied_time: Option<i64>,
    ) -> AuthResult<AuthenticatedWebOwner> {
        let session_digest = digest(&cookie.0)?;
        let csrf_digest = digest(&csrf_token(cookie)?.0)?;
        if request.mutation {
            let csrf = request.csrf.ok_or(AuthError::CsrfRejected)?;
            let supplied = digest(&csrf.0).map_err(|_| AuthError::CsrfRejected)?;
            if !equal_bytes(supplied.as_bytes(), csrf_digest.as_bytes()) {
                return Err(AuthError::CsrfRejected);
            }
        }
        let mut db = self.database()?;
        // Read the production clock after acquiring the writer. Requests that
        // wait for this lock must not look like clock rollback to later arrivals.
        let now = supplied_time.unwrap_or_else(now);
        let tx = db.connection.transaction().map_err(storage_error)?;
        let current = account(&tx)?.ok_or(AuthError::Unauthenticated)?;
        let absolute: Option<i64> = tx.query_row(
            "SELECT absolute FROM sessions WHERE digest=?1 AND csrf=?2 AND epoch=?3 AND issued<=?4
                AND seen<=?4 AND idle>?4 AND absolute>?4",
            params![session_digest, csrf_digest, storage::sql_epoch(current.auth_epoch)?, now], |r| r.get(0)
        ).optional().map_err(storage_error)?;
        let absolute = absolute.ok_or(AuthError::Unauthenticated)?;
        let idle = now.saturating_add(IDLE_TTL).min(absolute);
        tx.execute(
            "UPDATE sessions SET seen=?1,idle=?2 WHERE digest=?3",
            params![now, idle, session_digest],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(AuthenticatedWebOwner {
            session_digest,
            csrf_digest,
            view: current.view(idle, absolute),
        })
    }

    /// Called only after exact Host/Origin validation; no plaintext credential is
    /// persisted or sent to the brain. Caller sends this nonce in a response body.
    pub fn csrf_for_session(&self, cookie: &SensitiveCookie) -> AuthResult<SensitiveCookie> {
        self.authenticate(
            cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None,
            },
        )?;
        csrf_token(cookie)
    }

    pub fn logout(&self, owner: &AuthenticatedWebOwner) -> AuthResult<()> {
        self.logout_with_time(owner, None)
    }

    #[cfg(test)]
    pub(super) fn logout_at(&self, owner: &AuthenticatedWebOwner, now: i64) -> AuthResult<()> {
        self.logout_with_time(owner, Some(now))
    }

    fn logout_with_time(
        &self,
        owner: &AuthenticatedWebOwner,
        supplied_time: Option<i64>,
    ) -> AuthResult<()> {
        let mut db = self.database()?;
        let now = supplied_time.unwrap_or_else(now);
        let tx = db.connection.transaction().map_err(storage_error)?;
        let current = storage::check_owner(&tx, owner, now)?;
        let minimum_epoch = current
            .auth_epoch
            .checked_add(1)
            .ok_or(AuthError::RecoveryRequired)?;
        storage::enqueue_fence(
            &tx,
            &current.owner,
            &owner.session_digest,
            current.auth_epoch,
            Some(&owner.session_digest),
            minimum_epoch,
            now,
        )?;
        tx.execute(
            "DELETE FROM reauth WHERE session=?1",
            [&owner.session_digest],
        )
        .map_err(storage_error)?;
        tx.execute(
            "DELETE FROM sessions WHERE digest=?1",
            [&owner.session_digest],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)
    }

    pub fn reauthenticate(
        &self,
        owner: &AuthenticatedWebOwner,
        password: SensitivePassword,
        operation: &p::SchemaDigest,
    ) -> AuthResult<ReauthenticationProof> {
        self.reauthenticate_with_time(owner, password, operation, None)
    }

    #[cfg(test)]
    pub(super) fn reauthenticate_at(
        &self,
        owner: &AuthenticatedWebOwner,
        password: SensitivePassword,
        operation: &p::SchemaDigest,
        now: i64,
    ) -> AuthResult<ReauthenticationProof> {
        self.reauthenticate_with_time(owner, password, operation, Some(now))
    }

    fn reauthenticate_with_time(
        &self,
        owner: &AuthenticatedWebOwner,
        password: SensitivePassword,
        operation: &p::SchemaDigest,
        supplied_time: Option<i64>,
    ) -> AuthResult<ReauthenticationProof> {
        p::validate_product_digest(operation).map_err(|_| AuthError::InvalidInput)?;
        let _job = self.password_slot()?;
        let snapshot = {
            let db = self.database()?;
            storage::check_owner(&db.connection, owner, supplied_time.unwrap_or_else(now))?
        };
        if !snapshot.password.verify(&password.0)? {
            return Err(AuthError::Unauthenticated);
        }
        let token = SensitiveCookie::new(random_token()?);
        let proof_digest = digest(&token.0)?;
        let mut db = self.database()?;
        // KDF work can outlive the session. Recheck with the commit-time clock.
        let now = supplied_time.unwrap_or_else(now);
        let tx = db.connection.transaction().map_err(storage_error)?;
        let current = storage::check_owner(&tx, owner, now)?;
        if snapshot.credential_revision != current.credential_revision {
            return Err(AuthError::Conflict);
        }
        tx.execute("DELETE FROM reauth WHERE expires<=?1", [now])
            .map_err(storage_error)?;
        let count: u32 = tx
            .query_row("SELECT count(*) FROM reauth", [], |r| r.get(0))
            .map_err(storage_error)?;
        if count >= 64 {
            return Err(AuthError::Busy);
        }
        tx.execute(
            "INSERT INTO reauth VALUES (?1,?2,?3,?4,?5)",
            params![
                proof_digest,
                owner.session_digest,
                operation.0,
                storage::sql_epoch(current.auth_epoch)?,
                now.saturating_add(REAUTH_TTL)
            ],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(ReauthenticationProof(token))
    }

    #[cfg(test)]
    pub(super) fn revoke_at(
        &self,
        owner: &AuthenticatedWebOwner,
        command: RevokeSessions,
        now: i64,
    ) -> AuthResult<p::AuthRevision> {
        self.revoke_with_time(owner, command, Some(now))
    }

    fn revoke_with_time(
        &self,
        owner: &AuthenticatedWebOwner,
        command: RevokeSessions,
        supplied_time: Option<i64>,
    ) -> AuthResult<p::AuthRevision> {
        if command.operation_digest != Self::revoke_operation(owner)? {
            return Err(AuthError::Conflict);
        }
        let mut db = self.database()?;
        let now = supplied_time.unwrap_or_else(now);
        let tx = db.connection.transaction().map_err(storage_error)?;
        let mut current = storage::check_owner(&tx, owner, now)?;
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
        current.auth_epoch = next_epoch;
        storage::save_account(&tx, &current)?;
        tx.execute("DELETE FROM sessions", [])
            .map_err(storage_error)?;
        tx.execute("DELETE FROM reauth", [])
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(current.revision())
    }
}
