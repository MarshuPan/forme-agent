//! Policy argument matching, independent of authority evaluation.
use crate::{p, path_within};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionMatcher {
    pub backend: Option<p::BackendKind>,
    pub capability: Option<p::CapabilityRef>,
    pub action_type: Option<p::ActionType>,
    pub parameters: ArgMatcher,
}

impl Default for ActionMatcher {
    fn default() -> Self {
        Self {
            backend: None,
            capability: None,
            action_type: None,
            parameters: ArgMatcher::Any,
        }
    }
}

impl ActionMatcher {
    pub(crate) fn matches(&self, intent: &p::ActionIntent) -> bool {
        self.backend
            .is_none_or(|value| value == intent.backend_hint)
            && self
                .capability
                .as_ref()
                .is_none_or(|value| value == &intent.capability_ref)
            && self
                .action_type
                .is_none_or(|value| value == intent.action_type)
            && self.parameters.matches(intent)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgMatcher {
    Any,
    ShellProgram(String),
    ShellArgsPrefix(Vec<String>),
    FilePathPrefix(String),
    /// An authenticated owner ingress inserts this request-specific Session
    /// matcher with effect Ask. It cannot grant a discovery without approval.
    McpDiscoveryTarget {
        server: p::McpServerRef,
        program_ref: String,
        action_digest: p::SchemaDigest,
    },
    McpTarget {
        server: p::McpServerRef,
        tool: p::ToolRef,
    },
}

impl ArgMatcher {
    pub fn matches(&self, intent: &p::ActionIntent) -> bool {
        match (self, &intent.parameters) {
            (Self::Any, _) => true,
            (
                Self::McpDiscoveryTarget {
                    server,
                    program_ref,
                    action_digest,
                },
                p::ActionParameters::McpDiscovery(spec),
            ) => {
                spec.server == *server
                    && spec.program_ref == *program_ref
                    && p::normalized_action_digest(intent)
                        .is_ok_and(|digest| digest == *action_digest)
            }
            (Self::ShellProgram(expected), p::ActionParameters::Shell { program, .. }) => {
                expected == program
            }
            (Self::ShellArgsPrefix(expected), p::ActionParameters::Shell { args, .. }) => {
                args.starts_with(expected)
            }
            (Self::FilePathPrefix(root), p::ActionParameters::File { path, .. }) => {
                path_within(root, path)
            }
            (
                Self::McpTarget {
                    server: expected_server,
                    tool: expected_tool,
                },
                p::ActionParameters::Mcp { server, tool, .. },
            ) => expected_server == server && expected_tool == tool,
            _ => false,
        }
    }
}
