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
}
