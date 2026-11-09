//! Independent Host consent and issuance facts. No brain writer or helper launch.
use forme_protocol as p;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

mod journal;
mod managed;
pub use managed::InstalledDesktopAuthority;
mod output;
mod transitions;
#[cfg(windows)]
mod windows;
use journal::{DesktopFact, DesktopWriter};
#[cfg(windows)]
pub use windows::WindowsDesktopSessionCatalog;

/// Safe observation produced by the OS adapter, never accepted from Web/Core.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedDesktopSession {
    pub schema_version: p::SchemaVersion,
    pub helper_ref: String,
    pub platform: p::DesktopPlatform,
    pub label: String,
    pub os_user_ref: String,
    pub os_session_ref: String,
    pub session_generation: u64,
    pub desktop_ref: String,
    pub capabilities: BTreeSet<p::DesktopCapability>,
}
impl ObservedDesktopSession {
    fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.session_generation == 0
            || self.label.trim().is_empty()
            || self.label.len() > 128
            || self.label.chars().any(char::is_control)
            || self.capabilities.is_empty()
            || self.capabilities.contains(&p::DesktopCapability::PtyInput)
        {
            return Err(invalid());
        }
        for id in [
            &self.helper_ref,
            &self.os_user_ref,
            &self.os_session_ref,
            &self.desktop_ref,
        ] {
            p::validate_product_id(id)?;
        }
        Ok(())
    }
}
/// A directory read must never start a helper or acquire an execution grant.
pub trait DesktopSessionCatalogProvider: Send + Sync {
    fn sessions(&self) -> p::Result<Vec<ObservedDesktopSession>>;
    fn recheck(&self, original: &ObservedDesktopSession) -> p::Result<()>;
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityIdentity {
    schema_version: p::SchemaVersion,
    installation_ref: String,
    owner_ref: p::VerifiedPrincipal,
    authority_epoch: p::AuthorityEpoch,
}
impl AuthorityIdentity {
    fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1) || self.authority_epoch.0 == 0 {
            return Err(invalid());
        }
        p::validate_product_id(&self.installation_ref)?;
        p::validate_product_id(&self.owner_ref.0)
    }
}
struct CachedCatalog {
    public: p::DesktopSessionCatalog,
    originals: BTreeMap<String, ObservedDesktopSession>,
}
pub struct DesktopAuthority {
    writer: Mutex<DesktopWriter>,
    provider: Arc<dyn DesktopSessionCatalogProvider>,
}
impl DesktopAuthority {
    pub fn open(
        path: &Path,
        installation_ref: String,
        owner_ref: p::VerifiedPrincipal,
        authority_epoch: p::AuthorityEpoch,
        provider: Arc<dyn DesktopSessionCatalogProvider>,
    ) -> p::Result<Self> {
        let identity = AuthorityIdentity {
            schema_version: p::SchemaVersion(1),
            installation_ref,
            owner_ref,
            authority_epoch,
        };
        identity.validate()?;
        Ok(Self {
            writer: Mutex::new(DesktopWriter::open(path, identity)?),
            provider,
        })
    }
    /// Caller must authenticate the owner or AgentExecution domain before entry.
    pub fn identity_matches(
        &self,
        owner: &p::VerifiedPrincipal,
        epoch: p::AuthorityEpoch,
    ) -> p::Result<()> {
        let writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        if writer.state.identity.owner_ref != *owner
            || writer.state.identity.authority_epoch != epoch
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn sessions(&self, now: p::Timestamp) -> p::Result<p::DesktopSessionCatalog> {
        let mut writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        let observed = self.provider.sessions()?;
        if now <= 0 || observed.len() > p::DESKTOP_CATALOG_MAX_ENTRIES {
            return Err(invalid());
        }
        let mut originals = BTreeMap::new();
        let mut entries = Vec::new();
        for session in observed {
            session.validate()?;
            let digest = p::canonical_digest(&(
                "forme.desktop.session.choice.v1",
                &writer.state.identity,
                writer.state.revision,
                now,
                &session,
            ))?;
            let reference = format!("desktop-session:{}", &digest.0[7..]);
            entries.push(p::DesktopSessionChoice {
                schema_version: p::SchemaVersion(1),
                session_ref: reference.clone(),
                platform: session.platform,
                label: session.label.clone(),
                available: true,
                supported_capabilities: session.capabilities.clone(),
            });
            if originals.insert(reference, session).is_some() {
                return Err(invalid());
            }
        }
        let identity = &writer.state.identity;
        let mut public = p::DesktopSessionCatalog {
            schema_version: p::SchemaVersion(1),
            installation_ref: identity.installation_ref.clone(),
            owner_ref: identity.owner_ref.clone(),
            authority_epoch: identity.authority_epoch,
            catalog_revision: writer.state.revision,
            catalog_digest: p::canonical_digest(&"pending")?,
            entries,
            observed_at: now,
            expires_at: now
                .checked_add(p::DESKTOP_CATALOG_TTL_MS)
                .ok_or_else(invalid)?,
        };
        public.catalog_digest = public.digest()?;
        public.validate_at(now)?;
        writer.catalog = Some(CachedCatalog {
            public: public.clone(),
            originals,
        });
        Ok(public)
    }
    pub fn register_pending(
        &self,
        request: p::DesktopPendingAction,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopPendingAction> {
        request.validate()?;
        let mut writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        if request.owner_ref != writer.state.identity.owner_ref
            || request.authority_epoch != writer.state.identity.authority_epoch
        {
            return Err(invalid());
        }
        if let Some(existing) = writer.state.pending.get(&request.pending_ref) {
            return if existing == &request {
                Ok(existing.clone())
            } else {
                Err(invalid())
            };
        }
        if now <= 0 || request.expires_at <= now {
            return Err(invalid());
        }
        writer.append(DesktopFact::Pending {
            request: request.clone(),
            at: now,
        })?;
        Ok(request)
    }
    pub fn consent(
        &self,
        request: p::DesktopConsentRequest,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopConsentView> {
        request.validate()?;
        let digest = request.digest()?;
        let mut writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        if let Some(operation) = writer.state.operations.get(&request.operation_id) {
            if operation.digest != digest {
                return Err(invalid());
            }
            return writer.state.consent_view(&request.operation_id, now);
        }
        if request.expected_revision != writer.state.revision {
            return Err(invalid());
        }
        let catalog = writer.catalog.as_ref().ok_or_else(invalid)?;
        catalog.public.validate_at(now)?;
        if request.catalog_revision != catalog.public.catalog_revision
            || request.catalog_digest != catalog.public.catalog_digest
        {
            return Err(invalid());
        }
        let selected = catalog
            .originals
            .get(&request.session_ref)
            .ok_or_else(invalid)?
            .clone();
        self.provider.recheck(&selected)?;
        let consent_ref = p::desktop_consent_ref(&request.pending_ref)?;
        writer.append(DesktopFact::Consent {
            request: request.clone(),
            selected,
            consent_ref,
            at: now,
        })?;
        writer.state.consent_view(&request.operation_id, now)
    }
    pub fn consent_operation(
        &self,
        operation_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopConsentView> {
        p::validate_product_id(operation_id)?;
        if operation_id.starts_with("desktop-pending:") {
            return self.pending_consent(operation_id, now);
        }
        let writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        writer.state.consent_view(operation_id, now)
    }
    pub fn pending_consent(
        &self,
        pending_ref: &str,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopConsentView> {
        let consent_ref = p::desktop_consent_ref(pending_ref)?;
        let writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        let pending = writer.state.pending.get(pending_ref).ok_or_else(invalid)?;
        if let Some(consent) = writer.state.consents.get(&consent_ref) {
            return writer
                .state
                .consent_view(&consent.request.operation_id, now);
        }
        if pending.expires_at <= now {
            return Err(invalid());
        }
        let suffix = pending_ref
            .strip_prefix("desktop-pending:")
            .ok_or_else(invalid)?;
        let operation_id = format!("desktop-consent-request:{suffix}");
        p::validate_product_id(&operation_id)?;
        if now <= 0 {
            return Err(invalid());
        }
        Ok(p::DesktopConsentView {
            schema_version: p::SchemaVersion(1),
            operation_id,
            revision: writer.state.revision,
            pending_ref: pending_ref.into(),
            consent_ref,
            status: p::DesktopConsentStatus::Prepared,
            expires_at: pending.expires_at.min(
                now.checked_add(p::MAX_DESKTOP_SESSION_TTL_MS)
                    .ok_or_else(invalid)?,
            ),
            reason_ref: None,
        })
    }

    pub fn revoke(
        &self,
        request: p::DesktopRevokeRequest,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopConsentView> {
        request.validate()?;
        let digest = revoke_digest(&request)?;
        let mut writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        if let Some(operation) = writer.state.operations.get(&request.operation_id) {
            if operation.digest != digest {
                return Err(invalid());
            }
        } else {
            writer.append(DesktopFact::Revoke {
                request: request.clone(),
                at: now,
            })?;
        }
        writer.state.consent_view(&request.operation_id, now)
    }
    pub fn issue(
        &self,
        request: p::DesktopGrantRequest,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopGrantView> {
        let digest = request.digest()?;
        let mut writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        if let Some(grant) = writer.state.grants.get(&request.operation_id) {
            if grant.request_digest != digest {
                return Err(invalid());
            }
            return writer.state.grant_view(&request.operation_id, now);
        }
        request.validate_at(now)?;
        let consent = writer
            .state
            .consents
            .get(&request.consent_ref)
            .ok_or_else(invalid)?;
        let pending = writer
            .state
            .pending
            .get(&consent.request.pending_ref)
            .ok_or_else(invalid)?;
        pending.validate_for(&request, now)?;
        if consent.revoked || consent.request.expires_at <= now {
            return Err(invalid());
        }
        self.provider.recheck(&consent.selected)?;
        let selected = &consent.selected;
        let binding = p::DesktopSessionBinding {
            schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            helper_ref: selected.helper_ref.clone(),
            grant_ref: format!("desktop-grant:{}", &digest.0[7..]),
            owner_ref: request.job.owner_ref.clone(),
            os_user_ref: selected.os_user_ref.clone(),
            os_session_ref: selected.os_session_ref.clone(),
            session_generation: selected.session_generation,
            platform: selected.platform,
            desktop_ref: selected.desktop_ref.clone(),
            action_ref: request.job.action_ref.clone(),
            plan_digest: request.job.plan_digest.clone(),
            authority_epoch: request.job.authority_epoch,
            nonce: request.job.nonce.clone(),
            capabilities: [pending.capability].into_iter().collect(),
            issued_at: now,
            expires_at: consent
                .request
                .expires_at
                .min(request.job.deadline)
                .min(request.permit.deadline)
                .min(
                    now.checked_add(p::MAX_DESKTOP_SESSION_TTL_MS)
                        .ok_or_else(invalid)?,
                ),
        };
        binding.validate_for(&request.job, &request.permit, now)?;
        writer.append(DesktopFact::Grant {
            operation_id: request.operation_id.clone(),
            request_digest: digest,
            consent_ref: request.consent_ref,
            binding,
        })?;
        writer.state.grant_view(&request.operation_id, now)
    }
    pub fn grant_operation(
        &self,
        operation_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopGrantView> {
        p::validate_product_id(operation_id)?;
        let writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        writer.state.grant_view(operation_id, now)
    }
    /// Durably consumes before invoking the original admission while holding the
    /// authority lock. A crash or callback error cannot make this grant reusable.
    pub(crate) fn consume<T>(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: &p::WorkerExecutionSpec,
        now: p::Timestamp,
        admit: impl FnOnce() -> p::Result<T>,
    ) -> p::Result<T> {
        let binding = execution.desktop_session.as_ref().ok_or_else(invalid)?;
        let mut writer = self.writer.lock().map_err(|_| invalid())?;
        writer.healthy()?;
        let grant = writer
            .state
            .grants
            .values()
            .find(|g| g.binding.grant_ref == binding.grant_ref)
            .ok_or_else(invalid)?;
        if grant.consumed || grant.binding != *binding {
            return Err(invalid());
        }
        let consent = writer
            .state
            .consents
            .get(&grant.consent_ref)
            .ok_or_else(invalid)?;
        if consent.revoked || consent.request.expires_at <= now {
            return Err(invalid());
        }
        binding.validate_for(job, permit, now)?;
        execution.validate_for(job, permit, now)?;
        let mut original_execution = execution.clone();
        original_execution.desktop_session = None;
        let request = p::DesktopGrantRequest {
            schema_version: p::SchemaVersion(1),
            operation_id: grant.operation_id.clone(),
            consent_ref: grant.consent_ref.clone(),
            job: job.clone(),
            permit: permit.clone(),
            execution: original_execution,
        };
        writer
            .state
            .pending
            .get(&consent.request.pending_ref)
            .ok_or_else(invalid)?
            .validate_for(&request, now)?;
        if request.digest()? != grant.request_digest {
            return Err(invalid());
        }
        self.provider.recheck(&consent.selected)?;
        let operation_id = grant.operation_id.clone();
        writer.append(DesktopFact::Consume {
            operation_id,
            at: now,
        })?;
        admit()
    }
}
fn revoke_digest(request: &p::DesktopRevokeRequest) -> p::Result<p::SchemaDigest> {
    p::canonical_digest(&("forme.product.desktop.revoke.v1", request))
}
fn invalid() -> p::Error {
    p::Error(
        "desktop authority is unavailable, stale, or not bound to the original operation".into(),
    )
}

#[cfg(test)]
mod tests;
