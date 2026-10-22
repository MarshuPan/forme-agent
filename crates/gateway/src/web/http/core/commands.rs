//! Browser controls carry a binding, never an authenticated principal.
use forme_protocol as p;
use serde::Deserialize;

use super::super::WebError;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum WebRunControl {
    Cancel(p::CancelRequest),
    ResolveApproval(WebApproval),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebApproval {
    schema_version: p::SchemaVersion,
    approval_id: p::ApprovalId,
    outcome: p::ApprovalOutcome,
    bound_plan_digest: p::PlanDigest,
    normalized_action_digest: p::SchemaDigest,
    affected_resources: Vec<p::ResourceRef>,
    policy_version: p::Version,
    tool_schema_version: p::Version,
    nonce: p::Nonce,
    use_by: p::Timestamp,
}

pub(super) fn bind_body(
    method: &str,
    path: &str,
    body: &[u8],
    owner: &p::WebSessionView,
    key: Option<&str>,
) -> Result<Vec<u8>, WebError> {
    if method == "GET" {
        return if body.is_empty() {
            Ok(Vec::new())
        } else {
            Err(WebError::invalid())
        };
    }
    if path == "runs" {
        let request: p::RunRequest =
            serde_json::from_slice(body).map_err(|_| WebError::invalid())?;
        if request.source != p::Source::UserTurn
            || request.idempotency_key.as_ref().map(|v| v.0.as_str()) != key
        {
            return Err(WebError::invalid());
        }
        return serde_json::to_vec(&request).map_err(|_| WebError::invalid());
    }
    let segments: Vec<_> = path.split('/').collect();
    if path == "workbench/query" {
        let query: p::WorkbenchQuery =
            serde_json::from_slice(body).map_err(|_| WebError::invalid())?;
        query.validate().map_err(|_| WebError::invalid())?;
        return serde_json::to_vec(&query).map_err(|_| WebError::invalid());
    }
    if path == "workbench/review" {
        let command: p::WorkbenchReviewCommand =
            serde_json::from_slice(body).map_err(|_| WebError::invalid())?;
        command.validate().map_err(|_| WebError::invalid())?;
        if Some(command.command.0.as_str()) != key {
            return Err(WebError::invalid());
        }
        return serde_json::to_vec(&command).map_err(|_| WebError::invalid());
    }
    if matches!(segments.as_slice(), ["runs", _, "control"]) {
        let control: WebRunControl =
            serde_json::from_slice(body).map_err(|_| WebError::invalid())?;
        let control = match control {
            WebRunControl::Cancel(cancel) => {
                cancel.validate().map_err(|_| WebError::invalid())?;
                p::RunControl::Cancel(cancel)
            }
            WebRunControl::ResolveApproval(approval) => {
                if Some(approval.nonce.0.as_str()) != key {
                    return Err(WebError::invalid());
                }
                let decision = p::ApprovalDecision {
                    schema_version: approval.schema_version,
                    approval_id: approval.approval_id,
                    outcome: approval.outcome,
                    approver: p::VerifiedPrincipal(owner.owner.0.clone()),
                    bound_plan_digest: approval.bound_plan_digest,
                    normalized_action_digest: approval.normalized_action_digest,
                    affected_resources: approval.affected_resources,
                    policy_version: approval.policy_version,
                    tool_schema_version: approval.tool_schema_version,
                    nonce: approval.nonce,
                    use_by: approval.use_by,
                };
                decision.validate().map_err(|_| WebError::invalid())?;
                p::RunControl::ResolveApproval(decision)
            }
        };
        return serde_json::to_vec(&control).map_err(|_| WebError::invalid());
    }
    Ok(body.to_owned())
}
