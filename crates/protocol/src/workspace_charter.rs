//! Executable, fail-closed workspace constraints. Legacy charter DTOs remain
//! readable; production commands opt into this bounded control grammar.
use std::collections::BTreeSet;

use crate::*;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CharterRules {
    pub denied: BTreeSet<CapabilityRef>,
    pub allowed: BTreeSet<CapabilityRef>,
    pub scope: Option<Scope>,
    pub max_nodes: Option<usize>,
    pub require_review: bool,
    pub read_only: bool,
}

impl CharterRules {
    pub fn parse(charter: &WorkspaceCharterRecord) -> Result<Self> {
        charter.validate()?;
        if charter.constraints.is_empty() && charter.prohibitions.is_empty() {
            return Err(Error(
                "workspace charter needs an executable constraint".into(),
            ));
        }
        if charter.constraints.len() + charter.prohibitions.len() > 64 {
            return Err(Error("workspace charter exceeds the rule bound".into()));
        }
        let mut rules = Self::default();
        for (rule, prohibition) in charter
            .constraints
            .iter()
            .map(|r| (r, false))
            .chain(charter.prohibitions.iter().map(|r| (r, true)))
        {
            if rule.0.len() > 512 {
                return Err(Error("workspace rule is too long".into()));
            }
            match rule.0.as_str() {
                "read-only" => rules.read_only = true,
                "require-review" if !prohibition => rules.require_review = true,
                value if value.starts_with("deny-capability:") => {
                    rules.denied.insert(CapabilityRef(nonempty(&value[16..])?));
                }
                value if !prohibition && value.starts_with("allow-capability:") => {
                    rules.allowed.insert(CapabilityRef(nonempty(&value[17..])?));
                }
                value if !prohibition && value.starts_with("scope:") => {
                    let scope = Scope(nonempty(&value[6..])?);
                    if !charter_scope_contains(&Scope(charter.workspace.0.clone()), &scope)
                        || rules.scope.replace(scope).is_some()
                    {
                        return Err(Error(
                            "workspace scope rule widens or duplicates its boundary".into(),
                        ));
                    }
                }
                value if !prohibition && value.starts_with("max-nodes:") => {
                    let max = value[10..]
                        .parse::<usize>()
                        .ok()
                        .filter(|n| (1..=32).contains(n))
                        .ok_or_else(|| Error("workspace route node bound is invalid".into()))?;
                    if rules.max_nodes.replace(max).is_some() {
                        return Err(Error("workspace node bound is duplicated".into()));
                    }
                }
                _ => {
                    return Err(Error(
                        "workspace rule has no supported enforcement semantics".into(),
                    ))
                }
            }
        }
        Ok(rules)
    }

    pub fn permits_capability(&self, capability: &CapabilityRef) -> bool {
        !self.denied.contains(capability)
            && (self.allowed.is_empty() || self.allowed.contains(capability))
    }

    pub fn check_action(&self, action: &ActionIntent) -> Result<()> {
        if !self.permits_capability(&action.capability_ref)
            || self
                .scope
                .as_ref()
                .is_some_and(|scope| !charter_scope_contains(scope, &action.scope))
            || (self.read_only
                && !matches!(
                    action.action_type,
                    ActionType::Observe | ActionType::Analyze | ActionType::Prepare
                ))
        {
            return Err(Error("workspace charter denies the final action".into()));
        }
        Ok(())
    }
}

pub fn charter_scope_contains(parent: &Scope, child: &Scope) -> bool {
    parent == child
        || child
            .0
            .strip_prefix(&parent.0)
            .is_some_and(|tail| tail.starts_with('/') || tail.starts_with(':'))
}

fn nonempty(value: &str) -> Result<String> {
    if value.trim().is_empty() || value.trim() != value {
        Err(Error(
            "workspace rule argument is empty or noncanonical".into(),
        ))
    } else {
        Ok(value.to_owned())
    }
}
