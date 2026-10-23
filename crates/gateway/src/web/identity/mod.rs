//! Independent durable owner authentication. No brain-store or model access.
mod account;
mod challenge;
mod local_fence;
pub(crate) use local_fence::fence_installed_sessions;
mod maintenance;
mod password;
mod proposals;
mod recovery;
mod sessions;
mod setup;
mod setup_claim;
mod storage;
mod workbench;

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
#[derive(Clone)]
pub struct AuthenticatedWebOwner {
    session_digest: String,
    csrf_digest: String,
    pub(super) view: p::WebSessionView,
}

pub(crate) struct CoreSessionBinding {
    pub(crate) session_digest: String,
    pub(crate) owner: p::OwnerPrincipalRef,
    pub(crate) auth_epoch: u64,
}

/// A local-only, durable reduction of a Web session fence. It contains only
/// opaque digests and the monotonic epoch; it is never sent to the browser or
/// written to the brain store.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FenceOutboxRecord {
    pub(crate) schema_version: p::SchemaVersion,
    pub(crate) operation: p::SchemaDigest,
    pub(crate) owner: p::OwnerPrincipalRef,
    pub(crate) source_session: p::SchemaDigest,
    pub(crate) source_auth_epoch: u64,
    pub(crate) target_session: Option<p::SchemaDigest>,
    pub(crate) minimum_auth_epoch: u64,
    pub(crate) created_at: p::Timestamp,
}

impl FenceOutboxRecord {
    pub(crate) fn validate(&self) -> AuthResult<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.source_auth_epoch == 0
            || self.minimum_auth_epoch <= self.source_auth_epoch
            || self.created_at <= 0
        {
            return Err(AuthError::RecoveryRequired);
        }
        self.owner
            .validate()
            .map_err(|_| AuthError::RecoveryRequired)?;
        let expected = p::canonical_digest(&(
            p::SchemaVersion(1),
            "forme-web-fence-outbox-v1",
            &self.owner,
            &self.source_session.0,
            self.source_auth_epoch,
            self.target_session.as_ref().map(|value| &value.0),
            self.minimum_auth_epoch,
        ))
        .map_err(|_| AuthError::RecoveryRequired)?;
        if expected != self.operation {
            return Err(AuthError::RecoveryRequired);
        }
        p::validate_product_digest(&self.operation)
            .and_then(|_| p::validate_product_digest(&self.source_session))
            .and_then(|_| {
                self.target_session
                    .as_ref()
                    .map_or(Ok(()), p::validate_product_digest)
            })
            .map_err(|_| AuthError::RecoveryRequired)
    }

    pub(crate) fn binding(&self) -> CoreSessionBinding {
        CoreSessionBinding {
            session_digest: self.source_session.0.clone(),
            owner: self.owner.clone(),
            auth_epoch: self.source_auth_epoch,
        }
    }
}

impl AuthenticatedWebOwner {
    pub fn view(&self) -> &p::WebSessionView {
        &self.view
    }

    pub(crate) fn core_binding(&self) -> CoreSessionBinding {
        CoreSessionBinding {
            session_digest: self.session_digest.clone(),
            owner: self.view.owner.clone(),
            auth_epoch: self.view.auth_epoch,
        }
    }

    pub(crate) fn owner(&self) -> &p::OwnerPrincipalRef {
        &self.view.owner
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
    operation_id: String,
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

    pub(crate) fn pending_fences(&self) -> AuthResult<Vec<FenceOutboxRecord>> {
        let db = self.database()?;
        storage::pending_fences(&db.connection)
    }

    pub(crate) fn acknowledge_fence(&self, operation: &p::SchemaDigest) -> AuthResult<()> {
        let mut db = self.database()?;
        let tx = db.connection.transaction().map_err(storage_error)?;
        let removed = tx
            .execute(
                "DELETE FROM fence_outbox WHERE operation=?1",
                [&operation.0],
            )
            .map_err(storage_error)?;
        if removed > 1 {
            return Err(AuthError::RecoveryRequired);
        }
        tx.commit().map_err(storage_error)
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
