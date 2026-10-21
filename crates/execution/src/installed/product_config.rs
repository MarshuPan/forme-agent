//! Host-owned configuration staging for the managed core.
//!
//! This module never calls a model and never writes brain state. It owns the
//! product configuration file, model credential files, revision CAS, and the
//! safe projection returned to Web. The core service remains the only reader
//! that can turn the resulting file into a ProductionRuntime.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

const MAX_CONFIG_BYTES: u64 = 256 * 1024;
const MAX_STATE_BYTES: u64 = 128 * 1024;

#[derive(Clone)]
pub struct ProductConfigStore {
    template: p::ProductConfigTemplate,
    state_path: PathBuf,
    state: ProductConfigState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductConfigState {
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

impl ProductConfigState {
    fn empty(install_ref: &str) -> Self {
        Self {
            schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
            install_ref: install_ref.to_owned(),
            owner: None,
            active_revision: 0,
            acknowledged_revision: None,
            pending_restart: false,
            models: Vec::new(),
            last_operation_id: None,
            last_patch_digest: None,
        }
    }

    fn validate(&self, install_ref: &str) -> p::Result<()> {
        if self.schema_version != p::PRODUCT_CONFIG_SCHEMA_VERSION
            || self.install_ref != install_ref
            || self.active_revision > 1_000_000_000
        {
            return Err(invalid());
        }
        if let Some(owner) = &self.owner {
            owner.validate()?;
        }
        if let Some(acknowledged) = self.acknowledged_revision {
            if acknowledged == 0 || acknowledged > self.active_revision {
                return Err(invalid());
            }
        }
        for model in &self.models {
            if model.schema_version != p::PRODUCT_CONFIG_SCHEMA_VERSION
                || model.id.trim().is_empty()
                || !model.credential_configured
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

impl ProductConfigStore {
    pub fn open(template_path: &Path) -> p::Result<Mutex<Self>> {
        let template = load_json::<p::ProductConfigTemplate>(template_path, MAX_CONFIG_BYTES)?;
        template.validate()?;
        reject_path(&template.core_config_path)?;
        reject_path(&template.configuration_ack_path)?;
        reject_path(&template.model_credentials_directory)?;
        let state_path = template.core_config_path.with_extension("state.json");
        reject_path(&state_path)?;
        let state = if state_path.exists() {
            load_json::<ProductConfigState>(&state_path, MAX_STATE_BYTES)?
        } else {
            ProductConfigState::empty(&template.install_ref)
        };
        state.validate(&template.install_ref)?;
        Ok(Mutex::new(Self {
            template,
            state_path,
            state,
        }))
    }

    pub fn snapshot(&mut self) -> p::Result<p::ProductConfigView> {
        self.sync_acknowledgement()?;
        self.state.validate(&self.template.install_ref)?;
        Ok(p::ProductConfigView {
            schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
            install_ref: self.template.install_ref.clone(),
            owner: self.state.owner.clone(),
            active_revision: self.state.active_revision,
            acknowledged_revision: self.state.acknowledged_revision,
            pending_restart: self.state.pending_restart,
            configured: !self.state.models.is_empty() && self.state.owner.is_some(),
            models: self.state.models.clone(),
            issue_refs: Vec::new(),
        })
    }

    pub fn bind_owner(
        &mut self,
        binding: p::ProductConfigOwnerBinding,
    ) -> p::Result<p::ProductConfigResponse> {
        binding.validate()?;
        let digest = p::canonical_digest(&(
            &binding.schema_version,
            &binding.operation_id,
            &binding.expected_revision,
            &binding.owner,
        ))?;
        if self.state.last_operation_id.as_deref() == Some(&binding.operation_id) {
            if self.state.last_patch_digest.as_ref() != Some(&digest) {
                return Err(p::Error(
                    "configuration operation id was reused with different content".into(),
                ));
            }
            return Ok(p::ProductConfigResponse {
                schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
                view: self.snapshot()?,
                applied: false,
                operation_id: Some(binding.operation_id),
            });
        }
        if binding.expected_revision != self.state.active_revision {
            return Err(p::Error(
                "configuration revision is stale; refresh before saving".into(),
            ));
        }
        if let Some(owner) = &self.state.owner {
            if owner != &binding.owner {
                return Err(p::Error(
                    "configuration owner does not match the installed owner".into(),
                ));
            }
        }
        self.state.owner = Some(binding.owner);
        self.state.last_operation_id = Some(binding.operation_id.clone());
        self.state.last_patch_digest = Some(digest);
        self.persist_state()?;
        Ok(p::ProductConfigResponse {
            schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
            view: self.snapshot()?,
            applied: true,
            operation_id: Some(binding.operation_id),
        })
    }

    pub fn apply(&mut self, patch: p::ProductConfigPatch) -> p::Result<p::ProductConfigResponse> {
        patch.validate()?;
        let digest = p::canonical_digest(&(
            &patch.schema_version,
            &patch.operation_id,
            &patch.expected_revision,
            &patch.owner,
            &patch.models,
        ))?;
        if self.state.last_operation_id.as_deref() == Some(&patch.operation_id) {
            if self.state.last_patch_digest.as_ref() != Some(&digest) {
                return Err(p::Error(
                    "configuration operation id was reused with different content".into(),
                ));
            }
            return Ok(p::ProductConfigResponse {
                schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
                view: self.snapshot()?,
                applied: false,
                operation_id: Some(patch.operation_id),
            });
        }
        if patch.expected_revision != self.state.active_revision {
            return Err(p::Error(
                "configuration revision is stale; refresh before saving".into(),
            ));
        }
        if let Some(owner) = &self.state.owner {
            if owner != &patch.owner {
                return Err(p::Error(
                    "configuration owner does not match the installed owner".into(),
                ));
            }
        }

        fs::create_dir_all(&self.template.model_credentials_directory).map_err(|_| invalid())?;
        let mut views = Vec::with_capacity(patch.models.len());
        for model in &patch.models {
            let credential_path =
                credential_path(&self.template.model_credentials_directory, &model.id)?;
            let configured = if let Some(value) = model.credential.as_deref() {
                write_secret(&credential_path, value)?;
                true
            } else {
                credential_path.is_file()
            };
            if !configured {
                return Err(p::Error(format!(
                    "credential for model profile {} is not configured",
                    model.id
                )));
            }
            views.push(view(model));
        }

        let revision = self
            .state
            .active_revision
            .checked_add(1)
            .ok_or_else(invalid)?;
        let core = core_config(&self.template, &patch, revision)?;
        let encoded = serde_json::to_vec_pretty(&core).map_err(|_| invalid())?;
        write_atomic(&self.template.core_config_path, &encoded, 0o600)?;
        self.state = ProductConfigState {
            schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
            install_ref: self.template.install_ref.clone(),
            owner: Some(patch.owner),
            active_revision: revision,
            acknowledged_revision: None,
            pending_restart: true,
            models: views,
            last_operation_id: Some(patch.operation_id.clone()),
            last_patch_digest: Some(digest),
        };
        let state = serde_json::to_vec_pretty(&self.state).map_err(|_| invalid())?;
        write_atomic(&self.state_path, &state, 0o600)?;
        Ok(p::ProductConfigResponse {
            schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
            view: self.snapshot()?,
            applied: true,
            operation_id: Some(patch.operation_id),
        })
    }

    pub fn acknowledge(
        &mut self,
        acknowledgement: p::ProductConfigAcknowledgement,
    ) -> p::Result<()> {
        acknowledgement.validate()?;
        if acknowledgement.install_ref != self.template.install_ref
            || acknowledgement.revision != self.state.active_revision
        {
            return Err(p::Error(
                "core acknowledged an unknown configuration revision".into(),
            ));
        }
        self.state.acknowledged_revision = Some(acknowledgement.revision);
        self.state.pending_restart = false;
        self.persist_state()
    }

    fn sync_acknowledgement(&mut self) -> p::Result<()> {
        if !self.template.configuration_ack_path.is_file() {
            return Ok(());
        }
        let acknowledgement = load_json::<p::ProductConfigAcknowledgement>(
            &self.template.configuration_ack_path,
            MAX_STATE_BYTES,
        )?;
        if acknowledgement.install_ref != self.template.install_ref {
            return Err(p::Error(
                "product configuration acknowledgement belongs to another installation".into(),
            ));
        }
        if acknowledgement.revision == self.state.active_revision
            && self.state.acknowledged_revision != Some(acknowledgement.revision)
        {
            self.acknowledge(acknowledgement)?;
        } else if acknowledgement.revision > self.state.active_revision {
            return Err(p::Error(
                "product configuration acknowledgement is ahead of active state".into(),
            ));
        }
        Ok(())
    }

    fn persist_state(&self) -> p::Result<()> {
        let state = serde_json::to_vec_pretty(&self.state).map_err(|_| invalid())?;
        write_atomic(&self.state_path, &state, 0o600)
    }
}

fn view(model: &p::ProductModelProfilePatch) -> p::ProductModelProfileView {
    p::ProductModelProfileView {
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
    }
}

fn core_config(
    template: &p::ProductConfigTemplate,
    patch: &p::ProductConfigPatch,
    revision: u64,
) -> p::Result<serde_json::Value> {
    let mut models = Vec::with_capacity(patch.models.len());
    for model in &patch.models {
        let credential_path = credential_path(&template.model_credentials_directory, &model.id)?;
        models.push(serde_json::json!({
            "schema_version": 1,
            "profile": {
                "id": model.id,
                "provider": model.provider,
                "model": model.model,
                "base_url": model.base_url,
                "credential_env": "FORME_MANAGED_CREDENTIAL",
                "context_window": model.context_window,
                "tool_use": model.tool_use,
                "strength": model.strength,
                "input_microunits_per_million": model.input_microunits_per_million,
                "output_microunits_per_million": model.output_microunits_per_million,
                "requests_per_minute": model.requests_per_minute,
                "tokens_per_minute": model.tokens_per_minute,
                "timeout_ms": model.timeout_ms,
                "fallback_group": model.fallback_group,
                "default": model.default,
                "tool_schema_versions": model.tool_schema_versions,
            },
            "credential_path": credential_path,
        }));
    }
    Ok(serde_json::json!({
        "schema_version": 1,
        "install_ref": template.install_ref,
        "revision": revision,
        "brain": {
            "schema_version": 1,
            "owner": patch.owner,
            "store_path": template.brain_store_path,
            "artifact_root": template.artifact_root,
            "workspace": template.workspace,
            "agent_profile": template.agent_profile,
            "models": models,
            "scheduler_enabled": true,
            "scheduler": {
                "schema_version": 1,
                "tick": template.scheduler_tick,
                "lease": template.scheduler_lease,
                "max_claims_per_tick": template.scheduler_max_claims,
            },
            "cognition": {
                "relevance_mode": "DirectModel",
                "schema_version": 1,
                "tick_enabled": true,
                "tick_interval": template.cognition_tick,
                "stable_evidence_max_age": template.stable_evidence_max_age,
            },
            "authority_lease": template.authority_lease,
        },
        "transport": {
            "schema_version": 1,
            "bind": template.core_bind,
            "owner_credential_path": template.core_owner_credential_path,
            "web_credential_path": template.core_web_credential_path,
            "web_assertion_verifying_key": template.core_web_assertion_verifying_key,
            "web_session_fence_path": template.core_session_fence_path,
            "configuration_ack_path": template.configuration_ack_path,
            "max_body_bytes": 262144,
            "request_timeout_ms": 30000,
            "max_requests_per_minute": 120,
        },
        "drain_timeout_ms": template.drain_timeout.0,
    }))
}

fn credential_path(directory: &Path, id: &str) -> p::Result<PathBuf> {
    let digest = p::canonical_digest(&("forme-model-credential-path-v1", id))?;
    let name = format!("{}.secret", digest.0.trim_start_matches("sha256:"));
    let path = directory.join(name);
    reject_path(&path)?;
    Ok(path)
}

fn write_secret(path: &Path, value: &str) -> p::Result<()> {
    if value.trim().is_empty() || value.contains(['\r', '\n']) || value.len() > 16 * 1024 {
        return Err(p::Error("model credential is empty or malformed".into()));
    }
    write_atomic(path, value.as_bytes(), 0o600)
}

fn load_json<T: for<'de> Deserialize<'de>>(path: &Path, limit: u64) -> p::Result<T> {
    reject_path(path)?;
    let file = File::open(path).map_err(|_| invalid())?;
    let metadata = file.metadata().map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> p::Result<()> {
    reject_path(path)?;
    let parent = path.parent().ok_or_else(invalid)?;
    fs::create_dir_all(parent).map_err(|_| invalid())?;
    let nonce = p::canonical_digest(&(
        path.to_string_lossy().as_ref(),
        bytes.len(),
        std::process::id(),
    ))?;
    let temporary = parent.join(format!(".{}.tmp", nonce.0.trim_start_matches("sha256:")));
    reject_path(&temporary)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options.open(&temporary).map_err(|_| invalid())?;
    if file.write_all(bytes).is_err() || file.sync_all().is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(invalid());
    }
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path).map_err(|_| invalid())?;
    }
    fs::rename(&temporary, path).map_err(|_| {
        let _ = fs::remove_file(&temporary);
        invalid()
    })?;
    #[cfg(unix)]
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn reject_path(path: &Path) -> p::Result<()> {
    super::reject_linked_ancestors(path).map_err(|_| invalid())
}

fn invalid() -> p::Error {
    p::Error("managed product configuration is unavailable or invalid".into())
}
