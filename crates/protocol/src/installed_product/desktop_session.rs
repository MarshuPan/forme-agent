//! Closed product-side contract for a user desktop session helper.
//!
//! The helper is an execution placement only. It never owns brain state,
//! policy, credentials, or the EventStore.
use std::collections::BTreeSet;

use super::{validate_product_digest, validate_product_id};
use crate::{
    ActionId, AuthorityEpoch, Error, Nonce, PlanDigest, SchemaDigest, SchemaVersion, Timestamp,
    VerifiedPrincipal,
};
use serde::{Deserialize, Serialize};

pub const DESKTOP_SESSION_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
pub const MAX_DESKTOP_SESSION_TTL_MS: i64 = 15 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPlatform {
    Windows,
    Macos,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopCapability {
    PointerMove,
    PointerClick,
    KeyboardType,
    KeyboardKey,
    Scroll,
    Screenshot,
    PtyInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopSessionState {
    Absent,
    Starting,
    Ready,
    Revoked,
    Expired,
    Unavailable,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopSessionLiveness {
    Live,
    Stale,
    Lost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopActionOutcome {
    Committed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopActionPhase {
    Pending,
    Started,
    Committed,
    Failed,
    Cancelled,
    Unknown,
}

impl DesktopActionPhase {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Committed | Self::Failed | Self::Cancelled | Self::Unknown
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopSessionBinding {
    pub schema_version: SchemaVersion,
    pub helper_ref: String,
    pub grant_ref: String,
    pub owner_ref: VerifiedPrincipal,
    pub os_user_ref: String,
    pub os_session_ref: String,
    pub session_generation: u64,
    pub platform: DesktopPlatform,
    pub desktop_ref: String,
    pub action_ref: ActionId,
    pub plan_digest: PlanDigest,
    pub authority_epoch: AuthorityEpoch,
    pub nonce: Nonce,
    pub capabilities: BTreeSet<DesktopCapability>,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
}

impl DesktopSessionBinding {
    pub fn validate(&self) -> crate::Result<()> {
        if self.schema_version != DESKTOP_SESSION_SCHEMA_VERSION {
            return Err(Error("unsupported desktop session schema".into()));
        }
        for (value, name) in [
            (&self.helper_ref, "desktop helper"),
            (&self.grant_ref, "desktop grant"),
            (&self.os_user_ref, "desktop OS user"),
            (&self.os_session_ref, "desktop OS session"),
            (&self.desktop_ref, "desktop reference"),
        ] {
            validate_product_id(value).map_err(|_| Error(format!("{name} is invalid")))?;
        }
        validate_product_id(&self.owner_ref.0)
            .map_err(|_| Error("desktop owner is invalid".into()))?;
        validate_product_id(&self.action_ref.0)
            .map_err(|_| Error("desktop action is invalid".into()))?;
        validate_product_id(&self.nonce.0).map_err(|_| Error("desktop nonce is invalid".into()))?;
        validate_product_digest(&SchemaDigest(self.plan_digest.0.clone()))?;
        if self.authority_epoch.0 == 0 || self.session_generation == 0 {
            return Err(Error("desktop session fencing is invalid".into()));
        }
        if self.capabilities.is_empty() || self.capabilities.len() > 16 {
            return Err(Error("desktop capability set is invalid".into()));
        }
        if self.issued_at <= 0
            || self.expires_at <= self.issued_at
            || self.expires_at - self.issued_at > MAX_DESKTOP_SESSION_TTL_MS
        {
            return Err(Error("desktop session lifetime is invalid".into()));
        }
        Ok(())
    }

    pub fn validate_at(&self, now: Timestamp) -> crate::Result<()> {
        self.validate()?;
        if now < self.issued_at || now >= self.expires_at {
            return Err(Error("desktop session grant is not currently valid".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopSessionObservation {
    pub schema_version: SchemaVersion,
    pub helper_ref: String,
    pub grant_ref: Option<String>,
    pub state: DesktopSessionState,
    pub liveness: DesktopSessionLiveness,
    pub os_user_ref: Option<String>,
    pub os_session_ref: Option<String>,
    pub session_generation: Option<u64>,
    pub platform: DesktopPlatform,
    pub desktop_ref: Option<String>,
    pub peer_ref: Option<String>,
    pub receipt_ref: Option<String>,
    pub reason_ref: Option<String>,
    pub observed_at: Timestamp,
}

impl DesktopSessionObservation {
    pub fn validate(&self) -> crate::Result<()> {
        if self.schema_version != DESKTOP_SESSION_SCHEMA_VERSION {
            return Err(Error("unsupported desktop observation schema".into()));
        }
        validate_product_id(&self.helper_ref)
            .map_err(|_| Error("desktop observation helper is invalid".into()))?;
        for (value, name) in [
            (&self.grant_ref, "desktop observation grant"),
            (&self.os_user_ref, "desktop observation OS user"),
            (&self.os_session_ref, "desktop observation OS session"),
            (&self.desktop_ref, "desktop observation reference"),
            (&self.peer_ref, "desktop observation peer"),
            (&self.receipt_ref, "desktop observation receipt"),
            (&self.reason_ref, "desktop observation reason"),
        ] {
            if let Some(value) = value {
                validate_product_id(value).map_err(|_| Error(format!("{name} is invalid")))?;
            }
        }
        if self.session_generation == Some(0) || self.observed_at <= 0 {
            return Err(Error("desktop observation is invalid".into()));
        }
        if self.state == DesktopSessionState::Ready
            && (self.liveness != DesktopSessionLiveness::Live
                || self.os_user_ref.is_none()
                || self.os_session_ref.is_none()
                || self.session_generation.is_none()
                || self.desktop_ref.is_none()
                || self.peer_ref.is_none())
        {
            return Err(Error(
                "ready desktop helper lacks live session proof".into(),
            ));
        }
        Ok(())
    }

    pub fn validate_for(&self, binding: &DesktopSessionBinding) -> crate::Result<()> {
        self.validate()?;
        binding.validate()?;
        if self.state != DesktopSessionState::Ready
            || self.liveness != DesktopSessionLiveness::Live
            || self.helper_ref != binding.helper_ref
            || self.grant_ref.as_deref() != Some(binding.grant_ref.as_str())
            || self.os_user_ref.as_deref() != Some(binding.os_user_ref.as_str())
            || self.os_session_ref.as_deref() != Some(binding.os_session_ref.as_str())
            || self.session_generation != Some(binding.session_generation)
            || self.platform != binding.platform
            || self.desktop_ref.as_deref() != Some(binding.desktop_ref.as_str())
        {
            return Err(Error(
                "desktop session observation is not bound to the grant".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopActionRequest {
    pub schema_version: SchemaVersion,
    pub job_id: String,
    pub permit_ref: String,
    pub binding: DesktopSessionBinding,
    pub normalized_action_digest: SchemaDigest,
    pub capability: DesktopCapability,
    pub deadline: Timestamp,
}

impl DesktopActionRequest {
    pub fn validate_at(
        &self,
        observation: &DesktopSessionObservation,
        now: Timestamp,
    ) -> crate::Result<()> {
        if self.schema_version != DESKTOP_SESSION_SCHEMA_VERSION {
            return Err(Error("unsupported desktop action schema".into()));
        }
        validate_product_id(&self.job_id)
            .map_err(|_| Error("desktop action job is invalid".into()))?;
        validate_product_id(&self.permit_ref)
            .map_err(|_| Error("desktop action permit is invalid".into()))?;
        validate_product_digest(&self.normalized_action_digest)?;
        self.binding.validate_at(now)?;
        observation.validate_for(&self.binding)?;
        if !self.binding.capabilities.contains(&self.capability)
            || self.deadline <= now
            || self.deadline > self.binding.expires_at
        {
            return Err(Error("desktop action is outside its grant".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopActionReceipt {
    pub schema_version: SchemaVersion,
    pub job_id: String,
    pub permit_ref: String,
    pub action_ref: ActionId,
    pub plan_digest: PlanDigest,
    pub nonce: Nonce,
    pub normalized_action_digest: SchemaDigest,
    pub outcome: DesktopActionOutcome,
    pub result_ref: Option<String>,
    pub observed_at: Timestamp,
}

impl DesktopActionReceipt {
    pub fn validate_for(&self, request: &DesktopActionRequest) -> crate::Result<()> {
        if self.schema_version != DESKTOP_SESSION_SCHEMA_VERSION {
            return Err(Error("unsupported desktop receipt schema".into()));
        }
        request.binding.validate()?;
        validate_product_id(&self.job_id)
            .map_err(|_| Error("desktop receipt job is invalid".into()))?;
        validate_product_id(&self.permit_ref)
            .map_err(|_| Error("desktop receipt permit is invalid".into()))?;
        validate_product_id(&self.action_ref.0)
            .map_err(|_| Error("desktop receipt action is invalid".into()))?;
        validate_product_id(&self.nonce.0)
            .map_err(|_| Error("desktop receipt nonce is invalid".into()))?;
        validate_product_digest(&SchemaDigest(self.plan_digest.0.clone()))?;
        validate_product_digest(&self.normalized_action_digest)?;
        if let Some(result_ref) = &self.result_ref {
            validate_product_id(result_ref)
                .map_err(|_| Error("desktop receipt result is invalid".into()))?;
        }
        if self.job_id != request.job_id
            || self.permit_ref != request.permit_ref
            || self.action_ref != request.binding.action_ref
            || self.plan_digest != request.binding.plan_digest
            || self.nonce != request.binding.nonce
            || self.normalized_action_digest != request.normalized_action_digest
            || self.observed_at <= 0
        {
            return Err(Error("desktop receipt is not bound to the request".into()));
        }
        Ok(())
    }
}

pub fn validate_desktop_action_transition(
    current: DesktopActionPhase,
    next: DesktopActionPhase,
) -> crate::Result<()> {
    let valid = match current {
        DesktopActionPhase::Pending => matches!(
            next,
            DesktopActionPhase::Started
                | DesktopActionPhase::Failed
                | DesktopActionPhase::Cancelled
        ),
        DesktopActionPhase::Started => next.is_terminal(),
        DesktopActionPhase::Committed
        | DesktopActionPhase::Failed
        | DesktopActionPhase::Cancelled
        | DesktopActionPhase::Unknown => false,
    };
    if valid {
        Ok(())
    } else {
        Err(Error("invalid desktop action state transition".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> DesktopSessionBinding {
        DesktopSessionBinding {
            schema_version: DESKTOP_SESSION_SCHEMA_VERSION,
            helper_ref: "helper:desktop".into(),
            grant_ref: "grant:one".into(),
            owner_ref: VerifiedPrincipal("owner:one".into()),
            os_user_ref: "os-user:one".into(),
            os_session_ref: "os-session:one".into(),
            session_generation: 7,
            platform: DesktopPlatform::Windows,
            desktop_ref: "desktop:primary".into(),
            action_ref: ActionId("action:one".into()),
            plan_digest: PlanDigest(
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            ),
            authority_epoch: AuthorityEpoch(3),
            nonce: Nonce("nonce:one".into()),
            capabilities: [DesktopCapability::PointerClick].into_iter().collect(),
            issued_at: 100,
            expires_at: 1_000,
        }
    }

    fn observation() -> DesktopSessionObservation {
        DesktopSessionObservation {
            schema_version: DESKTOP_SESSION_SCHEMA_VERSION,
            helper_ref: "helper:desktop".into(),
            grant_ref: Some("grant:one".into()),
            state: DesktopSessionState::Ready,
            liveness: DesktopSessionLiveness::Live,
            os_user_ref: Some("os-user:one".into()),
            os_session_ref: Some("os-session:one".into()),
            session_generation: Some(7),
            platform: DesktopPlatform::Windows,
            desktop_ref: Some("desktop:primary".into()),
            peer_ref: Some("peer:desktop".into()),
            receipt_ref: None,
            reason_ref: None,
            observed_at: 200,
        }
    }

    fn request() -> DesktopActionRequest {
        DesktopActionRequest {
            schema_version: DESKTOP_SESSION_SCHEMA_VERSION,
            job_id: "job:one".into(),
            permit_ref: "permit:one".into(),
            binding: binding(),
            normalized_action_digest: SchemaDigest(
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            ),
            capability: DesktopCapability::PointerClick,
            deadline: 900,
        }
    }

    #[test]
    fn valid_live_binding_admits_one_bounded_action() {
        assert!(binding().validate_at(200).is_ok());
        assert!(request().validate_at(&observation(), 200).is_ok());
    }

    #[test]
    fn stale_or_changed_session_cannot_admit_action() {
        let mut changed_session = observation();
        changed_session.session_generation = Some(8);
        assert!(request().validate_at(&changed_session, 200).is_err());

        let mut stale = observation();
        stale.liveness = DesktopSessionLiveness::Stale;
        assert!(request().validate_at(&stale, 200).is_err());
    }

    #[test]
    fn grant_expiry_and_capability_scope_block_action() {
        assert!(request().validate_at(&observation(), 1_000).is_err());

        let mut request = request();
        request.capability = DesktopCapability::Screenshot;
        assert!(request.validate_at(&observation(), 200).is_err());
    }

    #[test]
    fn unknown_is_only_reachable_after_started_and_is_terminal() {
        assert!(validate_desktop_action_transition(
            DesktopActionPhase::Started,
            DesktopActionPhase::Unknown
        )
        .is_ok());
        assert!(validate_desktop_action_transition(
            DesktopActionPhase::Unknown,
            DesktopActionPhase::Started
        )
        .is_err());
        assert!(validate_desktop_action_transition(
            DesktopActionPhase::Pending,
            DesktopActionPhase::Unknown
        )
        .is_err());
    }

    #[test]
    fn receipt_must_match_the_original_request_binding() {
        let request = request();
        let receipt = DesktopActionReceipt {
            schema_version: DESKTOP_SESSION_SCHEMA_VERSION,
            job_id: "job:one".into(),
            permit_ref: "permit:one".into(),
            action_ref: ActionId("action:one".into()),
            plan_digest: request.binding.plan_digest.clone(),
            nonce: request.binding.nonce.clone(),
            normalized_action_digest: request.normalized_action_digest.clone(),
            outcome: DesktopActionOutcome::Unknown,
            result_ref: None,
            observed_at: 300,
        };
        assert!(receipt.validate_for(&request).is_ok());

        let mut changed = receipt.clone();
        changed.nonce = Nonce("nonce:replayed".into());
        assert!(changed.validate_for(&request).is_err());

        let mut changed = receipt;
        changed.permit_ref = "permit:other".into();
        assert!(changed.validate_for(&request).is_err());
    }
}
