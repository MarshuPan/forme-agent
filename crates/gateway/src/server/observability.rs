//! HTTP observability responsibilities; no independent runtime or writer.
use super::*;

#[derive(Debug, Serialize)]
pub(super) struct RuntimeServiceStateView {
    schema_version: p::SchemaVersion,
    name: String,
    status: String,
    reason: Option<p::ReasonRef>,
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimeManifestView {
    schema_version: p::SchemaVersion,
    owner_digest: p::SchemaDigest,
    store_identity: p::SchemaDigest,
    aggregate_refs: BTreeMap<String, p::RunId>,
    workspace: p::WorkspaceRef,
    agent_profile: p::AgentProfileRef,
    policy_profile: p::PolicyProfileRef,
    toolset_ref: p::ToolsetRef,
    model_profiles: Vec<p::ModelProfileRef>,
    capability_snapshot: p::SchemaDigest,
    service_states: Vec<RuntimeServiceStateView>,
    digest: p::SchemaDigest,
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimeDoctorView {
    schema_version: p::SchemaVersion,
    checks: Vec<RuntimeServiceStateView>,
    blocking: bool,
    digest: p::SchemaDigest,
}

/// One owner-brain tick signal: whether a window ran, and per phase what it
/// appended. The report digest is the one the durable tick ledger records.
#[derive(Debug, Serialize)]
pub(super) struct RuntimeTickView {
    schema_version: p::SchemaVersion,
    ran: bool,
    skipped_reason: Option<String>,
    window: Option<p::Timestamp>,
    attempt: Option<u32>,
    run: Option<p::RunId>,
    phases: Vec<RuntimeTickPhaseView>,
    model_calls: u32,
    report_digest: Option<p::SchemaDigest>,
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimeTickPhaseView {
    phase: &'static str,
    status: &'static str,
    event_refs: Vec<p::EventId>,
    model_calls: u32,
    deferred_reason: Option<p::ReasonRef>,
    failure: Option<p::ReasonRef>,
}

pub(super) fn runtime_tick_view(outcome: CognitiveTickOutcome) -> RuntimeTickView {
    match outcome {
        CognitiveTickOutcome::Ran(report) => {
            let report = *report;
            RuntimeTickView {
                schema_version: report.schema_version,
                ran: true,
                skipped_reason: None,
                window: Some(report.claim.window),
                attempt: Some(report.claim.attempt),
                model_calls: report.model_calls(),
                report_digest: Some(report.digest.clone()),
                run: Some(report.run),
                phases: report
                    .phases
                    .into_iter()
                    .map(|phase| RuntimeTickPhaseView {
                        phase: phase.phase.as_str(),
                        status: phase.status.as_str(),
                        event_refs: phase.event_refs,
                        model_calls: phase.model_calls,
                        deferred_reason: phase.deferred_reason,
                        failure: phase.failure,
                    })
                    .collect(),
            }
        }
        CognitiveTickOutcome::NotClaimed {
            reason,
            latest_window,
        } => RuntimeTickView {
            schema_version: p::SchemaVersion(1),
            ran: false,
            skipped_reason: Some(reason),
            window: latest_window,
            attempt: None,
            run: None,
            phases: Vec::new(),
            model_calls: 0,
            report_digest: None,
        },
    }
}

pub(super) fn runtime_service_state_view(state: &RuntimeServiceState) -> RuntimeServiceStateView {
    RuntimeServiceStateView {
        schema_version: state.schema_version,
        name: state.name.clone(),
        status: match state.status {
            RuntimeServiceStatus::Ready => "ready",
            RuntimeServiceStatus::Disabled => "disabled",
            RuntimeServiceStatus::Unsupported => "unsupported",
            RuntimeServiceStatus::Blocking => "blocking",
        }
        .into(),
        reason: state.reason.clone(),
    }
}

pub(super) fn runtime_manifest_view(manifest: &CompositionManifest) -> RuntimeManifestView {
    RuntimeManifestView {
        schema_version: manifest.schema_version,
        owner_digest: manifest.owner_digest.clone(),
        store_identity: manifest.store_identity.clone(),
        aggregate_refs: manifest
            .aggregate_refs
            .as_map()
            .into_iter()
            .map(|(kind, run)| (kind.as_str().to_owned(), run))
            .collect(),
        workspace: manifest.workspace.clone(),
        agent_profile: manifest.agent_profile.clone(),
        policy_profile: manifest.policy_profile.clone(),
        toolset_ref: manifest.toolset_ref.clone(),
        model_profiles: manifest.model_profiles.clone(),
        capability_snapshot: manifest.capability_snapshot.clone(),
        service_states: manifest
            .service_states
            .iter()
            .map(runtime_service_state_view)
            .collect(),
        digest: manifest.digest.clone(),
    }
}

pub(super) fn runtime_doctor_view(
    doctor: &forme_harness::ConfigDoctorSnapshot,
) -> RuntimeDoctorView {
    RuntimeDoctorView {
        schema_version: doctor.schema_version,
        checks: doctor
            .checks
            .iter()
            .map(runtime_service_state_view)
            .collect(),
        blocking: doctor.blocking,
        digest: doctor.digest.clone(),
    }
}

pub(super) async fn runtime_manifest(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<RuntimeManifestView>, ApiError> {
    state.authorize(&headers, false)?;
    Ok(Json(runtime_manifest_view(state.runtime.manifest())))
}

pub(super) async fn runtime_doctor(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<RuntimeDoctorView>, ApiError> {
    state.authorize(&headers, false)?;
    Ok(Json(runtime_doctor_view(&state.runtime.doctor())))
}

pub(super) async fn runtime_tick(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Result<Json<RuntimeTickView>, ApiError> {
    state.authorize(&headers, true)?;
    let runtime = state.runtime.clone();
    let outcome = state
        .blocking(move || runtime.signal_cognitive_tick())
        .await?;
    Ok(Json(runtime_tick_view(outcome)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BrainWorkspaceQuery {
    scope: Option<String>,
    capacity: Option<usize>,
    at: Option<p::Timestamp>,
}

pub(super) async fn brain_workspace(
    State(state): State<Arc<HttpState>>,
    Query(query): Query<BrainWorkspaceQuery>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let auth = state.authorize(&headers, false)?;
    let gateway = state.gateway.clone();
    let projection = state
        .blocking(move || {
            gateway.agent_workspace(
                forme_harness::AgentWorkspaceQuery {
                    schema_version: p::SchemaVersion(1),
                    scope: query.scope.map(p::Scope),
                    capacity: query.capacity.unwrap_or(10),
                    now: query.at.unwrap_or_else(now_ms),
                    config_version: p::Version(1),
                },
                auth,
            )
        })
        .await?;
    Ok(Json(
        serde_json::json!({"schema_version":1,"snapshot_ref":projection.snapshot_ref,"capacity":projection.capacity,
        "items":projection.items.iter().map(|item|serde_json::json!({"schema_version":1,"id":item.id,"kind":format!("{:?}",item.kind),
            "scope":item.scope,"value":item.value,"urgency":item.urgency,"event_ref":item.event_ref,"run":item.run,"stream_seq":item.stream_seq})).collect::<Vec<_>>()}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BrainDashboardQuery {
    workspace: Option<String>,
    from: Option<p::Timestamp>,
    to: Option<p::Timestamp>,
}

pub(super) async fn brain_dashboard(
    State(state): State<Arc<HttpState>>,
    Query(query): Query<BrainDashboardQuery>,
    headers: HeaderMap,
) -> Result<Json<forme_harness::CognitiveDashboardSnapshot>, ApiError> {
    state.authorize(&headers, false)?;
    let runtime = state.runtime.clone();
    let snapshot = state
        .blocking(move || {
            runtime.dashboard(query.workspace.map(p::WorkspaceRef), query.from, query.to)
        })
        .await?;
    Ok(Json(snapshot))
}
