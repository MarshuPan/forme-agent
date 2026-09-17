//! Bounded Shell, File, and MCP execution backends (prd/08).
#![forbid(unsafe_code)]

mod app_api;
mod artifact;
mod browser;
mod computer;
mod external;
mod file;
mod mcp;
mod notification;
mod planner;
mod pty;
mod registry;
mod remote;
mod remote_fixture;
mod remote_ledger;
mod remote_tls;
mod shell;
mod sink;
mod support;

pub use app_api::*;
pub use artifact::*;
pub use browser::*;
pub use computer::*;
pub use external::*;
pub use file::*;
pub use mcp::*;
pub use notification::*;
pub use planner::*;
pub use pty::*;
pub use registry::*;
pub use remote::*;
pub use remote_fixture::*;
pub use remote_ledger::*;
pub use remote_tls::*;
pub use shell::*;
pub use sink::*;
pub use support::CancelToken;

use forme_protocol as p;

pub type BackendKind = p::BackendKind;

pub trait ExecutionPlanner {
    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan>;
}

pub trait ActionBackend {
    fn kind(&self) -> BackendKind;
    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan>;
    fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        cancel: CancelToken,
    ) -> p::Result<ActionResult>;
    fn cancel(&self, action: p::ActionId) -> p::Result<()>;
}
