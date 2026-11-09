//! Finite M5 adapters; browser payloads never supply an approval principal.
use super::super::WebError;
use forme_protocol as p;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeQuery {
    scope: String,
}

pub(super) fn path(method: &str, path: &str, query: Option<&str>) -> Result<String, WebError> {
    if method == "GET" && path == "ecosystem/snapshot" {
        let uri: axum::http::Uri = format!("/?{}", query.ok_or_else(WebError::invalid)?)
            .parse()
            .map_err(|_| WebError::invalid())?;
        let query = axum::extract::Query::<ScopeQuery>::try_from_uri(&uri)
            .map_err(|_| WebError::invalid())?
            .0;
        p::validate_product_id(&query.scope).map_err(|_| WebError::invalid())?;
        return Ok(format!("/v1/ecosystem/snapshot?scope={}", query.scope));
    }
    if method != "POST"
        || query.is_some()
        || !matches!(
            path,
            "ecosystem/publishers"
                | "ecosystem/admissions"
                | "ecosystem/plans"
                | "ecosystem/changes"
                | "ecosystem/distributions/prepare"
        )
    {
        return Err(WebError::invalid());
    }
    Ok(format!("/v1/{path}"))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Publisher {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    grant: p::CapabilityPublisherGrant,
    #[serde(default)]
    previous: Option<p::CapabilityPublisherGrantRef>,
    expected: p::EcosystemAggregateVersion,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    catalog_run: p::RunId,
    package: p::SignedCapabilityPackage,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema_version: p::SchemaVersion,
    operation: p::CapabilityPackageOperation,
    package: p::CapabilityPackageRef,
    release: p::CapabilityReleaseRef,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    plan: p::CapabilityInstallPlan,
    approval: Approval,
    reason: p::ReasonRef,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    schema_version: p::SchemaVersion,
    approval: p::ApprovalId,
    plan_digest: p::PlanDigest,
    nonce: p::Nonce,
    expires_at: p::Timestamp,
}
#[derive(Serialize)]
struct BoundChange {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    plan: p::CapabilityInstallPlan,
    approval: p::CapabilityPackageApproval,
    reason: p::ReasonRef,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Distribution {
    schema_version: p::SchemaVersion,
    plan: p::CapabilityInstallPlan,
    target_peer: p::FederatedPeerRef,
    authority_epoch: p::AuthorityEpoch,
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, WebError> {
    serde_json::from_slice(bytes).map_err(|_| WebError::invalid())
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, WebError> {
    serde_json::to_vec(value).map_err(|_| WebError::invalid())
}
fn version(version: p::SchemaVersion) -> Result<(), WebError> {
    if version != p::M5_SCHEMA_VERSION {
        return Err(WebError::invalid());
    }
    Ok(())
}
fn run_key(run: &p::RunId, key: Option<&str>) -> Result<(), WebError> {
    p::validate_product_id(&run.0).map_err(|_| WebError::invalid())?;
    if key != Some(run.0.as_str()) {
        return Err(WebError::invalid());
    }
    Ok(())
}

pub(super) fn body(
    method: &str,
    path: &str,
    bytes: &[u8],
    owner: &p::WebSessionView,
    key: Option<&str>,
) -> Result<Vec<u8>, WebError> {
    if method == "GET" {
        return if path == "ecosystem/snapshot" && bytes.is_empty() {
            Ok(Vec::new())
        } else {
            Err(WebError::invalid())
        };
    }
    if method != "POST" {
        return Err(WebError::invalid());
    }
    match path {
        "ecosystem/publishers" => {
            let value: Publisher = decode(bytes)?;
            version(value.schema_version)?;
            version(value.grant.schema_version)?;
            run_key(&value.run, key)?;
            value.grant.validate().map_err(|_| WebError::invalid())?;
            version(value.expected.schema_version)?;
            value.expected.validate().map_err(|_| WebError::invalid())?;
            encode(&value)
        }
        "ecosystem/admissions" => {
            let value: Admission = decode(bytes)?;
            version(value.schema_version)?;
            version(value.package.schema_version)?;
            run_key(&value.run, key)?;
            p::validate_product_id(&value.catalog_run.0).map_err(|_| WebError::invalid())?;
            value.package.validate().map_err(|_| WebError::invalid())?;
            encode(&value)
        }
        "ecosystem/plans" => {
            let value: Plan = decode(bytes)?;
            version(value.schema_version)?;
            p::validate_product_id(&value.package.0).map_err(|_| WebError::invalid())?;
            p::validate_product_id(&value.release.0).map_err(|_| WebError::invalid())?;
            encode(&value)
        }
        "ecosystem/changes" => {
            let value: Change = decode(bytes)?;
            version(value.schema_version)?;
            version(value.plan.schema_version)?;
            version(value.approval.schema_version)?;
            value.plan.validate().map_err(|_| WebError::invalid())?;
            if value.plan.digest != value.approval.plan_digest
                || key != Some(value.approval.nonce.0.as_str())
            {
                return Err(WebError::invalid());
            }
            let approval = p::CapabilityPackageApproval {
                schema_version: value.approval.schema_version,
                approval: value.approval.approval,
                plan_digest: value.approval.plan_digest,
                principal: p::VerifiedPrincipal(owner.owner.0.clone()),
                nonce: value.approval.nonce,
                expires_at: value.approval.expires_at,
            };
            approval.validate().map_err(|_| WebError::invalid())?;
            encode(&BoundChange {
                schema_version: value.schema_version,
                run: value.run,
                plan: value.plan,
                approval,
                reason: value.reason,
            })
        }
        "ecosystem/distributions/prepare" => {
            let value: Distribution = decode(bytes)?;
            version(value.schema_version)?;
            version(value.plan.schema_version)?;
            value.plan.validate().map_err(|_| WebError::invalid())?;
            p::validate_product_id(&value.target_peer.0).map_err(|_| WebError::invalid())?;
            if value.authority_epoch.0 == 0 {
                return Err(WebError::invalid());
            }
            encode(&value)
        }
        _ => Err(WebError::invalid()),
    }
}

#[cfg(test)]
#[path = "ecosystem_tests.rs"]
mod tests;
