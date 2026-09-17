//! Canonical action and resource normalization shared by governance layers.
//!
//! The approval, execution, and harness crates must never invent their own
//! serialization or resource-set semantics.  This module is deliberately
//! deterministic and contains no policy decision logic.
#![forbid(unsafe_code)]

use serde_json::{Map, Value};

use crate::{canonical_digest, ActionIntent, ActionParameters, ResourceRef, Result, SchemaDigest};

/// Returns a sorted, de-duplicated resource set.
pub fn normalize_resource_set(resources: &[ResourceRef]) -> Vec<ResourceRef> {
    let mut normalized = resources.to_vec();
    normalized.sort_by(|left, right| left.0.cmp(&right.0));
    normalized.dedup_by(|left, right| left.0 == right.0);
    normalized
}

/// Derives the resources affected by an action and canonicalizes the set.
pub fn affected_resources_for(intent: &ActionIntent) -> Vec<ResourceRef> {
    normalize_resource_set(&affected_resources_for_parameters(&intent.parameters))
}

fn affected_resources_for_parameters(parameters: &ActionParameters) -> Vec<ResourceRef> {
    match parameters {
        ActionParameters::Shell { program, cwd, .. } => {
            let mut resources = vec![ResourceRef(format!("program:{program}"))];
            if let Some(cwd) = cwd {
                resources.push(ResourceRef(format!("path:{cwd}")));
            }
            resources
        }
        ActionParameters::File { path, .. } => vec![ResourceRef(format!("path:{path}"))],
        ActionParameters::Mcp { server, tool, .. } => {
            vec![ResourceRef(format!("mcp:{}:{}", server.0, tool.0))]
        }
        ActionParameters::Notification {
            surface, target, ..
        } => vec![
            ResourceRef(format!("surface:{}", surface.0)),
            ResourceRef(format!("participant:{}", target.0)),
        ],
        ActionParameters::Browser(spec) => vec![
            ResourceRef(format!("browser-driver:{}", spec.driver.0)),
            ResourceRef(format!("url:{}", spec.target_url)),
        ],
        ActionParameters::Computer(spec) => vec![
            ResourceRef(format!("computer-driver:{}", spec.driver.0)),
            ResourceRef(format!("surface:{}", spec.surface.0)),
        ],
        ActionParameters::Pty(spec) => vec![
            ResourceRef(format!("program:{}", spec.program)),
            ResourceRef(format!("path:{}", spec.cwd)),
        ],
        ActionParameters::AppApi(spec) => {
            let mut resources = vec![
                ResourceRef(format!("connector:{}", spec.connector.0)),
                ResourceRef(format!("endpoint:{}", spec.endpoint)),
            ];
            if let Some(participant) = &spec.participant {
                resources.push(ResourceRef(format!("participant:{}", participant.0)));
            }
            resources
        }
        ActionParameters::Remote(spec) => {
            let mut resources = vec![
                ResourceRef(format!("federated-peer:{}", spec.placement.executor.0)),
                ResourceRef(format!("peer-grant:{}", spec.placement.peer_grant.0)),
            ];
            resources.extend(affected_resources_for_parameters(
                &spec.placement.operation.parameters,
            ));
            resources
        }
    }
}

/// Computes the canonical digest of action semantics.
///
/// Object keys in embedded JSON values and the unordered permission set are
/// normalized before hashing.  Arrays whose order is meaningful (for example
/// shell arguments) retain their order.  The caller cannot provide or
/// override the resulting digest.
pub fn normalized_action_digest(intent: &ActionIntent) -> Result<SchemaDigest> {
    let mut value = serde_json::to_value(intent)
        .map_err(|error| crate::Error(format!("failed to encode action intent: {error}")))?;
    normalize_json_objects(&mut value);
    if let Value::Object(object) = &mut value {
        if let Some(Value::Array(permissions)) = object.get_mut("requested_permissions") {
            permissions.sort_by_key(|permission| permission.to_string());
            permissions.dedup();
        }
    }
    canonical_digest(&value)
}

fn normalize_json_objects(value: &mut Value) {
    match value {
        Value::Object(object) => {
            let entries = std::mem::take(object);
            let mut normalized = Map::new();
            let mut entries = entries.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            for (key, mut child) in entries {
                normalize_json_objects(&mut child);
                normalized.insert(key, child);
            }
            *object = normalized;
        }
        Value::Array(values) => {
            for value in values {
                normalize_json_objects(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use serde_json::{Map, Value};

    fn mcp_intent(arguments: Value, permissions: &[&str]) -> ActionIntent {
        ActionIntent {
            schema_version: SchemaVersion(1),
            intent_id: ActionId("action:test".into()),
            source: Source::UserTurn,
            goal: GoalRef("goal:test".into()),
            backend_hint: BackendKind::Mcp,
            capability_ref: CapabilityRef("mcp:test".into()),
            action_type: ActionType::Execute,
            scope: Scope("workspace:test".into()),
            risk_hint: Risk::Medium,
            expected_effect: ExpectedEffect::Internal,
            rollback_expectation: RollbackBoundary("none".into()),
            parameters: ActionParameters::Mcp {
                server: McpServerRef("server:test".into()),
                tool: ToolRef("tool:test".into()),
                arguments,
                schema_digest: Some(SchemaDigest("sha256:schema".into())),
                transport: McpTransport::Stdio,
                stdio: McpStdioSpec {
                    schema_version: SchemaVersion(1),
                    command: "mcp-test".into(),
                    args: vec!["--stdio".into()],
                    env: Vec::new(),
                },
                timeout: DurationMs(1_000),
            },
            requested_permissions: permissions
                .iter()
                .map(|permission| PermissionRef((*permission).into()))
                .collect(),
            requested_at: 42,
            estimated_output_bytes: 1_024,
            estimated_duration: DurationMs(500),
        }
    }

    #[test]
    fn resource_sets_are_sorted_and_deduplicated() {
        let resources = vec![
            ResourceRef("path:z".into()),
            ResourceRef("path:a".into()),
            ResourceRef("path:z".into()),
        ];

        assert_eq!(
            normalize_resource_set(&resources),
            vec![ResourceRef("path:a".into()), ResourceRef("path:z".into())]
        );
    }

    #[test]
    fn action_digest_ignores_json_object_and_permission_set_order() {
        let mut left_object = Map::new();
        left_object.insert("z".into(), Value::from(1));
        left_object.insert("a".into(), Value::from(2));
        let mut right_object = Map::new();
        right_object.insert("a".into(), Value::from(2));
        right_object.insert("z".into(), Value::from(1));

        let left = mcp_intent(
            Value::Object(left_object),
            &["network:read", "workspace:read", "network:read"],
        );
        let right = mcp_intent(
            Value::Object(right_object),
            &["workspace:read", "network:read"],
        );

        assert_eq!(
            normalized_action_digest(&left).expect("left digest"),
            normalized_action_digest(&right).expect("right digest")
        );
    }

    #[test]
    fn action_digest_preserves_semantic_array_order() {
        let mut left = mcp_intent(Value::Null, &["process:execute"]);
        left.backend_hint = BackendKind::Shell;
        left.capability_ref = CapabilityRef("shell:execute".into());
        left.parameters = ActionParameters::Shell {
            program: "runner".into(),
            args: vec!["first".into(), "second".into()],
            cwd: Some("workspace".into()),
            network: false,
        };
        let mut right = left.clone();
        let ActionParameters::Shell { args, .. } = &mut right.parameters else {
            unreachable!("test uses shell parameters");
        };
        args.reverse();

        assert_ne!(
            normalized_action_digest(&left).expect("left digest"),
            normalized_action_digest(&right).expect("right digest")
        );
    }

    #[test]
    fn affected_resources_are_derived_by_protocol_and_canonicalized() {
        let mut intent = mcp_intent(Value::Null, &[]);
        intent.backend_hint = BackendKind::Shell;
        intent.parameters = ActionParameters::Shell {
            program: "runner".into(),
            args: Vec::new(),
            cwd: Some("workspace".into()),
            network: false,
        };

        assert_eq!(
            affected_resources_for(&intent),
            vec![
                ResourceRef("path:workspace".into()),
                ResourceRef("program:runner".into()),
            ]
        );
    }
}
