//! Shared protocol contracts for forme (prd/02).
//!
//! The event taxonomy is frozen by architecture/03 section 2.1.1. Change the
//! architecture contract before changing an event name or adding an event.
#![forbid(unsafe_code)]

mod action_parameters;
pub use action_parameters::*;
mod brain_learning;
mod brain_memory;
mod brain_orchestration;
pub use brain_memory::*;
pub use brain_orchestration::*;
mod brain_relevance;
pub use brain_relevance::*;
mod content_digest;
mod content_protection;
mod control;
mod control_resolution;
mod data_workbench;
mod event;
mod installed_product;
mod lifecycle;
mod m2_c;
mod m3_a;
mod m3_b;
mod m3_c;
mod m4;
mod m5;
mod managed_mcp_discovery;
pub use managed_mcp_discovery::*;
mod normalization;
mod primitives;
mod product_bootstrap;
mod product_config;
mod product_tool_artifacts;
mod product_workbench;
mod provider;
mod run_pending;
mod v1_closure;
mod web_identity;
mod workbench;
mod workbench_correction;
mod workbench_material;
mod workspace_charter;
pub use brain_learning::*;
pub use web_identity::*;
pub use workbench::*;
pub use workbench_correction::*;
pub use workbench_material::*;
pub use workspace_charter::*;

pub use content_digest::*;
pub use content_protection::*;
pub use control::*;
pub use control_resolution::*;
pub use data_workbench::*;
pub use event::*;
pub use installed_product::*;
pub use lifecycle::*;
pub use m2_c::*;
pub use m3_a::*;
pub use m3_b::*;
pub use m3_c::*;
pub use m4::*;
pub use m5::*;
pub use normalization::*;
pub use primitives::*;
pub use product_bootstrap::*;
pub use product_config::*;
pub use product_tool_artifacts::*;
pub use product_workbench::*;
pub use provider::*;
pub use run_pending::*;
pub use v1_closure::*;
