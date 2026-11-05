//! Resume the same two-party claim; no account hash leaves the Web process.
use rusqlite::params;

use super::*;
use crate::web::bootstrap::{BootstrapClientError, LocalBootstrapClient};
use crate::web::configuration::LocalConfigurationClient;

pub(crate) struct SetupClaimInput {
    pub cookie: SensitiveCookie,
    pub request_binding: WebRequestBinding,
    pub login: SensitiveLoginRequest,
    pub candidate_ref: String,
    pub operation: String,
    pub existing_session: Option<SensitiveCookie>,
}

impl From<BootstrapClientError> for AuthError {
    fn from(error: BootstrapClientError) -> Self {
        use p::BootstrapProblemCode as Code;
        match error.0 {
            Code::InvalidInput => Self::InvalidInput,
            Code::Unauthenticated => Self::Unauthenticated,
            Code::Conflict => Self::Conflict,
            Code::Expired => Self::Expired,
            Code::DataUnavailable | Code::RecoveryRequired => Self::RecoveryRequired,
            Code::Unavailable => Self::Unavailable,
        }
    }
}

impl WebIdentityStore {
    pub(crate) fn finish_bootstrap(
        &self,
        client: &LocalBootstrapClient,
        configuration: Option<&LocalConfigurationClient>,
        input: SetupClaimInput,
    ) -> AuthResult<SessionCookie> {
        let setup = self.setup_access(&input.cookie, input.request_binding, true)?;
        if setup.binding.operation_id != input.operation
            || setup.candidate.as_ref() != Some(&input.candidate_ref)
        {
            return Err(AuthError::Conflict);
        }
        let commitment = self.prepare_claim(
            &input.operation,
            &input.login.login_name,
            SensitivePassword::new(input.login.password.0.to_string()),
        )?;
        if let Some(candidate) = setup.legacy_candidate {
            let grant = client.adopt_data_v2(&p::BootstrapClaimRequestV2 {
                schema_version: p::SchemaVersion(2),
                session: super::setup_legacy::binding_v2(&setup.binding),
                candidate_ref: input.candidate_ref,
                expected_candidate_digest: candidate.candidate_digest,
                account_commitment: commitment,
            })?;
            self.commit_claim_activation(&grant)?;
        } else {
            let receipt = client.commit(&p::BootstrapCommitRequest {
                schema_version: p::SchemaVersion(1),
                session: setup.binding,
                candidate_ref: input.candidate_ref,
                account_commitment: commitment,
            })?;
            let configuration = configuration.ok_or(AuthError::Unavailable)?;
            configuration
                .bind_owner(&input.operation, 0, receipt.receipt.owner.clone())
                .map_err(|_| AuthError::Unavailable)?;
            self.commit_claim(&receipt)?;
        }
        let attempt = challenge::reference(&input.cookie)?;
        let csrf = SensitiveCookie::new(challenge::csrf(&input.cookie)?);
        {
            let db = self.database()?;
            let count: i64 = db
                .connection
                .query_row("SELECT count(*) FROM login_challenges", [], |r| r.get(0))
                .map_err(storage_error)?;
            if count >= 128 {
                return Err(AuthError::Busy);
            }
            db.connection
                .execute(
                    "INSERT OR IGNORE INTO login_challenges VALUES (?1,?2,NULL)",
                    params![attempt.0, now().saturating_add(5 * 60 * 1000)],
                )
                .map_err(storage_error)?;
        }
        self.login_challenged(
            input.login,
            LoginChallengeBinding {
                cookie: &input.cookie,
                csrf: &csrf,
                attempt: &attempt,
                existing_session: input.existing_session.as_ref(),
            },
        )
    }

    /// Startup recovery uses only a previously persisted explicit claim intent.
    /// A missing/unavailable broker leaves it pending, never creates a new owner.
    pub(crate) fn recover_bootstrap(
        &self,
        client: &LocalBootstrapClient,
        configuration: Option<&LocalConfigurationClient>,
    ) -> AuthResult<()> {
        if let Some(original) = self.pending_legacy_bootstrap()? {
            // Never call adopt on startup: only re-fetch an already activated
            // original result and complete its existing staged-account commit.
            let grant = client.claim_activation_v2(&p::BootstrapClaimQueryV2 {
                schema_version: p::SchemaVersion(2),
                session: original.session,
            })?;
            if grant.receipt.candidate_ref != original.candidate_ref
                || grant.receipt.candidate_digest != original.expected_candidate_digest
                || grant.receipt.account_commitment != original.account_commitment
            {
                return Err(AuthError::Conflict);
            }
            self.commit_claim_activation(&grant)?;
            return Ok(());
        }
        let Some(request) = self.pending_bootstrap()? else {
            return Ok(());
        };
        let grant = client.commit(&request)?;
        let configuration = configuration.ok_or(AuthError::Unavailable)?;
        configuration
            .bind_owner(
                &request.session.operation_id,
                0,
                grant.receipt.owner.clone(),
            )
            .map_err(|_| AuthError::Unavailable)?;
        self.commit_claim(&grant)?;
        Ok(())
    }
}
