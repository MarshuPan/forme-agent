//! Managed role preferences are subordinate to model compatibility and risk.
use super::*;

impl ModelRegistry {
    pub fn with_role_routing(mut self, routing: p::ProductModelRouting) -> p::Result<Self> {
        routing.validate(
            &self
                .models
                .keys()
                .map(|profile| profile.0.clone())
                .collect::<Vec<_>>(),
        )?;
        self.routing = routing;
        Ok(self)
    }

    /// A configured cognition provider is resolved exactly; an unavailable
    /// configured reference never silently inherits the default's identity.
    pub fn cognition_ref(&self) -> &p::ModelProfileRef {
        self.routing.cognition.as_ref().unwrap_or(&self.default)
    }

    /// Fingerprints explicit roles and registered selection semantics for the
    /// production composition binding. Credential values, credential refs and
    /// resolver paths never participate. No roles preserves legacy digests.
    pub fn role_routing_digest(&self) -> p::Result<Option<p::SchemaDigest>> {
        if self.routing.foreground.is_none() && self.routing.cognition.is_none() {
            return Ok(None);
        }
        // The registry's BTreeMap and normalized compatibility entries provide
        // deterministic order, independent of input order and process lifetime.
        let profiles: Vec<_> = self
            .models
            .values()
            .map(|registered| {
                let profile = registered.profile();
                let compatibility: Vec<_> = registered
                    .tool_compatibility
                    .iter()
                    .map(|entry| {
                        (
                            entry.schema_version,
                            entry.tool_schema_version,
                            entry.compatible,
                        )
                    })
                    .collect();
                serde_json::json!({
                    "schema_version": profile.schema_version,
                    "reference": registered.reference(),
                    "provider": profile.provider,
                    "model": profile.model,
                    "endpoint": profile.base_url.as_str(),
                    "capability": (
                        profile.capability.schema_version,
                        profile.capability.context_window,
                        profile.capability.tool_use,
                        strength_rank(profile.capability.strength),
                    ),
                    "cost": (
                        profile.cost.schema_version,
                        profile.cost.input_microunits_per_million,
                        profile.cost.output_microunits_per_million,
                    ),
                    "rate_limit": (
                        profile.rate_limit.schema_version,
                        profile.rate_limit.requests_per_minute,
                        profile.rate_limit.tokens_per_minute,
                    ),
                    "fallback_group": registered.fallback_group(),
                    "tool_compatibility": compatibility,
                })
            })
            .collect();
        p::canonical_digest(&(
            "forme-model-role-routing-v1",
            &self.routing,
            &self.default,
            profiles,
        ))
        .map(Some)
    }

    pub(super) fn select_foreground(
        &self,
        request: &ModelSelectionRequest,
    ) -> p::Result<ModelSelection> {
        request.validate()?;
        self.validate_ready()?;
        let mut candidates = self
            .models
            .values()
            .filter(|registered| {
                let profile = registered.profile();
                profile.capability.context_window >= request.minimum_context_window
                    && strength_rank(profile.capability.strength)
                        >= strength_rank(request.minimum_strength)
                    && (!request.require_tool_use
                        || registered.permits_tools(request.tool_schema_version))
            })
            .collect::<Vec<_>>();
        let preferred = self.routing.foreground.as_ref();
        candidates.sort_by_key(|registered| {
            let profile = registered.profile();
            (
                preferred.is_some_and(|preferred| registered.reference() != preferred),
                strength_rank(profile.capability.strength),
                profile.cost.input_microunits_per_million,
                profile.cost.output_microunits_per_million,
                registered.reference().clone(),
            )
        });
        let selected = candidates
            .first()
            .copied()
            .ok_or_else(|| p::Error("no compatible model profile is registered".into()))?;
        let fallback_chain = candidates
            .iter()
            .skip(1)
            .filter(|candidate| {
                request.risk != p::Risk::High
                    && selected.fallback_group().is_some()
                    && candidate.fallback_group() == selected.fallback_group()
            })
            .take(2)
            .map(|candidate| candidate.reference().clone())
            .collect();
        let configured = preferred == Some(selected.reference());
        Ok(ModelSelection {
            schema_version: p::SchemaVersion(1),
            selected: selected.reference().clone(),
            policy: p::ReasonRef(
                if configured {
                    "model-selection:managed-preference"
                } else {
                    "model-selection:a-wave-deterministic"
                }
                .into(),
            ),
            rationale: p::Rationale(format!(
                "compatible profile selected for context={}, tools={}, risk={:?}, evidence={}",
                request.minimum_context_window,
                request.require_tool_use,
                request.risk,
                request.evidence.len()
            )),
            fallback_chain,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::tests::profile;

    fn registry() -> ModelRegistry {
        let mut registry = ModelRegistry::new(p::ModelProfileRef("standard".into())).unwrap();
        registry
            .register(profile("standard", ModelStrength::Standard, true, 1))
            .unwrap();
        registry
            .register(profile("strong", ModelStrength::Strong, true, 2))
            .unwrap();
        registry
            .register(profile("text", ModelStrength::Basic, false, 0))
            .unwrap();
        registry
    }

    fn request() -> ModelSelectionRequest {
        ModelSelectionRequest {
            schema_version: p::SchemaVersion(1),
            task: None,
            minimum_context_window: 8000,
            require_tool_use: true,
            tool_schema_version: Some(p::Version(1)),
            minimum_strength: ModelStrength::Standard,
            risk: p::Risk::Medium,
            evidence: vec![],
        }
    }

    #[test]
    fn managed_preference_changes_selection_but_cannot_relax_compatibility_or_risk() {
        let registry = registry()
            .with_role_routing(p::ProductModelRouting {
                schema_version: p::SchemaVersion(1),
                foreground: Some(p::ModelProfileRef("strong".into())),
                cognition: Some(p::ModelProfileRef("text".into())),
            })
            .unwrap();
        let mut request = request();
        let selected = registry.select(&request).unwrap();
        assert_eq!(selected.selected.0, "strong");
        assert_eq!(selected.policy.0, "model-selection:managed-preference");
        assert_eq!(
            selected.fallback_chain,
            vec![p::ModelProfileRef("standard".into())]
        );
        assert_eq!(registry.cognition_ref().0, "text");
        request.risk = p::Risk::High;
        assert!(registry.select(&request).unwrap().fallback_chain.is_empty());
        request.minimum_context_window = 100000;
        assert!(registry.select(&request).is_err());
        let registry = registry
            .with_role_routing(p::ProductModelRouting {
                schema_version: p::SchemaVersion(1),
                foreground: Some(p::ModelProfileRef("text".into())),
                cognition: None,
            })
            .unwrap();
        assert_eq!(
            registry
                .select(&super::tests::request())
                .unwrap()
                .selected
                .0,
            "standard"
        );
    }

    #[test]
    fn absent_role_profile_is_rejected_and_omitted_roles_keep_legacy_behavior() {
        let registry = registry();
        assert_eq!(registry.select(&request()).unwrap().selected.0, "standard");
        assert_eq!(registry.cognition_ref().0, "standard");
        assert!(registry
            .with_role_routing(p::ProductModelRouting {
                schema_version: p::SchemaVersion(1),
                foreground: None,
                cognition: Some(p::ModelProfileRef("missing".into())),
            })
            .is_err());
    }

    #[test]
    fn role_binding_is_order_independent_ignores_credentials_and_detects_selection_changes() {
        let legacy = registry();
        assert_eq!(legacy.role_routing_digest().unwrap(), None);
        let routing = p::ProductModelRouting {
            schema_version: p::SchemaVersion(1),
            foreground: Some(p::ModelProfileRef("strong".into())),
            cognition: None,
        };
        let configured = legacy.with_role_routing(routing.clone()).unwrap();
        let expected = configured.role_routing_digest().unwrap().unwrap();
        let mut reordered = ModelRegistry::new(p::ModelProfileRef("standard".into())).unwrap();
        for name in ["text", "strong", "standard"] {
            let mut registered = configured
                .get(&p::ModelProfileRef(name.into()))
                .unwrap()
                .clone();
            let mut changed_profile = registered.profile.clone();
            changed_profile.credential_ref = p::CredentialRef("file:rotated-fixture-key".into());
            registered.provider = Arc::new(
                crate::ScriptedModelProvider::new(changed_profile.clone(), vec![]).unwrap(),
            );
            registered.profile = changed_profile;
            reordered.register(registered).unwrap();
        }
        let reordered = reordered.with_role_routing(routing.clone()).unwrap();
        assert_eq!(
            reordered.role_routing_digest().unwrap(),
            Some(expected.clone())
        );

        let mut background = routing;
        background.cognition = Some(p::ModelProfileRef("text".into()));
        assert_ne!(
            configured
                .clone()
                .with_role_routing(background)
                .unwrap()
                .role_routing_digest()
                .unwrap(),
            Some(expected.clone())
        );
        // Same profile IDs do not hide a different provider model, endpoint,
        // compatibility, capability, fallback relation or selection cost.
        for field in [
            "model", "provider", "endpoint", "context", "tools", "strength", "cost", "rate",
            "fallback", "schema",
        ] {
            let mut changed = configured.clone();
            let registered = changed
                .models
                .get_mut(&p::ModelProfileRef("strong".into()))
                .unwrap();
            match field {
                "model" => registered.profile.model = "another-model".into(),
                "provider" => {
                    registered.profile.provider = p::ProviderId("another-provider".into())
                }
                "endpoint" => {
                    registered.profile.base_url = Url::parse("https://changed.invalid/v1").unwrap()
                }
                "context" => registered.profile.capability.context_window += 1,
                "tools" => registered.profile.capability.tool_use = false,
                "strength" => registered.profile.capability.strength = ModelStrength::Standard,
                "cost" => registered.profile.cost.output_microunits_per_million += 1,
                "rate" => registered.profile.rate_limit.requests_per_minute += 1,
                "fallback" => registered.fallback_group = None,
                "schema" => registered.tool_compatibility[0].compatible = false,
                _ => unreachable!(),
            }
            assert_ne!(
                changed.role_routing_digest().unwrap(),
                Some(expected.clone()),
                "{field}"
            );
        }
        assert_eq!(
            configured
                .with_role_routing(p::ProductModelRouting::default())
                .unwrap()
                .role_routing_digest()
                .unwrap(),
            None
        );
    }
}
