//! A single approved body to one installation-owned local recipient.
use crate::*;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

pub const MANAGED_CHANNEL_PROFILE: &str = "windows-appcontainer-channel-delivery-v1";
pub const MANAGED_CHANNEL_MAX_BYTES: u64 = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductChannelDeliverySource {
    pub schema_version: SchemaVersion,
    pub endpoint: String,
    pub schema_digest: SchemaDigest,
    pub credential_ref: String,
    pub credential_digest: SchemaDigest,
    pub receipt_public_key: [u8; 32],
    pub capability_ref: CapabilityRef,
    pub permission: PermissionRef,
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub timeout: DurationMs,
}
impl ProductChannelDeliverySource {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || !(1..=MANAGED_CHANNEL_MAX_BYTES).contains(&self.max_input_bytes)
            || !(1..=MANAGED_CHANNEL_MAX_BYTES).contains(&self.max_output_bytes)
            || !(1..=60_000).contains(&self.timeout.0)
        {
            return Err(invalid());
        }
        channel_delivery_endpoint(&self.endpoint)?;
        for reference in [
            &self.credential_ref,
            &self.capability_ref.0,
            &self.permission.0,
        ] {
            validate_product_id(reference)?;
        }
        sha256_digest_bytes(&self.schema_digest)?;
        sha256_digest_bytes(&self.credential_digest)?;
        Ok(())
    }
}

/// Parsing does not resolve a host or consult environment proxy settings.
pub fn channel_delivery_endpoint(endpoint: &str) -> Result<(SocketAddr, &str)> {
    let (authority, path) = endpoint
        .strip_prefix("http://")
        .and_then(|s| s.split_once('/'))
        .ok_or_else(invalid)?;
    let address: SocketAddr = authority.parse().map_err(|_| invalid())?;
    if !matches!(address.ip(), std::net::IpAddr::V4(ip) if ip == std::net::Ipv4Addr::LOCALHOST)
        && !matches!(address.ip(), std::net::IpAddr::V6(ip) if ip == std::net::Ipv6Addr::LOCALHOST)
    {
        return Err(invalid());
    }
    if address.port() == 0
        || address.to_string() != authority
        || endpoint.len() > 2048
        || path
            .bytes()
            .any(|b| !b.is_ascii_graphic() || b"?#%\\@".contains(&b))
        || path.split('/').any(|s| matches!(s, "." | ".."))
    {
        return Err(invalid());
    }
    Ok((address, &endpoint[("http://".len() + authority.len())..]))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedChannelDeliveryGrantV1 {
    pub schema_version: SchemaVersion,
    pub grant_ref: String,
    pub installation_ref: String,
    pub owner: VerifiedPrincipal,
    pub channel_ref: String,
    pub connector_ref: String,
    pub source_digest: SchemaDigest,
    pub participant: ParticipantId,
    pub scope: Scope,
    pub job_id: String,
    pub action_ref: ActionId,
    pub plan_digest: PlanDigest,
    pub normalized_action_digest: SchemaDigest,
    pub permit_digest: SchemaDigest,
    pub authority_epoch: AuthorityEpoch,
    pub nonce: Nonce,
    pub endpoint: String,
    pub method: String,
    pub content_digest: SchemaDigest,
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub expires_at: Timestamp,
}
impl ManagedChannelDeliveryGrantV1 {
    pub fn reference(&self) -> Result<String> {
        let mut material = self.clone();
        material.grant_ref.clear();
        Ok(format!(
            "channel-delivery-grant:{}",
            &canonical_digest(&("forme.managed-channel.delivery-grant.v1", material))?.0[7..]
        ))
    }
    /// The Host calls this independently with its current protected source.
    pub fn from_request(
        installation_ref: &str,
        source: &ProductChannelSource,
        request: &WorkerDispatchRequest,
        content: &[u8],
        now: Timestamp,
    ) -> Result<Self> {
        validate_product_id(installation_ref)?;
        source.validate()?;
        request.validate_at(now)?;
        let delivery = source.delivery.as_ref().ok_or_else(invalid)?;
        let execution = request.execution.as_ref().ok_or_else(invalid)?;
        let intent = &execution.intent;
        let ActionParameters::AppApi(spec) = &intent.parameters else {
            return Err(invalid());
        };
        let AppApiOperation::Mutation {
            method: AppApiMutationMethod::Post,
            body: Some(ExternalInput::Content(reference)),
            idempotency_key,
        } = &spec.operation
        else {
            return Err(invalid());
        };
        let content_digest = sha256_content_digest(content);
        if request.job.sandbox_profile_ref != MANAGED_CHANNEL_PROFILE
            || execution.desktop_session.is_some()
            || execution.backend != BackendKind::AppApi
            || execution.scope != source.scope
            || execution.timeout.0 > delivery.timeout.0
            || execution.output_budget_bytes > delivery.max_output_bytes
            || request
                .job
                .deadline
                .saturating_sub(request.permit.issued_at)
                > delivery.timeout.0 as i64
            || intent.source != Source::Communication
            || intent.action_type != ActionType::Deliver
            || intent.expected_effect != ExpectedEffect::Outward
            || intent.capability_ref != delivery.capability_ref
            || intent.requested_permissions != [delivery.permission.clone()]
            || source.participant.0 == request.job.owner_ref.0
            || spec.schema_version != SchemaVersion(1)
            || spec.connector.0 != source.connector_ref
            || spec.endpoint != delivery.endpoint
            || spec.schema_digest != delivery.schema_digest
            || spec.credential.as_ref().map(|v| &v.0) != Some(&delivery.credential_ref)
            || spec.participant.as_ref() != Some(&source.participant)
            || spec.representation.is_none()
            || spec
                .disclosure_request
                .as_ref()
                .is_none_or(|v| v.0.trim().is_empty())
            || spec.timeout.0 > execution.timeout.0
            || reference.0.is_empty()
            || content.is_empty()
            || content.len() as u64 > delivery.max_input_bytes
            || !idempotency_key.0.starts_with("communication:")
            || !idempotency_key
                .0
                .ends_with(&format!(":{}", content_digest.0))
        {
            return Err(invalid());
        }
        let mut value = Self {
            schema_version: SchemaVersion(1),
            grant_ref: String::new(),
            installation_ref: installation_ref.into(),
            owner: request.job.owner_ref.clone(),
            channel_ref: source.channel_ref.clone(),
            connector_ref: source.connector_ref.clone(),
            source_digest: canonical_digest(source)?,
            participant: source.participant.clone(),
            scope: source.scope.clone(),
            job_id: request.job.job_id.clone(),
            action_ref: request.job.action_ref.clone(),
            plan_digest: request.job.plan_digest.clone(),
            normalized_action_digest: execution.normalized_action_digest.clone(),
            permit_digest: canonical_digest(&request.permit)?,
            authority_epoch: request.job.authority_epoch,
            nonce: request.job.nonce.clone(),
            endpoint: delivery.endpoint.clone(),
            method: "POST".into(),
            content_digest,
            max_input_bytes: delivery.max_input_bytes,
            max_output_bytes: delivery.max_output_bytes.min(execution.output_budget_bytes),
            expires_at: request.job.deadline.min(request.permit.deadline),
        };
        value.grant_ref = value.reference()?;
        Ok(value)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedChannelDeliveryInputV1 {
    pub schema_version: SchemaVersion,
    pub execution: WorkerDispatchRequest,
    pub grant_ref: String,
    pub content_bytes: Vec<u8>,
}
impl ManagedChannelDeliveryInputV1 {
    pub fn validate_for(
        &self,
        installation: &str,
        source: &ProductChannelSource,
        now: Timestamp,
    ) -> Result<ManagedChannelDeliveryGrantV1> {
        if self.schema_version != SchemaVersion(1)
            || self.content_bytes.len() as u64 > MANAGED_CHANNEL_MAX_BYTES
        {
            return Err(invalid());
        }
        let expected = ManagedChannelDeliveryGrantV1::from_request(
            installation,
            source,
            &self.execution,
            &self.content_bytes,
            now,
        )?;
        if expected.grant_ref != self.grant_ref {
            return Err(invalid());
        }
        Ok(expected)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedChannelDeliveryReceiptV1 {
    pub schema_version: SchemaVersion,
    pub grant_ref: String,
    pub job_id: String,
    pub nonce: Nonce,
    pub plan_digest: PlanDigest,
    pub content_digest: SchemaDigest,
    pub participant: ParticipantId,
    pub accepted: bool,
    pub signature: Vec<u8>,
}
impl ManagedChannelDeliveryReceiptV1 {
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        #[derive(Serialize)]
        struct Material<'a> {
            schema_version: SchemaVersion,
            grant_ref: &'a str,
            job_id: &'a str,
            nonce: &'a Nonce,
            plan_digest: &'a PlanDigest,
            content_digest: &'a SchemaDigest,
            participant: &'a ParticipantId,
            accepted: bool,
        }
        serde_json::to_vec(&(
            "forme.managed-channel.delivery-receipt.v1",
            Material {
                schema_version: self.schema_version,
                grant_ref: &self.grant_ref,
                job_id: &self.job_id,
                nonce: &self.nonce,
                plan_digest: &self.plan_digest,
                content_digest: &self.content_digest,
                participant: &self.participant,
                accepted: self.accepted,
            },
        ))
        .map_err(|_| invalid())
    }
    pub fn validate_binding(&self, grant: &ManagedChannelDeliveryGrantV1) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.grant_ref != grant.grant_ref
            || self.job_id != grant.job_id
            || self.nonce != grant.nonce
            || self.plan_digest != grant.plan_digest
            || self.content_digest != grant.content_digest
            || self.participant != grant.participant
            || !self.accepted
            || self.signature.len() != 64
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn invalid() -> Error {
    Error("managed channel delivery is invalid or outside its exact authorization".into())
}

/// Private bounded stdio message. It conveys no address, body or credential.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedChannelWorkerRequestV1 {
    pub schema_version: SchemaVersion,
    pub grant_ref: String,
}
impl ManagedChannelWorkerRequestV1 {
    pub fn validate(&self) -> Result<()> {
        let suffix = self
            .grant_ref
            .strip_prefix("channel-delivery-grant:")
            .ok_or_else(invalid)?;
        if self.schema_version != SchemaVersion(1)
            || suffix.len() != 64
            || !suffix
                .bytes()
                .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
        {
            return Err(invalid());
        }
        Ok(())
    }
}
