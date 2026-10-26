//! Composition inputs and credential binding. The composition itself is shared
//! by managed services, development entry points and durable-store fixtures.
use super::*;
use forme_execution::installed::ManagedWorkerBinding;

pub struct ProductionRuntimeBuilder {
    config: ProductionRuntimeConfig,
    credential_files: Option<ManagedModelSources>,
    managed_key_directory: Option<PathBuf>,
    managed_mode: bool,
    worker_binding: Option<ManagedWorkerBinding>,
    // None is development; Some(None) is a managed, unrestored store.
    required_restoration_fence: Option<Option<p::SchemaDigest>>,
}

pub(super) struct ManagedModelSources {
    credentials: BTreeMap<String, PathBuf>,
    routing: p::ProductModelRouting,
    proxy: Option<ModelHttpProxy>,
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
            managed_key_directory: None,
            managed_mode: false,
            worker_binding: None,
            required_restoration_fence: None,
        })
    }

    /// Managed mode uses only explicit file credentials; it never falls back
    /// to process environment. File ACLs must also be enforced by installation.
    pub fn from_managed_config(config: crate::ManagedBrainConfig) -> p::Result<Self> {
        let model_proxy = match config.model_proxy.as_ref() {
            Some(proxy) => {
                proxy.validate()?;
                match proxy.mode {
                    p::ProductProxyMode::Direct => None,
                    p::ProductProxyMode::Http => {
                        Some(ModelHttpProxy::parse(proxy.endpoint.clone().ok_or_else(
                            || p::Error("managed model proxy endpoint is missing".into()),
                        )?)?)
                    }
                }
            }
            None => None,
        };
        let worker_binding = config.worker_binding.clone();
        let runtime = config.runtime_config()?;
        let key_directory = config.content_key_directory()?;
        let expected_fence = config.expected_restoration_fence.clone();
        let credential_files = config
            .models
            .into_iter()
            .map(|model| (model.profile.id, model.credential_path))
            .collect();
        Ok(Self {
            config: runtime,
            credential_files: Some(ManagedModelSources {
                credentials: credential_files,
                routing: config.model_routing,
                proxy: model_proxy,
            }),
            managed_key_directory: Some(key_directory),
            managed_mode: true,
            worker_binding,
            required_restoration_fence: Some(expected_fence),
        })
    }

    pub fn build(self) -> p::Result<ProductionRuntime> {
        let path = self.config.store_path.clone();
        ensure_store_parent(&path)?;
        let access = forme_store::StoreAccessGuard::online(&path)?;
        let store = SqliteEventStore::open(
            path,
            StoreOptions {
                fts_enabled: true,
                ..StoreOptions::default()
            },
        )?;
        self.finish_build(store, access)
    }

    /// Uses exactly the same composition and credential source as `build`.
    pub fn build_with_store(self, store: SqliteEventStore) -> p::Result<ProductionRuntime> {
        let access = forme_store::StoreAccessGuard::online(&self.config.store_path)?;
        self.finish_build(store, access)
    }

    fn finish_build(
        self,
        store: SqliteEventStore,
        access: forme_store::StoreAccessGuard,
    ) -> p::Result<ProductionRuntime> {
        self.config.validate()?;
        let data_operation_root = self
            .managed_key_directory
            .as_ref()
            .and_then(|path| path.parent())
            .map(|base| base.join("data-operations"));
        access.require_store(&store)?;
        if let Some(expected) = &self.required_restoration_fence {
            let actual = store
                .restored_execution_fence()?
                .map(|f| f.digest())
                .transpose()?;
            if &actual != expected {
                return Err(p::Error(
                    "managed restoration authority binding is missing or changed".into(),
                ));
            }
        }
        if store.content_protection()?.is_none() {
            let key_path = self
                .managed_key_directory
                .unwrap_or_else(|| self.config.store_path.with_extension("content-keys"));
            let keys = if self.credential_files.is_some() {
                if store.brain_authority()?.is_some() || !store.run_ids()?.is_empty() {
                    forme_store::FileContentKeyResolver::open_existing(&key_path)?;
                }
                forme_store::FileContentKeyResolver::new_managed(key_path)?
            } else {
                forme_store::FileContentKeyResolver::new(key_path)?
            };
            store.enable_content_protection(forme_store::StoreContentProtection::new(
                &p::VerifiedPrincipal(self.config.owner.0.clone()),
                p::Scope(self.config.workspace.0.clone()),
                Arc::new(keys),
            )?)?;
        }
        let mut runtime = compose(
            self.config,
            store,
            self.credential_files,
            access,
            self.managed_mode,
            self.worker_binding,
        )?;
        runtime.data_operation_root = data_operation_root;
        Ok(runtime)
    }
}

pub(super) fn bind_models(
    models: &[ModelRuntimeConfig],
    credentials: Option<ManagedModelSources>,
) -> p::Result<(ModelRegistry, Vec<forme_models::UnavailableModelProfile>)> {
    let config = ModelRegistryConfig {
        schema_version: p::SchemaVersion(1),
        profiles: models.to_vec(),
    };
    match credentials {
        None => ModelRegistry::from_config_degrading(config),
        Some(sources) => {
            let proxy = sources.proxy.clone();
            let (registry, unavailable) =
                ModelRegistry::from_config_degrading_with_credentials_and_proxy(
                    config,
                    proxy,
                    |profile| {
                        let path = sources.credentials.get(&profile.id).ok_or_else(|| {
                            p::Error("managed model credential is unavailable".into())
                        })?;
                        let bytes = crate::managed_config::read_managed_file(path, 16 * 1024)?;
                        let value = String::from_utf8(bytes)
                            .map_err(|_| p::Error("managed model credential is invalid".into()))?;
                        let secret = forme_models::SecretString::new(
                            value.trim_end_matches(['\r', '\n']).to_owned(),
                        )?;
                        Ok((
                            secret,
                            p::CredentialRef(format!("managed-model:{}", profile.id)),
                        ))
                    },
                )?;
            Ok((registry.with_role_routing(sources.routing)?, unavailable))
        }
    }
}
