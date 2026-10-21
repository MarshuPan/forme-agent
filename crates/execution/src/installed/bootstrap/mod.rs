//! Installation claim authority. No models, brain tables or Agent permissions.
mod config;
mod journal;

#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::{Mutex, MutexGuard};

use ed25519_dalek::{Signer, SigningKey};
use forme_protocol as p;
use p::BootstrapProblemCode as Code;

use super::{HostAuth, HostCredentialDomain};
pub use config::BootstrapAuthorityConfig;
use journal::{Fact, Journal, PROOF_TTL, SETUP_TTL};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapError(pub Code);
impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "installation setup rejected: {:?}", self.0)
    }
}
impl std::error::Error for BootstrapError {}
pub type BootstrapResult<T> = Result<T, BootstrapError>;

/// Private construction and per-call binding prevent cross-authority reuse.
pub struct BootstrapServiceAuth(p::SchemaDigest);

pub struct BootstrapAuthority {
    config: BootstrapAuthorityConfig,
    signing_key: SigningKey,
    web_digest: p::SchemaDigest,
    maintenance_digest: p::SchemaDigest,
    journal: Mutex<Journal>,
}

impl BootstrapAuthority {
    /// Configuration and keys must have installer-enforced service ACLs.
    /// This validates structure; it does not certify native isolation.
    pub fn open(
        config: BootstrapAuthorityConfig,
        maintenance_digest: p::SchemaDigest,
        agent_digest: Option<&p::SchemaDigest>,
    ) -> BootstrapResult<Self> {
        config.validate()?;
        p::validate_product_digest(&maintenance_digest)
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        let signing_key = SigningKey::from_bytes(&config::read_key(&config.signing_key_path)?);
        let credential = config::read_credential(&config.web_credential_path)?;
        let web_digest =
            p::canonical_digest(&credential).map_err(|_| BootstrapError(Code::Unavailable))?;
        if web_digest == maintenance_digest || agent_digest == Some(&web_digest) {
            return Err(BootstrapError(Code::InvalidInput));
        }
        let key_digest = p::canonical_digest(&(
            "forme-setup-issuer-v1",
            signing_key.verifying_key().to_bytes(),
        ))
        .map_err(|_| BootstrapError(Code::Unavailable))?;
        let journal = Journal::open(&config.journal_path, &config.install_ref, key_digest)?;
        Ok(Self {
            config,
            signing_key,
            web_digest,
            maintenance_digest,
            journal: Mutex::new(journal),
        })
    }

    pub fn verifying_key(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    pub fn authenticate_service(&self, bearer: &str) -> BootstrapResult<BootstrapServiceAuth> {
        if !(32..=256).contains(&bearer.len()) || bearer.chars().any(char::is_whitespace) {
            return Err(BootstrapError(Code::Unauthenticated));
        }
        let digest = p::canonical_digest(&bearer).map_err(|_| BootstrapError(Code::Unavailable))?;
        if digest != self.web_digest {
            return Err(BootstrapError(Code::Unauthenticated));
        }
        Ok(BootstrapServiceAuth(digest))
    }

    /// Only the authenticated local-management domain can issue/rotate a proof.
    /// A rotation revokes an unfinished setup; it never changes a claimed owner.
    pub fn issue_proof(
        &self,
        admin: &HostAuth,
        rotation: &str,
        now: i64,
    ) -> BootstrapResult<p::BootstrapProofView> {
        if admin.domain() != HostCredentialDomain::OwnerMaintenance
            || admin.actor() != p::MaintenanceActorClass::LocalAdministrator
            || admin.proof_ref() != &self.maintenance_digest
        {
            return Err(BootstrapError(Code::Unauthenticated));
        }
        p::validate_product_id(rotation).map_err(|_| BootstrapError(Code::InvalidInput))?;
        let mut journal = self.lock(now)?;
        if journal.state.rotations.contains(rotation) {
            return Err(BootstrapError(Code::Conflict));
        }
        if journal.state.committed.is_some() {
            return Err(BootstrapError(Code::Conflict));
        }
        let proof = random_secret()?;
        let expires = now
            .checked_add(PROOF_TTL)
            .ok_or(BootstrapError(Code::InvalidInput))?;
        let digest = proof_digest(&self.config.install_ref, &proof)?;
        journal.append(Fact::ProofIssued {
            operation: rotation.into(),
            digest,
            at: now,
            expires,
        })?;
        Ok(p::BootstrapProofView {
            schema_version: p::SchemaVersion(1),
            proof,
            expires_at: expires,
        })
    }

    /// Duplicate reservation returns its original deadline, never a new setup.
    pub fn reserve(
        &self,
        auth: &BootstrapServiceAuth,
        request: p::BootstrapReserveRequest,
        now: i64,
    ) -> BootstrapResult<p::BootstrapReservationView> {
        self.check_service(auth)?;
        version(request.schema_version)?;
        let session = p::BootstrapSessionBinding {
            schema_version: p::SchemaVersion(1),
            operation_id: request.operation_id,
            setup_digest: request.setup_digest,
        };
        session
            .validate()
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        let digest = proof_digest(&self.config.install_ref, &request.proof)?;
        let mut journal = self.lock(now)?;
        if journal.state.committed.is_some() {
            return Err(BootstrapError(Code::Conflict));
        }
        let proof = journal
            .state
            .proof
            .as_ref()
            .ok_or(BootstrapError(Code::Unauthenticated))?;
        if proof.0 != digest {
            return Err(BootstrapError(Code::Unauthenticated));
        }
        if journal.state.reservation.is_some() {
            let reservation = self.check_session(&journal, &session, now, false)?;
            return Ok(reservation_view(reservation));
        }
        if now >= proof.1 {
            return Err(BootstrapError(Code::Expired));
        }
        let expires = now
            .checked_add(SETUP_TTL)
            .ok_or(BootstrapError(Code::InvalidInput))?;
        journal.append(Fact::Reserved {
            operation: session.operation_id.clone(),
            setup: session.setup_digest,
            at: now,
            expires,
        })?;
        Ok(p::BootstrapReservationView {
            schema_version: p::SchemaVersion(1),
            operation_id: session.operation_id,
            expires_at: expires,
        })
    }

    pub fn candidate(
        &self,
        auth: &BootstrapServiceAuth,
        session: &p::BootstrapSessionBinding,
        now: i64,
    ) -> BootstrapResult<p::BootstrapDataCandidateView> {
        self.check_service(auth)?;
        let journal = self.lock(now)?;
        let reservation = self.check_session(&journal, session, now, false)?;
        let available = self.config.check_empty_target().is_ok();
        Ok(p::BootstrapDataCandidateView {
            schema_version: p::SchemaVersion(1),
            candidate_ref: self.config.candidate_ref.clone(),
            selected: reservation.selection.is_some(),
            available,
            issue_code: (!available).then_some(Code::DataUnavailable),
        })
    }

    pub fn select(
        &self,
        auth: &BootstrapServiceAuth,
        request: p::BootstrapDataChoice,
        now: i64,
    ) -> BootstrapResult<p::BootstrapDataCandidateView> {
        self.check_service(auth)?;
        version(request.schema_version)?;
        if request.candidate_ref != self.config.candidate_ref {
            return Err(BootstrapError(Code::InvalidInput));
        }
        let mut journal = self.lock(now)?;
        let reservation = self.check_session(&journal, &request.session, now, false)?;
        self.config.check_empty_target()?;
        if reservation.selection.is_none() {
            let owner = p::OwnerPrincipalRef(format!("owner:{}", random_secret()?));
            journal.append(Fact::Selected {
                candidate: request.candidate_ref.clone(),
                owner,
                at: now,
            })?;
        }
        Ok(p::BootstrapDataCandidateView {
            schema_version: p::SchemaVersion(1),
            candidate_ref: request.candidate_ref,
            selected: true,
            available: true,
            issue_code: None,
        })
    }

    /// Commit before signing. Lost responses can only reissue this exact claim.
    /// A committed retry may outlive setup TTL; it grants no other setup rights.
    pub fn commit(
        &self,
        auth: &BootstrapServiceAuth,
        request: p::BootstrapCommitRequest,
        now: i64,
    ) -> BootstrapResult<p::SignedWebClaimCommit> {
        self.check_service(auth)?;
        version(request.schema_version)?;
        p::validate_product_digest(&request.account_commitment)
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        let mut journal = self.lock(now)?;
        let reservation = self.check_session(&journal, &request.session, now, true)?;
        let (candidate, owner) = reservation
            .selection
            .as_ref()
            .ok_or(BootstrapError(Code::Conflict))?;
        if &request.candidate_ref != candidate {
            return Err(BootstrapError(Code::Conflict));
        }
        let expires_at = now
            .checked_add(5 * 60 * 1000)
            .ok_or(BootstrapError(Code::InvalidInput))?;
        let mut receipt = p::WebClaimCommit {
            schema_version: p::SchemaVersion(1),
            install_ref: self.config.install_ref.clone(),
            operation_id: request.session.operation_id,
            candidate_ref: candidate.clone(),
            owner: owner.clone(),
            account_commitment: request.account_commitment,
            expires_at,
        };
        if let Some(original) = journal.state.committed.as_ref() {
            let mut comparable = original.clone();
            comparable.expires_at = expires_at;
            if receipt != comparable {
                return Err(BootstrapError(Code::Conflict));
            }
            receipt = comparable;
        } else {
            self.config.check_empty_target()?;
            journal.append(Fact::Committed {
                receipt: receipt.clone(),
                at: now,
            })?;
        }
        let message = p::canonical_digest(&("forme-web-claim-commit-v1", &receipt))
            .map_err(|_| BootstrapError(Code::Unavailable))?;
        Ok(p::SignedWebClaimCommit {
            schema_version: p::SchemaVersion(1),
            receipt,
            signature: self
                .signing_key
                .sign(message.0.as_bytes())
                .to_bytes()
                .to_vec(),
        })
    }

    fn check_service(&self, auth: &BootstrapServiceAuth) -> BootstrapResult<()> {
        if auth.0 != self.web_digest {
            Err(BootstrapError(Code::Unauthenticated))
        } else {
            Ok(())
        }
    }

    fn lock(&self, now: i64) -> BootstrapResult<MutexGuard<'_, Journal>> {
        let journal = self
            .journal
            .lock()
            .map_err(|_| BootstrapError(Code::Unavailable))?;
        journal.ensure_healthy()?;
        if now <= 0 || now < journal.state.last_at {
            return Err(BootstrapError(Code::InvalidInput));
        }
        Ok(journal)
    }

    fn check_session<'a>(
        &self,
        journal: &'a Journal,
        session: &p::BootstrapSessionBinding,
        now: i64,
        committed_retry: bool,
    ) -> BootstrapResult<&'a journal::Reservation> {
        session
            .validate()
            .map_err(|_| BootstrapError(Code::InvalidInput))?;
        let reservation = journal
            .state
            .reservation
            .as_ref()
            .ok_or(BootstrapError(Code::Unauthenticated))?;
        if reservation.operation != session.operation_id
            || reservation.setup != session.setup_digest
        {
            return Err(BootstrapError(Code::Conflict));
        }
        if journal.state.committed.is_some() && committed_retry {
            return Ok(reservation);
        }
        if journal.state.committed.is_some() {
            return Err(BootstrapError(Code::Conflict));
        }
        if now >= reservation.expires {
            return Err(BootstrapError(Code::Expired));
        }
        Ok(reservation)
    }
}

fn reservation_view(value: &journal::Reservation) -> p::BootstrapReservationView {
    p::BootstrapReservationView {
        schema_version: p::SchemaVersion(1),
        operation_id: value.operation.clone(),
        expires_at: value.expires,
    }
}

fn version(value: p::SchemaVersion) -> BootstrapResult<()> {
    if value != p::SchemaVersion(1) {
        Err(BootstrapError(Code::InvalidInput))
    } else {
        Ok(())
    }
}

fn proof_digest(install: &str, proof: &str) -> BootstrapResult<p::SchemaDigest> {
    if proof.len() != 64
        || !proof
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(BootstrapError(Code::Unauthenticated));
    }
    p::canonical_digest(&("forme-local-install-proof-v1", install, proof))
        .map_err(|_| BootstrapError(Code::Unavailable))
}

fn random_secret() -> BootstrapResult<String> {
    use std::fmt::Write;
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| BootstrapError(Code::Unavailable))?;
    let mut result = String::with_capacity(64);
    for byte in bytes {
        write!(&mut result, "{byte:02x}").map_err(|_| BootstrapError(Code::Unavailable))?;
    }
    Ok(result)
}
