//! External connector ingress uses its own identity and the existing Gateway.
use super::*;
use forme_communication::{
    AuthMethod, AuthPrincipal, ParticipantProfile, RawInput, TextChannelAdapter,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Message {
    schema_version: p::SchemaVersion,
    message_id: String,
    conversation: String,
    text: String,
}
impl Message {
    fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.text.trim().is_empty()
            || self.text.len() > 16 * 1024
        {
            return Err(invalid());
        }
        p::validate_product_id(&self.message_id)?;
        p::validate_product_id(&self.conversation)
    }
}
#[derive(Serialize)]
pub(super) struct Accepted {
    schema_version: p::SchemaVersion,
    accepted: bool,
}
fn invalid() -> p::Error {
    p::Error("managed channel identity or current source is unavailable".into())
}

pub(super) fn validate_configuration(config: &CoreServiceConfig) -> p::Result<()> {
    let enabled = config
        .brain
        .channels
        .as_ref()
        .map(|v| v.channels.iter().filter(|c| c.enabled).collect::<Vec<_>>())
        .unwrap_or_default();
    let Some(ingress) = &config.channel_ingress else {
        return if enabled.is_empty() {
            Ok(())
        } else {
            Err(invalid())
        };
    };
    crate::web::config::check_path(&ingress.configuration_path)?;
    if ingress.schema_version != p::SchemaVersion(1)
        || ingress.sources.len() != enabled.len()
        || ingress.configuration_path == config.brain.store_path
        || ingress
            .configuration_path
            .starts_with(&config.brain.artifact_root)
    {
        return Err(invalid());
    }
    p::ProductChannelSourceCatalog {
        schema_version: p::SchemaVersion(1),
        sources: ingress.sources.clone(),
    }
    .validate()?;
    for source in &ingress.sources {
        let settings = enabled
            .iter()
            .find(|s| s.channel_ref == source.channel_ref)
            .ok_or_else(invalid)?;
        source.validate_settings(
            settings,
            &config.brain.owner,
            &config.brain.runtime_config()?.workspace,
        )?;
    }
    Ok(())
}
fn current_source(
    ingress: &p::ProductChannelIngress,
    channel: &str,
    owner: &p::VerifiedPrincipal,
    workspace: &p::WorkspaceRef,
) -> p::Result<p::ProductChannelSource> {
    let expected = ingress
        .sources
        .iter()
        .find(|s| s.channel_ref == channel)
        .ok_or_else(invalid)?;
    let value: serde_json::Value = serde_json::from_slice(&super::managed_config::read_file(
        &ingress.configuration_path,
        256 * 1024,
    )?)
    .map_err(|_| invalid())?;
    if value["brain"]["owner"].as_str() != Some(owner.0.as_str())
        || value["brain"]["workspace"].as_str() != Some(workspace.0.as_str())
    {
        return Err(invalid());
    }
    let current: p::ProductChannelIngress =
        serde_json::from_value(value["channel_ingress"].clone()).map_err(|_| invalid())?;
    if current.schema_version != p::SchemaVersion(1)
        || current.configuration_path != ingress.configuration_path
        || !current.sources.contains(expected)
    {
        return Err(invalid());
    }
    let settings: p::ProductChannelSettings =
        serde_json::from_value(value["brain"]["channels"].clone()).map_err(|_| invalid())?;
    settings.validate()?;
    let setting = settings
        .channels
        .iter()
        .find(|s| s.channel_ref == channel)
        .ok_or_else(invalid)?;
    expected.validate_settings(setting, &p::OwnerPrincipalRef(owner.0.clone()), workspace)?;
    let catalog: p::ProductChannelSourceCatalog =
        serde_json::from_slice(&super::managed_config::read_file(
            &ingress
                .configuration_path
                .with_extension("channel-sources.json"),
            128 * 1024,
        )?)
        .map_err(|_| invalid())?;
    catalog.validate()?;
    if !catalog.sources.contains(expected) {
        return Err(invalid());
    }
    Ok(expected.clone())
}
fn authenticate(source: &p::ProductChannelSource, token: &str) -> p::Result<()> {
    if token.len() < 32 || token.len() > 4096 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(invalid());
    }
    let digest = p::sha256_content_digest(token.as_bytes());
    if !LocalBearerSecret::new(source.credential_digest.0.clone())?.matches(&digest.0) {
        return Err(invalid());
    }
    Ok(())
}
fn submit(state: &HttpState, channel: &str, token: &str, message: Message) -> p::Result<()> {
    message.validate()?;
    let ingress = state.channel_ingress.as_ref().ok_or_else(invalid)?;
    let source = current_source(
        ingress,
        channel,
        &state.owner,
        &state.runtime.config().workspace,
    )?;
    authenticate(&source, token)?;
    let participant = source.participant.clone();
    let adapter = TextChannelAdapter {
        schema_version: p::SchemaVersion(1),
        id: p::ProviderId(source.channel_ref.clone()),
        participant: ParticipantProfile::External(participant.clone()),
        scope: source.scope.clone(),
        enabled: true,
    };
    let message_id = p::canonical_digest(&(
        "forme.channel.message.v1",
        &source.channel_ref,
        &participant,
        &message.message_id,
    ))?
    .0;
    let session = p::canonical_digest(&(
        "forme.channel.session.v1",
        &source.channel_ref,
        &participant,
        &message.conversation,
    ))?
    .0;
    let raw = RawInput {
        schema_version: p::SchemaVersion(1),
        message_id,
        modality: p::Modality::Text,
        content: message.text,
        session_ref: Some(p::CommunicationSessionId(format!("channel:{session}"))),
        received_at: now_ms(),
    };
    let auth = AuthContext {
        schema_version: p::SchemaVersion(1),
        principal: AuthPrincipal::External(participant),
        method: AuthMethod::Token,
        expires_at: now_ms() + 30_000,
    };
    // Re-read the source immediately before the existing, durable ingress.
    if current_source(
        ingress,
        channel,
        &state.owner,
        &state.runtime.config().workspace,
    )? != source
    {
        return Err(invalid());
    }
    state.gateway.start_channel_event(&adapter, raw, auth)?;
    Ok(())
}
pub(super) async fn receive(
    State(state): State<Arc<HttpState>>,
    Path(channel): Path<String>,
    headers: HeaderMap,
    body: Result<Json<Message>, axum::extract::rejection::JsonRejection>,
) -> Result<(StatusCode, Json<Accepted>), ApiError> {
    let denied = || ApiError::new(StatusCode::UNAUTHORIZED, "channel input is not authorized");
    if !state
        .rate
        .lock()
        .map_err(|_| denied())?
        .admit(state.max_requests_per_minute)
    {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "channel input rate exceeded",
        ));
    }
    if headers.contains_key(header::ORIGIN)
        || headers.contains_key(header::COOKIE)
        || headers.keys().any(|k| k.as_str().starts_with("x-forme-"))
    {
        return Err(denied());
    }
    p::validate_product_id(&channel).map_err(|_| denied())?;
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(denied)?
        .to_owned();
    if state.token.matches(&token) {
        return Err(denied());
    }
    let message = body
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "channel message is invalid"))?
        .0;
    message
        .validate()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "channel message is invalid"))?;
    let worker = state.clone();
    state
        .blocking(move || submit(&worker, &channel, &token, message))
        .await
        .map_err(|_| denied())?;
    Ok((
        StatusCode::ACCEPTED,
        Json(Accepted {
            schema_version: p::SchemaVersion(1),
            accepted: true,
        }),
    ))
}
