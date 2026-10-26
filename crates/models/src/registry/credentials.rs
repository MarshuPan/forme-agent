//! Provider binding with an explicit resolver; no process-environment mutation.
use super::*;

impl ModelRegistry {
    /// Bind the actual provider from a caller-owned credential source. The
    /// resolver runs only during composition; secrets never enter model config.
    /// Required default failures block startup. Optional failures remain visible
    /// as unavailable profiles, exactly as in the development environment path.
    pub fn from_config_degrading_with_credentials<F>(
        config: ModelRegistryConfig,
        resolve: F,
    ) -> p::Result<(Self, Vec<UnavailableModelProfile>)>
    where
        F: FnMut(&ModelRuntimeConfig) -> p::Result<(SecretString, p::CredentialRef)>,
    {
        Self::from_config_degrading_with_credentials_and_proxy(config, None, resolve)
    }

    pub fn from_config_degrading_with_credentials_and_proxy<F>(
        config: ModelRegistryConfig,
        proxy: Option<ModelHttpProxy>,
        mut resolve: F,
    ) -> p::Result<(Self, Vec<UnavailableModelProfile>)>
    where
        F: FnMut(&ModelRuntimeConfig) -> p::Result<(SecretString, p::CredentialRef)>,
    {
        config.validate()?;
        if config
            .profiles
            .iter()
            .filter(|profile| profile.default)
            .count()
            != 1
        {
            return Err(p::Error(
                "model configuration must declare exactly one default profile".into(),
            ));
        }
        let default = config
            .profiles
            .iter()
            .find(|profile| profile.default)
            .ok_or_else(|| p::Error("model configuration has no default profile".into()))?;
        let mut registry = Self::new(p::ModelProfileRef(default.id.clone()))?;
        let mut unavailable = Vec::new();
        for profile in config.profiles {
            let reference = p::ModelProfileRef(profile.id.clone());
            let required = profile.default;
            let bound = resolve(&profile).and_then(|(secret, reference)| {
                profile.bind_resolved_with_proxy(secret, reference, proxy.clone())
            });
            match bound {
                Ok(model) => registry.register(model)?,
                Err(error) if required => return Err(error),
                Err(_) => unavailable.push(UnavailableModelProfile {
                    profile: reference,
                    reason: "model provider binding is unavailable (credential or endpoint)".into(),
                }),
            }
        }
        registry.validate_ready()?;
        Ok((registry, unavailable))
    }
}
