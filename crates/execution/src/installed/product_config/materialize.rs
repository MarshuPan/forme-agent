//! Immutable private revisions and static validation, with no provider calls.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use forme_protocol as p;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use super::{
    storage, ConfigState, ProductConfigStore, SnapshotRef, MAX_CONFIG_BYTES, MAX_STATE_BYTES,
};
use storage::invalid;

pub(super) fn bytes_digest(bytes: &[u8]) -> p::SchemaDigest {
    p::SchemaDigest(format!("sha256:{:x}", Sha256::digest(bytes)))
}

pub(super) fn new_ref() -> p::Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| invalid())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) fn validate_template(
    template: &p::ProductConfigTemplate,
    source: &Path,
) -> p::Result<()> {
    template.validate()?;
    let journal = template.core_config_path.with_extension("journal.jsonl");
    let revisions = template.core_config_path.with_extension("revisions");
    let legacy = template.core_config_path.with_extension("state.json");
    let paths = [
        source,
        &template.core_config_path,
        &journal,
        &revisions,
        &legacy,
        &template.configuration_ack_path,
        &template.model_credentials_directory,
        &template.brain_store_path,
        &template.artifact_root,
        &template.core_owner_credential_path,
        &template.core_web_credential_path,
        &template.core_session_fence_path,
    ];
    for (index, path) in paths.iter().enumerate() {
        storage::check(path)?;
        let normalized = normalized_path(path);
        if paths.iter().skip(index + 1).any(|other| {
            let other = normalized_path(other);
            normalized == other
                || normalized.starts_with(&(other.clone() + "/"))
                || other.starts_with(&(normalized.clone() + "/"))
        }) {
            return Err(invalid());
        }
    }
    Ok(())
}

fn normalized_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    let text = text.to_lowercase();
    text.trim_end_matches('/').to_owned()
}

pub(super) fn validate_model(model: &p::ProductModelProfilePatch) -> p::Result<()> {
    model.validate()?;
    let url =
        Url::parse(&model.base_url).map_err(|_| p::Error("model endpoint is invalid".into()))?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || model.model.chars().any(char::is_control)
        || model.provider.chars().any(char::is_control)
        || model.timeout_ms > 600_000
    {
        return Err(p::Error("model endpoint or limits are invalid".into()));
    }
    Ok(())
}

pub(super) fn patch_from_view(view: &p::ProductModelProfileView) -> p::ProductModelProfilePatch {
    p::ProductModelProfilePatch {
        schema_version: view.schema_version,
        id: view.id.clone(),
        provider: view.provider.clone(),
        model: view.model.clone(),
        base_url: view.base_url.clone(),
        credential: None,
        context_window: view.context_window,
        tool_use: view.tool_use,
        strength: view.strength.clone(),
        input_microunits_per_million: view.input_microunits_per_million,
        output_microunits_per_million: view.output_microunits_per_million,
        requests_per_minute: view.requests_per_minute,
        tokens_per_minute: view.tokens_per_minute,
        timeout_ms: view.timeout_ms,
        fallback_group: view.fallback_group.clone(),
        default: view.default,
        tool_schema_versions: view.tool_schema_versions.clone(),
    }
}

pub(super) fn prepare_models(
    models: &[p::ProductModelProfilePatch],
    previous: &BTreeMap<String, PathBuf>,
) -> p::Result<Vec<(p::ProductModelProfileView, Vec<u8>)>> {
    models
        .iter()
        .map(|model| {
            validate_model(model)?;
            let credential = match &model.credential {
                Some(value) => value.as_bytes().to_vec(),
                None => storage::secret(
                    previous
                        .get(&model.id)
                        .ok_or_else(|| p::Error("model credential is not configured".into()))?,
                )?,
            };
            let view = p::ProductModelProfileView {
                schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
                id: model.id.clone(),
                provider: model.provider.clone(),
                model: model.model.clone(),
                base_url: model.base_url.clone(),
                context_window: model.context_window,
                tool_use: model.tool_use,
                strength: model.strength.clone(),
                input_microunits_per_million: model.input_microunits_per_million,
                output_microunits_per_million: model.output_microunits_per_million,
                requests_per_minute: model.requests_per_minute,
                tokens_per_minute: model.tokens_per_minute,
                timeout_ms: model.timeout_ms,
                fallback_group: model.fallback_group.clone(),
                default: model.default,
                tool_schema_versions: model.tool_schema_versions.clone(),
                credential_configured: true,
            };
            Ok((view, credential))
        })
        .collect()
}

fn credential_name(id: &str) -> p::Result<String> {
    let digest = p::canonical_digest(&("forme-model-credential-path-v1", id))?;
    Ok(format!("{}.secret", digest.0.trim_start_matches("sha256:")))
}

pub(super) fn write_credentials(
    template: &p::ProductConfigTemplate,
    id: &str,
    models: &[(p::ProductModelProfileView, Vec<u8>)],
) -> p::Result<BTreeMap<String, PathBuf>> {
    let directory = template.model_credentials_directory.join(id);
    let paths = credential_paths(template, id, models)?;
    super::service_files::directory(&directory)?;
    for (view, secret) in models {
        super::service_files::immutable(paths.get(&view.id).ok_or_else(invalid)?, secret)?;
    }
    storage::sync_directory(&template.model_credentials_directory)?;
    Ok(paths)
}

pub(super) fn credential_paths(
    template: &p::ProductConfigTemplate,
    id: &str,
    models: &[(p::ProductModelProfileView, Vec<u8>)],
) -> p::Result<BTreeMap<String, PathBuf>> {
    models
        .iter()
        .map(|(view, _)| {
            Ok((
                view.id.clone(),
                template
                    .model_credentials_directory
                    .join(id)
                    .join(credential_name(&view.id)?),
            ))
        })
        .collect()
}

pub(super) fn validate_size(
    template: &p::ProductConfigTemplate,
    state: &ConfigState,
) -> p::Result<()> {
    if storage::encode(state)?.len() as u64 > MAX_STATE_BYTES
        || storage::encode(&core_config(template, state)?)?.len() as u64 > MAX_CONFIG_BYTES
    {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn revision_directory(template: &p::ProductConfigTemplate, id: &str) -> PathBuf {
    template
        .core_config_path
        .with_extension("revisions")
        .join(id)
}

pub(super) fn stage(
    template: &p::ProductConfigTemplate,
    state: &ConfigState,
) -> p::Result<SnapshotRef> {
    let id = new_ref()?;
    let directory = revision_directory(template, &id);
    storage::directory(&directory)?;
    let bytes = storage::encode(state)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(invalid());
    }
    storage::immutable(&directory.join("state.json"), &bytes)?;
    let core_digest = if state.revision > 0 {
        let core = storage::encode(&core_config(template, state)?)?;
        if core.len() as u64 > MAX_CONFIG_BYTES {
            return Err(invalid());
        }
        storage::immutable(&directory.join("core.json"), &core)?;
        Some(bytes_digest(&core))
    } else {
        None
    };
    storage::sync_directory(directory.parent().ok_or_else(invalid)?)?;
    Ok(SnapshotRef {
        id,
        revision: state.revision,
        state_digest: bytes_digest(&bytes),
        core_digest,
    })
}

pub(super) fn core_bytes(
    template: &p::ProductConfigTemplate,
    reference: &SnapshotRef,
) -> p::Result<Vec<u8>> {
    let bytes = storage::read(
        &revision_directory(template, &reference.id).join("core.json"),
        MAX_CONFIG_BYTES,
    )?;
    if Some(bytes_digest(&bytes)) != reference.core_digest {
        return Err(invalid());
    }
    Ok(bytes)
}

pub(super) fn core_config(
    template: &p::ProductConfigTemplate,
    state: &ConfigState,
) -> p::Result<serde_json::Value> {
    let mut models = Vec::with_capacity(state.models.len());
    for view in &state.models {
        let mut profile = serde_json::to_value(patch_from_view(view)).map_err(|_| invalid())?;
        let profile = profile.as_object_mut().ok_or_else(invalid)?;
        profile.remove("schema_version");
        profile.remove("credential");
        profile.insert("credential_env".into(), "FORME_MANAGED_CREDENTIAL".into());
        models.push(serde_json::json!({
            "schema_version": 1, "profile": profile,
            "credential_path": state.credentials.get(&view.id).ok_or_else(invalid)?,
        }));
    }
    let (store_path, content_key_directory, expected_restoration_fence) = state
        .active_data
        .as_ref()
        .map(|binding| {
            (
                binding.store_path.clone(),
                Some(binding.content_key_directory.clone()),
                binding.execution_fence_digest.clone(),
            )
        })
        .unwrap_or((template.brain_store_path.clone(), None, None));
    let mut config = serde_json::json!({
        "schema_version":1, "install_ref":template.install_ref, "revision":state.revision,
        "brain": {
            "schema_version":1, "owner":state.owner.as_ref().ok_or_else(invalid)?,
            "store_path":store_path, "content_key_directory":content_key_directory,
            "expected_restoration_fence":expected_restoration_fence,
            "artifact_root":template.artifact_root,
            "workspace":template.workspace, "agent_profile":template.agent_profile, "models":models,
            "scheduler_enabled":true,
            "scheduler":{"schema_version":1,"tick":template.scheduler_tick,"lease":template.scheduler_lease,"max_claims_per_tick":template.scheduler_max_claims},
            "cognition":{"schema_version":1,"relevance_mode":"DirectModel","tick_enabled":true,"tick_interval":template.cognition_tick,"stable_evidence_max_age":template.stable_evidence_max_age},
            "authority_lease":template.authority_lease
        },
        "transport":{
            "schema_version":1,"bind":template.core_bind,
            "owner_credential_path":template.core_owner_credential_path,
            "web_credential_path":template.core_web_credential_path,
            "web_assertion_verifying_key":template.core_web_assertion_verifying_key,
            "web_session_fence_path":template.core_session_fence_path,
            "configuration_ack_path":template.configuration_ack_path,
            "max_body_bytes":262144,"request_timeout_ms":30000,"max_requests_per_minute":120
        },
        "drain_timeout_ms":template.drain_timeout.0
    });
    // Do not add fields to historical materializations: their bytes remain
    // bound to the original immutable revision and journal digest.
    if let Some(brain) = &state.brain {
        config["brain"]["workspace"] =
            serde_json::to_value(&brain.default_workspace).map_err(|_| invalid())?;
        config["brain"]["model_routing"] =
            serde_json::to_value(&brain.model_routing).map_err(|_| invalid())?;
    }
    if let Some(capabilities) = &state.capabilities {
        config["brain"]["capabilities"] =
            serde_json::to_value(capabilities).map_err(|_| invalid())?;
    }
    if state.active_data.is_some() {
        config["transport"]["work_admission_path"] = serde_json::to_value(
            template
                .core_config_path
                .with_extension("work-admission.json"),
        )
        .map_err(|_| invalid())?;
    }
    Ok(config)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyState {
    schema_version: p::SchemaVersion,
    install_ref: String,
    owner: Option<p::OwnerPrincipalRef>,
    active_revision: u64,
    acknowledged_revision: Option<u64>,
    pending_restart: bool,
    models: Vec<p::ProductModelProfileView>,
    last_operation_id: Option<String>,
    last_patch_digest: Option<p::SchemaDigest>,
}

pub(super) fn import_legacy(store: &mut ProductConfigStore) -> p::Result<()> {
    let path = store.template.core_config_path.with_extension("state.json");
    if !path.try_exists().map_err(|_| invalid())? {
        if store
            .template
            .core_config_path
            .try_exists()
            .map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        return Ok(());
    }
    let legacy: LegacyState = storage::json(&path, MAX_STATE_BYTES)?;
    if legacy.schema_version != p::PRODUCT_CONFIG_SCHEMA_VERSION
        || legacy.install_ref != store.template.install_ref
        || legacy
            .acknowledged_revision
            .is_some_and(|revision| revision == 0 || revision > legacy.active_revision)
        || (legacy.active_revision > 0
            && !legacy.pending_restart
            && legacy.acknowledged_revision != Some(legacy.active_revision))
    {
        return Err(invalid());
    }
    let mut credentials = BTreeMap::new();
    for model in &legacy.models {
        let path = store
            .template
            .model_credentials_directory
            .join(credential_name(&model.id)?);
        storage::secret(&path)?;
        credentials.insert(model.id.clone(), path);
    }
    let state = ConfigState {
        schema_version: p::SchemaVersion(3),
        install_ref: legacy.install_ref,
        owner: legacy.owner,
        revision: legacy.active_revision,
        models: legacy.models,
        credentials,
        active_data: None,
        brain: None,
        capabilities: None,
    };
    state.validate(&store.template)?;
    if state.revision > 0 {
        let actual: serde_json::Value =
            storage::json(&store.template.core_config_path, MAX_CONFIG_BYTES)?;
        if actual != core_config(&store.template, &state)? {
            return Err(invalid());
        }
    } else if store
        .template
        .core_config_path
        .try_exists()
        .map_err(|_| invalid())?
    {
        return Err(invalid());
    }
    // Legacy history only retained its final operation; importing it does not
    // invent receipts for earlier operations. Stale revision CAS remains enforced.
    let operation = legacy.last_operation_id.ok_or_else(invalid)?;
    let digest = legacy.last_patch_digest.ok_or_else(invalid)?;
    let reference = stage(&store.template, &state)?;
    store.journal.import(operation, digest, reference)?;
    store.recover()?;
    if let Some(revision) = legacy.acknowledged_revision {
        if revision == state.revision {
            store.journal.acknowledge(revision)?;
        }
    }
    Ok(())
}
