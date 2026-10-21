//! Composition inputs and credential binding. The composition itself is shared
//! by managed services, development entry points and durable-store fixtures.
use super::*;

pub struct ProductionRuntimeBuilder {
    config: ProductionRuntimeConfig,
    credential_files: Option<BTreeMap<String, PathBuf>>,
}

impl ProductionRuntimeBuilder {
    pub fn from_environment() -> p::Result<Self> {
        Self::from_config(ProductionRuntimeConfig::from_environment()?)
    }

    pub fn from_config(config: ProductionRuntimeConfig) -> p::Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            credential_files: None,
        })
    }

    /// Managed mode uses only explicit file credentials; it never falls back
    /// to process environment. File ACLs must also be enforced by installation.
    pub fn from_managed_config(config: crate::ManagedBrainConfig) -> p::Result<Self> {
        let runtime = config.runtime_config()?;
        let credential_files = config
            .models
            .into_iter()
            .map(|model| (model.profile.id, model.credential_path))
            .collect();
        Ok(Self {
            config: runtime,
            credential_files: Some(credential_files),
        })
    }

    pub fn build(self) -> p::Result<ProductionRuntime> {
        let path = self.config.store_path.clone();
        ensure_store_parent(&path)?;
        let store = SqliteEventStore::open(
            path,
            StoreOptions {
                fts_enabled: true,
                ..StoreOptions::default()
            },
        )?;
        self.build_with_store(store)
    }

    /// Uses exactly the same composition and credential source as `build`.
    pub fn build_with_store(self, store: SqliteEventStore) -> p::Result<ProductionRuntime> {
        self.config.validate()?;
        if store.content_protection()?.is_none() {
            let keys = forme_store::FileContentKeyResolver::new(
                self.config.store_path.with_extension("content-keys"),
            )?;
            store.enable_content_protection(forme_store::StoreContentProtection::new(
                &p::VerifiedPrincipal(self.config.owner.0.clone()),
                p::Scope(self.config.workspace.0.clone()),
                Arc::new(keys),
            )?)?;
        }
        compose(self.config, store, self.credential_files)
    }
}

pub(super) fn bind_models(
    models: &[ModelRuntimeConfig],
    credentials: Option<BTreeMap<String, PathBuf>>,
) -> p::Result<(ModelRegistry, Vec<forme_models::UnavailableModelProfile>)> {
    let config = ModelRegistryConfig {
        schema_version: p::SchemaVersion(1),
        profiles: models.to_vec(),
    };
    match credentials {
        None => ModelRegistry::from_config_degrading(config),
        Some(paths) => ModelRegistry::from_config_degrading_with_credentials(config, |profile| {
            let path = paths
                .get(&profile.id)
                .ok_or_else(|| p::Error("managed model credential is unavailable".into()))?;
            let bytes = crate::managed_config::read_managed_file(path, 16 * 1024)?;
            let value = String::from_utf8(bytes)
                .map_err(|_| p::Error("managed model credential is invalid".into()))?;
            let secret =
                forme_models::SecretString::new(value.trim_end_matches(['\r', '\n']).to_owned())?;
            Ok((
                secret,
                p::CredentialRef(format!("managed-model:{}", profile.id)),
            ))
        }),
    }
}
