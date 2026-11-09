//! Validated fact folding and read-only state views share the same semantics.
use super::journal::*;
use super::*;

impl DesktopState {
    pub(super) fn apply(&mut self, fact: &DesktopFact, revision: u64) -> p::Result<()> {
        if revision != self.revision.checked_add(1).ok_or_else(invalid)? {
            return Err(invalid());
        }
        match fact {
            DesktopFact::Bound { identity } => {
                identity.validate()?;
                if self.revision != 0 || self.identity != *identity {
                    return Err(invalid());
                }
            }
            DesktopFact::Pending { request, at } => {
                request.validate()?;
                if self.revision == 0
                    || *at <= 0
                    || request.expires_at <= *at
                    || request.owner_ref != self.identity.owner_ref
                    || request.authority_epoch != self.identity.authority_epoch
                    || self.pending.contains_key(&request.pending_ref)
                {
                    return Err(invalid());
                }
                self.pending
                    .insert(request.pending_ref.clone(), request.clone());
            }
            DesktopFact::Consent {
                request,
                selected,
                consent_ref,
                at,
            } => {
                request.validate()?;
                selected.validate()?;
                p::validate_product_id(consent_ref)?;
                let pending = self.pending.get(&request.pending_ref).ok_or_else(invalid)?;
                if *at <= 0
                    || request.expected_revision != self.revision
                    || request.expires_at <= *at
                    || request.expires_at > pending.expires_at
                    || request.expires_at.saturating_sub(*at) > p::MAX_DESKTOP_SESSION_TTL_MS
                    || !selected.capabilities.contains(&pending.capability)
                    || self.consents.contains_key(consent_ref)
                    || self.operations.contains_key(&request.operation_id)
                    || self.grants.contains_key(&request.operation_id)
                    || self
                        .consents
                        .values()
                        .any(|v| v.request.pending_ref == request.pending_ref)
                {
                    return Err(invalid());
                }
                let digest = request.digest()?;
                if *consent_ref != p::desktop_consent_ref(&request.pending_ref)? {
                    return Err(invalid());
                }
                self.operations.insert(
                    request.operation_id.clone(),
                    OperationRecord {
                        digest,
                        consent_ref: consent_ref.clone(),
                        revision,
                    },
                );
                self.consents.insert(
                    consent_ref.clone(),
                    ConsentRecord {
                        request: request.clone(),
                        selected: selected.clone(),
                        revoked: false,
                    },
                );
            }
            DesktopFact::Revoke { request, at } => {
                request.validate()?;
                if *at <= 0
                    || request.expected_revision != self.revision
                    || self.operations.contains_key(&request.operation_id)
                    || self.grants.contains_key(&request.operation_id)
                {
                    return Err(invalid());
                }
                let consent = self
                    .consents
                    .get_mut(&request.consent_ref)
                    .ok_or_else(invalid)?;
                if consent.revoked {
                    return Err(invalid());
                }
                consent.revoked = true;
                self.operations.insert(
                    request.operation_id.clone(),
                    OperationRecord {
                        digest: revoke_digest(request)?,
                        consent_ref: request.consent_ref.clone(),
                        revision,
                    },
                );
            }
            DesktopFact::Grant {
                operation_id,
                request_digest,
                consent_ref,
                binding,
            } => {
                p::validate_product_id(operation_id)?;
                p::validate_product_digest(request_digest)?;
                binding.validate()?;
                let consent = self.consents.get(consent_ref).ok_or_else(invalid)?;
                let pending = self
                    .pending
                    .get(&consent.request.pending_ref)
                    .ok_or_else(invalid)?;
                let selected = &consent.selected;
                if consent.revoked
                    || binding.expires_at > consent.request.expires_at
                    || binding.expires_at > pending.expires_at
                    || binding.owner_ref != self.identity.owner_ref
                    || binding.authority_epoch != self.identity.authority_epoch
                    || binding.action_ref != pending.action_ref
                    || binding.plan_digest != pending.plan_digest
                    || binding.capabilities != [pending.capability].into_iter().collect()
                    || binding.helper_ref != selected.helper_ref
                    || binding.os_user_ref != selected.os_user_ref
                    || binding.os_session_ref != selected.os_session_ref
                    || binding.session_generation != selected.session_generation
                    || binding.platform != selected.platform
                    || binding.desktop_ref != selected.desktop_ref
                    || binding.grant_ref != format!("desktop-grant:{}", &request_digest.0[7..])
                    || self.operations.contains_key(operation_id)
                    || self.grants.contains_key(operation_id)
                    || self
                        .grants
                        .values()
                        .any(|g| g.consent_ref == *consent_ref || g.binding.nonce == binding.nonce)
                {
                    return Err(invalid());
                }
                self.grants.insert(
                    operation_id.clone(),
                    GrantRecord {
                        operation_id: operation_id.clone(),
                        request_digest: request_digest.clone(),
                        consent_ref: consent_ref.clone(),
                        binding: binding.clone(),
                        consumed: false,
                        revision,
                    },
                );
            }
            DesktopFact::Consume { operation_id, at } => {
                let grant = self.grants.get_mut(operation_id).ok_or_else(invalid)?;
                let consent = self.consents.get(&grant.consent_ref).ok_or_else(invalid)?;
                grant.binding.validate_at(*at)?;
                if grant.consumed || consent.revoked || consent.request.expires_at <= *at {
                    return Err(invalid());
                }
                grant.consumed = true;
                grant.revision = revision;
            }
        }
        self.revision = revision;
        Ok(())
    }
    pub(super) fn consent_view(
        &self,
        operation_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopConsentView> {
        if now <= 0 {
            return Err(invalid());
        }
        let operation = self.operations.get(operation_id).ok_or_else(invalid)?;
        let record = self
            .consents
            .get(&operation.consent_ref)
            .ok_or_else(invalid)?;
        let status = if record.revoked {
            p::DesktopConsentStatus::Revoked
        } else if now >= record.request.expires_at {
            p::DesktopConsentStatus::Expired
        } else {
            p::DesktopConsentStatus::Committed
        };
        Ok(p::DesktopConsentView {
            schema_version: p::SchemaVersion(1),
            operation_id: operation_id.into(),
            revision: operation.revision,
            pending_ref: record.request.pending_ref.clone(),
            consent_ref: operation.consent_ref.clone(),
            status,
            expires_at: record.request.expires_at,
            reason_ref: None,
        })
    }
    pub(super) fn grant_view(
        &self,
        operation_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopGrantView> {
        if now <= 0 {
            return Err(invalid());
        }
        let grant = self.grants.get(operation_id).ok_or_else(invalid)?;
        let consent = self.consents.get(&grant.consent_ref).ok_or_else(invalid)?;
        let status = if grant.consumed {
            p::DesktopGrantStatus::Consumed
        } else if consent.revoked {
            p::DesktopGrantStatus::Revoked
        } else if now >= grant.binding.expires_at {
            p::DesktopGrantStatus::Expired
        } else {
            p::DesktopGrantStatus::Issued
        };
        Ok(p::DesktopGrantView {
            schema_version: p::SchemaVersion(1),
            operation_id: operation_id.into(),
            revision: grant.revision,
            binding: grant.binding.clone(),
            status,
            reason_ref: None,
        })
    }
}
