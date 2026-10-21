//! Independent durable owner authentication. No brain-store or model access.
mod account;
mod challenge;
mod password;
mod sessions;
mod setup;
mod setup_claim;
mod storage;

#[cfg(test)]
mod tests;

use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use ed25519_dalek::VerifyingKey;
use forme_protocol as p;
use rusqlite::Connection;
use zeroize::Zeroizing;

use password::{equal_bytes, PasswordRecord};
pub(crate) use setup_claim::SetupClaimInput;
use storage::{Account, Database};

pub type AuthResult<T> = Result<T, AuthError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    InvalidInput,
    Unauthenticated,
    CsrfRejected,
    Conflict,
    Expired,
    Busy,
    Unavailable,
    RecoveryRequired,
}

impl fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidInput => "invalid authentication input",
            Self::Unauthenticated => "authentication required or credentials invalid",
            Self::CsrfRejected => "request binding rejected",
            Self::Conflict => "authentication state changed; refresh before continuing",
            Self::Expired => "authentication proof expired",
            Self::Busy => "authentication capacity temporarily unavailable",
            Self::Unavailable => "authentication storage or cryptography unavailable",
            Self::RecoveryRequired => "authentication state requires local recovery",
        })
    }
}

impl std::error::Error for AuthError {}

/// No Debug/Serialize: passwords and raw cookies cannot become diagnostics.
pub struct SensitivePassword(Zeroizing<String>);
impl SensitivePassword {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
}

pub struct SensitiveLoginRequest {
    pub login_name: String,
    pub password: SensitivePassword,
}

pub struct SensitiveCookie(Zeroizing<String>);
impl SensitiveCookie {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
    pub fn expose_for_transport(&self) -> &str {
        &self.0
    }
}

pub struct SessionCookie {
    pub cookie: SensitiveCookie,
    pub csrf: SensitiveCookie,
    pub view: p::WebSessionView,
}

pub struct LoginChallenge {
    pub cookie: SensitiveCookie,
    pub view: p::WebLoginChallengeView,
}

pub struct LoginChallengeBinding<'a> {
    pub cookie: &'a SensitiveCookie,
    pub csrf: &'a SensitiveCookie,
    pub attempt: &'a p::SchemaDigest,
    pub existing_session: Option<&'a SensitiveCookie>,
}

/// Transport must check exact Host/Origin before constructing this binding.
pub struct WebRequestBinding {
    pub mutation: bool,
    pub csrf: Option<SensitiveCookie>,
}

/// Constructor and session digest are private to the authentication service.
pub struct AuthenticatedWebOwner {
    session_digest: String,
    csrf_digest: String,
    pub(super) view: p::WebSessionView,
}

impl AuthenticatedWebOwner {
    pub fn view(&self) -> &p::WebSessionView {
        &self.view
    }
}

pub struct ReauthenticationProof(SensitiveCookie);

impl ReauthenticationProof {
    pub fn from_transport(value: String) -> AuthResult<Self> {
        let cookie = SensitiveCookie::new(value);
        digest(cookie.expose_for_transport())?;
        Ok(Self(cookie))
    }

    pub fn expose_for_transport(&self) -> &str {
        self.0.expose_for_transport()
    }
}

impl<'de> serde::Deserialize<'de> for ReauthenticationProof {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::from_transport(value).map_err(serde::de::Error::custom)
    }
}

impl<'de> serde::Deserialize<'de> for SensitivePassword {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        <String as serde::Deserialize>::deserialize(deserializer).map(Self::new)
    }
}

/// In-flight password replacement with an immutable salted-hash commitment.
/// This object is never a wire DTO and cannot be constructed from client JSON.
pub struct PreparedPasswordChange {
    password: PasswordRecord,
    operation: p::SchemaDigest,
    account_ref: String,
    credential_revision: u64,
    auth_epoch: u64,
}

impl PreparedPasswordChange {
    pub fn operation_digest(&self) -> &p::SchemaDigest {
        &self.operation
    }
}

pub struct RevokeSessions {
    pub operation_digest: p::SchemaDigest,
    pub proof: ReauthenticationProof,
}

pub trait WebIdentityService: Send + Sync {
    fn login(&self, request: SensitiveLoginRequest) -> AuthResult<SessionCookie>;
    fn authenticate(
        &self,
        cookie: &SensitiveCookie,
        request: WebRequestBinding,
    ) -> AuthResult<AuthenticatedWebOwner>;
    fn revoke(
        &self,
        owner: &AuthenticatedWebOwner,
        command: RevokeSessions,
    ) -> AuthResult<p::AuthRevision>;
}

pub struct WebIdentityStore {
    database: Mutex<Database>,
    claim_key: VerifyingKey,
    password_jobs: AtomicUsize,
    dummy: PasswordRecord,
}

const IDLE_TTL: i64 = 30 * 60 * 1000;
const ABSOLUTE_TTL: i64 = 12 * 60 * 60 * 1000;
const REAUTH_TTL: i64 = 5 * 60 * 1000;

impl WebIdentityStore {
    /// `directory` must already have the Web-service-only installation ACL.
    /// Unknown/foreign/corrupt files are rejected, never replaced or repaired.
    pub fn open(directory: &Path, install: &str, claim_key: VerifyingKey) -> AuthResult<Self> {
        p::validate_product_id(install).map_err(|_| AuthError::InvalidInput)?;
        Ok(Self {
            database: Mutex::new(Database::open(directory, install, &claim_key.to_bytes())?),
            claim_key,
            password_jobs: AtomicUsize::new(0),
            dummy: PasswordRecord::new(&random_token()?)?,
        })
    }

    fn database(&self) -> AuthResult<MutexGuard<'_, Database>> {
        self.database.lock().map_err(|_| AuthError::Unavailable)
    }

    fn password_slot(&self) -> AuthResult<PasswordJob<'_>> {
        self.password_jobs
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 2).then_some(count + 1)
            })
            .map_err(|_| AuthError::Busy)?;
        Ok(PasswordJob(&self.password_jobs))
    }
}

struct PasswordJob<'a>(&'a AtomicUsize);
impl Drop for PasswordJob<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn digest(value: &str) -> AuthResult<String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(AuthError::Unauthenticated);
    }
    p::canonical_digest(&(p::SchemaVersion(1), "web-session-secret", value))
        .map(|d| d.0)
        .map_err(|_| AuthError::Unavailable)
}

fn random_token() -> AuthResult<String> {
    use std::fmt::Write;
    let mut bytes = Zeroizing::new([0; 32]);
    getrandom::fill(bytes.as_mut()).map_err(|_| AuthError::Unavailable)?;
    let mut token = String::with_capacity(64);
    for byte in bytes.iter() {
        write!(&mut token, "{byte:02x}").map_err(|_| AuthError::Unavailable)?;
    }
    Ok(token)
}

fn storage_error(_: rusqlite::Error) -> AuthError {
    AuthError::Unavailable
}

fn now() -> p::Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// Refresh can reconstruct this nonce from the HttpOnly cookie without storing
/// its plaintext. Domain separation keeps it distinct from a login credential.
fn csrf_token(cookie: &SensitiveCookie) -> AuthResult<SensitiveCookie> {
    let value = p::canonical_digest(&(
        p::SchemaVersion(1),
        "forme-web-csrf-v1",
        cookie.expose_for_transport(),
    ))
    .map_err(|_| AuthError::Unavailable)?;
    Ok(SensitiveCookie::new(
        value.0.trim_start_matches("sha256:").to_owned(),
    ))
}

fn account(connection: &Connection) -> AuthResult<Option<Account>> {
    use rusqlite::OptionalExtension;
    let value: Option<String> = connection
        .query_row("SELECT value FROM account WHERE singleton = 1", [], |row| {
            row.get(0)
        })
        .optional()
        .map_err(storage_error)?;
    value
        .map(|json| serde_json::from_str(&json).map_err(|_| AuthError::RecoveryRequired))
        .transpose()
}
