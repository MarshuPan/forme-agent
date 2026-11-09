//! Browser controls carry a binding, never an authenticated principal.
use forme_protocol as p;
use serde::Deserialize;

use super::super::WebError;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum WebRunControl {
    Cancel(p::CancelRequest),
    ResolveApproval(WebApproval),
    ResolveToolOutcome {
        command: p::IdempotencyKey,
        resolution: p::ToolOutcomeResolution,
    },
    ResolveHandoff {
        command: p::IdempotencyKey,
        resolution: p::HandoffResolutionRequest,
    },
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
    if path.starts_with("ecosystem/") {
        return super::ecosystem::body(method, path, body, owner, key);
    }
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
    if path == "projects/supervision" {
        let command: p::ProjectSupervisionCommand =
            serde_json::from_slice(body).map_err(|_| WebError::invalid())?;
        command.validate().map_err(|_| WebError::invalid())?;
        if Some(command.command.0.as_str()) != key {
            return Err(WebError::invalid());
        }
        return serde_json::to_vec(&command).map_err(|_| WebError::invalid());
    }
    if path == "workbench/query" {
        let query: p::WorkbenchQuery =
            serde_json::from_slice(body).map_err(|_| WebError::invalid())?;
        query.validate().map_err(|_| WebError::invalid())?;
        return serde_json::to_vec(&query).map_err(|_| WebError::invalid());
    }
    if matches!(path, "workbench/material" | "workbench/material-lifecycle") {
        let query: p::WorkbenchMaterialQuery =
            serde_json::from_slice(body).map_err(|_| WebError::invalid())?;
        query.validate().map_err(|_| WebError::invalid())?;
        return serde_json::to_vec(&query).map_err(|_| WebError::invalid());
    }
    if path == "workbench/corrections/query" {
        let query: p::WorkbenchCorrectionQuery =
            serde_json::from_slice(body).map_err(|_| WebError::invalid())?;
        query.validate().map_err(|_| WebError::invalid())?;
        return serde_json::to_vec(&query).map_err(|_| WebError::invalid());
    }
    if path == "workbench/correction" {
        let command: p::WorkbenchCorrectionCommand =
            serde_json::from_slice(body).map_err(|_| WebError::invalid())?;
        command.validate().map_err(|_| WebError::invalid())?;
        if Some(command.command.0.as_str()) != key {
            return Err(WebError::invalid());
        }
        return serde_json::to_vec(&command).map_err(|_| WebError::invalid());
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
    if path == "workbench/proposal" {
        let command: p::WorkbenchProposalCommand =
            serde_json::from_slice(body).map_err(|_| WebError::invalid())?;
        command.validate().map_err(|_| WebError::invalid())?;
        if Some(command.command.0.as_str()) != key {
            return Err(WebError::invalid());
        }
        return serde_json::to_vec(&command).map_err(|_| WebError::invalid());
    }
    if path == "artifacts/query" {
        let query: p::ArtifactCatalogQuery =
            serde_json::from_slice(body).map_err(|_| WebError::invalid())?;
        query.validate().map_err(|_| WebError::invalid())?;
        return serde_json::to_vec(&query).map_err(|_| WebError::invalid());
    }
    if path == "artifacts/upload" {
        let command: p::ArtifactUploadCommand =
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
            WebRunControl::ResolveToolOutcome {
                command,
                resolution,
            } => {
                if Some(command.0.as_str()) != key {
                    return Err(WebError::invalid());
                }
                p::RunControl::ResolveToolOutcome {
                    command,
                    resolution,
                }
            }
            WebRunControl::ResolveHandoff {
                command,
                resolution,
            } => {
                if Some(command.0.as_str()) != key {
                    return Err(WebError::invalid());
                }
                p::RunControl::ResolveHandoff {
                    command,
                    resolution,
                }
            }
        };
        control.validate().map_err(|_| WebError::invalid())?;
        return serde_json::to_vec(&control).map_err(|_| WebError::invalid());
    }
    Ok(body.to_owned())
}
