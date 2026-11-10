//! Typed action parameters shared by planning and execution.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionParameters {
    Shell {
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        network: bool,
    },
    File {
        operation: FileOperation,
        path: String,
        content: Option<Vec<u8>>,
    },
    Mcp {
        server: McpServerRef,
        tool: ToolRef,
        arguments: serde_json::Value,
        #[serde(default)]
        schema_digest: Option<SchemaDigest>,
        transport: McpTransport,
        stdio: McpStdioSpec,
        timeout: DurationMs,
    },
    Notification {
        surface: SurfaceRef,
        target: ParticipantId,
        title: String,
        body_ref: ContentRef,
    },
    Browser(BrowserActionSpec),
    Computer(ComputerActionSpec),
    Pty(PtyActionSpec),
    AppApi(AppApiActionSpec),
    Remote(Box<crate::RemoteActionSpec>),
}
