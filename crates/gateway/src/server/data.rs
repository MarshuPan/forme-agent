//! HTTP data responsibilities; no independent runtime or writer.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DataTargetRequest {
    schema_version: p::SchemaVersion,
    target: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DataSourceRequest {
    schema_version: p::SchemaVersion,
    source: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DataRestorePlanRequest {
    schema_version: p::SchemaVersion,
    source: String,
    target: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DataApplyRequest {
    schema_version: p::SchemaVersion,
    plan: forme_harness::DataOperationPlan,
    confirmation: forme_harness::DataOperationConfirmation,
}
#[derive(Deserialize)]
#[serde(tag = "action", deny_unknown_fields)]
pub(super) enum DataLifecycleCommand {
    Prepare {
        schema_version: p::SchemaVersion,
        scope: p::Scope,
        subject_digest: p::SchemaDigest,
        operation: p::DataLifecycleOperation,
        expected_version: u64,
    },
    Apply {
        schema_version: p::SchemaVersion,
        plan: Box<forme_harness::DataOperationPlan>,
        confirmation: forme_harness::DataOperationConfirmation,
    },
}
pub(super) fn data_schema(version: p::SchemaVersion) -> Result<(), ApiError> {
    if version.0 != 1 {
        Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported data control schema",
        ))
    } else {
        Ok(())
    }
}
pub(super) async fn data_backup(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(r): Json<DataTargetRequest>,
) -> Result<Json<forme_harness::BackupManifest>, ApiError> {
    state.authorize(&headers, true)?;
    data_schema(r.schema_version)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    Ok(Json(
        state
            .blocking(move || runtime.create_data_backup(&owner, &r.target))
            .await?,
    ))
}
pub(super) async fn data_verify_backup(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(r): Json<DataSourceRequest>,
) -> Result<Json<forme_harness::BackupVerification>, ApiError> {
    state.authorize(&headers, true)?;
    data_schema(r.schema_version)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    Ok(Json(
        state
            .blocking(move || runtime.verify_data_backup(&owner, &r.source))
            .await?,
    ))
}
pub(super) async fn data_restore_plan(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(r): Json<DataRestorePlanRequest>,
) -> Result<Json<forme_harness::DataOperationPlan>, ApiError> {
    state.authorize(&headers, true)?;
    data_schema(r.schema_version)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    Ok(Json(
        state
            .blocking(move || runtime.prepare_data_restore(&owner, r.source, r.target))
            .await?,
    ))
}
pub(super) async fn data_restore(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(r): Json<DataApplyRequest>,
) -> Result<Json<forme_harness::DataOperationResult>, ApiError> {
    state.authorize(&headers, true)?;
    data_schema(r.schema_version)?;
    if !matches!(
        r.plan.operation,
        forme_harness::DataPlannedOperation::Restore { .. }
    ) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "restore route requires a restore plan",
        ));
    }
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    Ok(Json(
        state
            .blocking(move || runtime.apply_data_operation(&owner, r.plan, r.confirmation))
            .await?,
    ))
}
pub(super) async fn data_lifecycle(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(command): Json<DataLifecycleCommand>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.authorize(&headers, true)?;
    let runtime = state.runtime.clone();
    let owner = state.owner.clone();
    let value = state
        .blocking(move || match command {
            DataLifecycleCommand::Prepare {
                schema_version,
                scope,
                subject_digest,
                operation,
                expected_version,
            } => {
                if schema_version.0 != 1 {
                    return Err(p::Error("unsupported lifecycle schema".into()));
                }
                serde_json::to_value(runtime.prepare_data_lifecycle(
                    &owner,
                    scope,
                    subject_digest,
                    operation,
                    expected_version,
                )?)
                .map_err(|_| p::Error("lifecycle plan response failed".into()))
            }
            DataLifecycleCommand::Apply {
                schema_version,
                plan,
                confirmation,
            } => {
                if schema_version.0 != 1
                    || !matches!(
                        plan.operation,
                        forme_harness::DataPlannedOperation::Lifecycle { .. }
                    )
                {
                    return Err(p::Error("lifecycle route requires a lifecycle plan".into()));
                }
                serde_json::to_value(runtime.apply_data_operation(&owner, *plan, confirmation)?)
                    .map_err(|_| p::Error("lifecycle response failed".into()))
            }
        })
        .await?;
    Ok(Json(value))
}
