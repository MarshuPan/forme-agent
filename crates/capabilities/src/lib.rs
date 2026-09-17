//! Governed capability indexing and local capability providers (prd/07).
#![forbid(unsafe_code)]

mod app_api;
mod ecosystem;
mod m3_b;
mod mcp;
mod plugin;
mod registry;
mod skill;

pub use app_api::*;
pub use ecosystem::*;
pub use m3_b::*;
pub use mcp::*;
pub use plugin::*;
pub use registry::*;
pub use skill::*;
