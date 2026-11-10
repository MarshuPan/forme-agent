//! Owner-only discovery uses the same runtime, approval broker and event store.
use super::*;
use crate::managed_discovery::{DiscoveryRequest, PublicationRequest};

pub(super) fn routes() -> Router<Arc<HttpState>> {
    Router::new()
        .route("/v1/managed/mcp/discovery", post(discover))
        .route("/v1/managed/mcp/discovery/{run}/publish", post(publish))
        .layer(DefaultBodyLimit::max(8192))
}
fn owner(state: &HttpState, headers: &HeaderMap) -> Result<p::VerifiedPrincipal, ApiError> {
    let auth = state.authorize(headers, true)?;
    match auth.principal {
        forme_communication::AuthPrincipal::Owner(owner) if owner == state.owner => Ok(owner),
        _ => Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "managed discovery requires the configured owner",
        )),
    }
}
async fn discover(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(body): Json<DiscoveryRequest>,
) -> Result<(StatusCode, Json<p::RunId>), ApiError> {
    let owner = owner(&state, &headers)?;
    body.validate().map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "managed discovery request is invalid",
        )
    })?;
    let harness = state.runtime.harness();
    let run = state
        .blocking(move || {
            harness.discover_managed_mcp(&owner, body.session, body.command, body.spec)
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(run)))
}
async fn publish(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Path(run): Path<String>,
    Json(body): Json<PublicationRequest>,
) -> Result<Json<p::RunId>, ApiError> {
    let owner = owner(&state, &headers)?;
    body.validate()
        .and_then(|_| p::validate_product_id(&run))
        .map_err(|_| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "managed discovery publication is invalid",
            )
        })?;
    let run = p::RunId(run);
    let response = run.clone();
    let harness = state.runtime.harness();
    state
        .blocking(move || harness.publish_managed_mcp(&owner, run))
        .await?;
    Ok(Json(response))
}
