//! Five-minute login CSRF challenges with durable single-session consumption.
use rusqlite::{params, OptionalExtension};

use super::*;

const LOGIN_TTL: i64 = 5 * 60 * 1000;

fn reference(cookie: &SensitiveCookie) -> AuthResult<p::SchemaDigest> {
    digest(cookie.expose_for_transport())?;
    p::canonical_digest(&("forme-login-attempt-v1", cookie.expose_for_transport()))
        .map_err(|_| AuthError::Unavailable)
}

fn csrf(cookie: &SensitiveCookie) -> AuthResult<String> {
    p::canonical_digest(&("forme-login-csrf-v1", cookie.expose_for_transport()))
        .map(|digest| digest.0.trim_start_matches("sha256:").into())
        .map_err(|_| AuthError::Unavailable)
}

impl WebIdentityStore {
    pub fn is_claimed(&self) -> AuthResult<bool> {
        account(&self.database()?.connection).map(|account| account.is_some())
    }

    pub fn login_challenge(&self) -> AuthResult<LoginChallenge> {
        self.login_challenge_with_time(None)
    }

    #[cfg(test)]
    pub(super) fn login_challenge_at(&self, now: i64) -> AuthResult<LoginChallenge> {
        self.login_challenge_with_time(Some(now))
    }

    fn login_challenge_with_time(&self, time: Option<i64>) -> AuthResult<LoginChallenge> {
        let cookie = SensitiveCookie::new(random_token()?);
        let attempt_ref = reference(&cookie)?;
        let csrf = csrf(&cookie)?;
        let mut db = self.database()?;
        let now = time.unwrap_or_else(now);
        let expires_at = now.checked_add(LOGIN_TTL).ok_or(AuthError::Unavailable)?;
        let tx = db.connection.transaction().map_err(storage_error)?;
        tx.execute("DELETE FROM login_challenges WHERE expires<=?1", [now])
            .map_err(storage_error)?;
        let count: i64 = tx
            .query_row("SELECT count(*) FROM login_challenges", [], |row| {
                row.get(0)
            })
            .map_err(storage_error)?;
        if count >= 128 {
            return Err(AuthError::Busy);
        }
        tx.execute(
            "INSERT INTO login_challenges VALUES (?1,?2,NULL)",
            params![attempt_ref.0, expires_at],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(LoginChallenge {
            cookie,
            view: p::WebLoginChallengeView {
                schema_version: p::SchemaVersion(1),
                attempt_ref,
                csrf,
                expires_at,
            },
        })
    }
}

pub(super) fn check(
    connection: &Connection,
    binding: &LoginChallengeBinding<'_>,
    now: i64,
) -> AuthResult<Option<String>> {
    if &reference(binding.cookie)? != binding.attempt
        || !equal_bytes(csrf(binding.cookie)?.as_bytes(), binding.csrf.0.as_bytes())
    {
        return Err(AuthError::CsrfRejected);
    }
    let row: Option<Option<String>> = connection
        .query_row(
            "SELECT session FROM login_challenges WHERE digest=?1 AND expires>?2 AND expires<=?3",
            params![binding.attempt.0, now, now.saturating_add(LOGIN_TTL)],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    row.ok_or(AuthError::Expired)
}

pub(super) fn existing_session(
    connection: &Connection,
    current: &storage::Account,
    binding: &LoginChallengeBinding<'_>,
    session: &str,
    now: i64,
) -> AuthResult<SessionCookie> {
    let cookie = binding.existing_session.ok_or(AuthError::Conflict)?;
    if !equal_bytes(digest(&cookie.0)?.as_bytes(), session.as_bytes()) {
        return Err(AuthError::Conflict);
    }
    let (idle, absolute) = connection
        .query_row(
            "SELECT idle,absolute FROM sessions WHERE digest=?1 AND epoch=?2
                AND issued<=?3 AND seen<=?3 AND idle>?3 AND absolute>?3",
            params![session, storage::sql_epoch(current.auth_epoch)?, now],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or(AuthError::Conflict)?;
    Ok(SessionCookie {
        cookie: SensitiveCookie::new(cookie.expose_for_transport().into()),
        csrf: csrf_token(cookie)?,
        view: current.view(idle, absolute),
    })
}
