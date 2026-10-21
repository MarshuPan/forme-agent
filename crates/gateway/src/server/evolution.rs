//! HTTP evolution responsibilities; no independent runtime or writer.
use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct EvolutionSnapshotQuery {
    scope: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct EcosystemSnapshotQuery {
    scope: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProvisionCapabilityPublisherRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    grant: p::CapabilityPublisherGrant,
    #[serde(default)]
    previous: Option<p::CapabilityPublisherGrantRef>,
    expected: p::EcosystemAggregateVersion,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdmitCapabilityPackageRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    catalog_run: p::RunId,
    package: p::SignedCapabilityPackage,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrepareCapabilityChangeRequest {
    schema_version: p::SchemaVersion,
    operation: p::CapabilityPackageOperation,
    package: p::CapabilityPackageRef,
    release: p::CapabilityReleaseRef,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ApplyCapabilityChangeRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    plan: p::CapabilityInstallPlan,
    approval: p::CapabilityPackageApproval,
    reason: p::ReasonRef,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrepareCapabilityDistributionRequest {
    schema_version: p::SchemaVersion,
    plan: p::CapabilityInstallPlan,
    target_peer: p::FederatedPeerRef,
    authority_epoch: p::AuthorityEpoch,
}

#[derive(Debug, Serialize)]
pub(super) struct AutoActivationStatus {
    schema_version: p::SchemaVersion,
    paused: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct SetAutoActivationStatusRequest {
    schema_version: p::SchemaVersion,
    paused: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RecordStrategyCandidateRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    candidate: p::StrategyCandidate,
}

#[derive(Debug, Deserialize)]
pub(super) struct EvaluateStrategyRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    candidate: p::StrategyCandidate,
    comparison: p::EvolutionComparison,
}

#[derive(Debug, Serialize)]
pub(super) struct EvaluateStrategyResponse {
    schema_version: p::SchemaVersion,
    evaluation: p::EvolutionEvaluation,
    event_id: p::EventId,
}

#[derive(Debug, Deserialize)]
pub(super) struct PromoteStrategyRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    candidate: p::StrategyCandidate,
    evaluation: p::EvolutionEvaluation,
}

#[derive(Debug, Deserialize)]
pub(super) struct ActivateStrategyRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    aggregate: p::EvolutionAggregateRef,
    candidate: p::StrategyCandidate,
    evaluation: p::EvolutionEvaluation,
    promotion: p::EventId,
}

#[derive(Debug, Deserialize)]
pub(super) struct RollbackStrategyRequest {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    aggregate: p::EvolutionAggregateRef,
    domain: p::StrategyDomain,
    scope: p::Scope,
    restored: p::StrategyVersionRef,
    triggers: Vec<p::EvidenceRef>,
    in_flight: p::InFlightDisposition,
}

#[derive(Debug, Serialize)]
pub(super) struct EvolutionActivationResponse {
    schema_version: p::SchemaVersion,
    append: p::ExpectedAppend,
    snapshot: p::EvolutionSnapshot,
}

pub(super) async fn evolution_snapshot(
    State(state): State<Arc<HttpState>>,
    Query(query): Query<EvolutionSnapshotQuery>,
    headers: HeaderMap,
) -> Result<Json<p::EvolutionSnapshot>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    if query.scope.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "evolution snapshot scope is required",
        ));
    }
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.evolution_snapshot(p::Scope(query.scope), auth))
        .await
        .map(Json)
}

pub(super) async fn ecosystem_snapshot(
    State(state): State<Arc<HttpState>>,
    Query(query): Query<EcosystemSnapshotQuery>,
    headers: HeaderMap,
) -> Result<Json<p::CapabilityEcosystemSnapshot>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    if query.scope.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "ecosystem snapshot scope is required",
        ));
    }
    let gateway = state.gateway.clone();
    state
        .blocking(move || gateway.ecosystem_snapshot(p::Scope(query.scope), auth))
        .await
        .map(Json)
}

pub(super) async fn provision_capability_publisher(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<ProvisionCapabilityPublisherRequest>,
) -> Result<(StatusCode, Json<p::ExpectedAppend>), ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let result = state
        .blocking(move || {
            gateway.provision_capability_publisher(
                request.run,
                request.grant,
                request.previous,
                request.expected,
                auth,
            )
        })
        .await?;
    Ok((StatusCode::CREATED, Json(result)))
}

pub(super) async fn admit_capability_package(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<AdmitCapabilityPackageRequest>,
) -> Result<(StatusCode, Json<p::CapabilityPackageAdmission>), ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let admission = state
        .blocking(move || {
            gateway.admit_capability_package(
                request.run,
                request.catalog_run,
                request.package,
                auth,
            )
        })
        .await?;
    Ok((StatusCode::CREATED, Json(admission)))
}

pub(super) async fn prepare_capability_change(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<PrepareCapabilityChangeRequest>,
) -> Result<Json<p::CapabilityInstallPlan>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || {
            gateway.prepare_capability_change(
                request.operation,
                request.package,
                request.release,
                auth,
            )
        })
        .await
        .map(Json)
}

pub(super) async fn apply_capability_change(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<ApplyCapabilityChangeRequest>,
) -> Result<Json<p::CapabilityPackageState>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || {
            gateway.apply_capability_change(
                request.run,
                request.plan,
                request.approval,
                request.reason,
                auth,
            )
        })
        .await
        .map(Json)
}

pub(super) async fn prepare_capability_distribution(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<PrepareCapabilityDistributionRequest>,
) -> Result<Json<p::CapabilityPackageDistributionEnvelope>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    state
        .blocking(move || {
            gateway.prepare_capability_distribution(
                request.plan,
                request.target_peer,
                request.authority_epoch,
                auth,
            )
        })
        .await
        .map(Json)
}

pub(super) async fn auto_activation_status(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<AutoActivationStatus>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    let paused = state
        .blocking(move || gateway.auto_activation_paused(auth))
        .await?;
    Ok(Json(AutoActivationStatus {
        schema_version: p::SchemaVersion(1),
        paused,
    }))
}

pub(super) async fn set_auto_activation_status(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<SetAutoActivationStatusRequest>,
) -> Result<Json<AutoActivationStatus>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let paused = request.paused;
    state
        .blocking(move || gateway.set_auto_activation_paused(paused, auth))
        .await?;
    Ok(Json(AutoActivationStatus {
        schema_version: p::SchemaVersion(1),
        paused,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InitializeStrategySeedRequest {
    schema_version: p::SchemaVersion,
    domain: p::StrategyDomain,
    workspace: p::WorkspaceRef,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RegisterStrategySpecRequest {
    schema_version: p::SchemaVersion,
    spec: forme_harness::ProductionStrategySpec,
}

pub(super) async fn initialize_strategy_seed(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<InitializeStrategySeedRequest>,
) -> Result<Json<forme_harness::ProductionStrategySpec>, ApiError> {
    require_schema(request.schema_version)?;
    state.authorize(&headers, true)?;
    let owner = state.owner.clone();
    let runtime = state.runtime.clone();
    let body = state
        .blocking(move || {
            runtime.initialize_strategy_seed(&owner, request.domain, &request.workspace)
        })
        .await?;
    Ok(Json(body))
}

pub(super) async fn register_strategy_spec(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<RegisterStrategySpecRequest>,
) -> Result<Json<forme_harness::ProductionStrategySpec>, ApiError> {
    require_schema(request.schema_version)?;
    state.authorize(&headers, true)?;
    let owner = state.owner.clone();
    let runtime = state.runtime.clone();
    let body = state
        .blocking(move || runtime.register_strategy_spec(&owner, request.spec))
        .await?;
    Ok(Json(body))
}

pub(super) async fn record_strategy_candidate(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(mut request): Json<RecordStrategyCandidateRequest>,
) -> Result<(StatusCode, Json<p::EventId>), ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    stamp_owner_candidate(&mut request.candidate);
    let gateway = state.gateway.clone();
    let event = state
        .blocking(move || gateway.record_strategy_candidate(request.run, request.candidate, auth))
        .await?;
    Ok((StatusCode::CREATED, Json(event)))
}

pub(super) async fn evaluate_strategy(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<EvaluateStrategyRequest>,
) -> Result<Json<EvaluateStrategyResponse>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let result = state
        .blocking(move || {
            gateway.evaluate_strategy(request.run, &request.candidate, request.comparison, auth)
        })
        .await?;
    Ok(Json(EvaluateStrategyResponse {
        schema_version: p::SchemaVersion(1),
        evaluation: result.evaluation,
        event_id: result.event_id,
    }))
}

pub(super) async fn promote_strategy(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<PromoteStrategyRequest>,
) -> Result<(StatusCode, Json<p::EventId>), ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let event = state
        .blocking(move || {
            gateway.promote_strategy(request.run, &request.candidate, &request.evaluation, auth)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(event)))
}

pub(super) async fn activate_strategy(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<ActivateStrategyRequest>,
) -> Result<Json<EvolutionActivationResponse>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let result = state
        .blocking(move || {
            gateway.activate_strategy(
                request.run,
                request.aggregate,
                &request.candidate,
                &request.evaluation,
                request.promotion,
                auth,
            )
        })
        .await?;
    Ok(Json(EvolutionActivationResponse {
        schema_version: p::SchemaVersion(1),
        append: result.append,
        snapshot: result.snapshot,
    }))
}

pub(super) async fn rollback_strategy(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<RollbackStrategyRequest>,
) -> Result<Json<EvolutionActivationResponse>, ApiError> {
    require_schema(request.schema_version)?;
    let auth = state.authorize(&headers, true)?;
    let gateway = state.gateway.clone();
    let result = state
        .blocking(move || {
            gateway.rollback_strategy(
                request.run,
                request.aggregate,
                request.domain,
                request.scope,
                request.restored,
                request.triggers,
                request.in_flight,
                auth,
            )
        })
        .await?;
    Ok(Json(EvolutionActivationResponse {
        schema_version: p::SchemaVersion(1),
        append: result.append,
        snapshot: result.snapshot,
    }))
}
