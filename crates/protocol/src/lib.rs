//! Shared protocol contracts for forme (prd/02).
//!
//! The event taxonomy is frozen by architecture/03 section 2.1.1. Change the
//! architecture contract before changing an event name or adding an event.
#![forbid(unsafe_code)]

mod brain_learning;
mod brain_memory;
mod brain_orchestration;
pub use brain_memory::*;
pub use brain_orchestration::*;
mod brain_relevance;
pub use brain_relevance::*;
mod content_protection;
mod control;
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
mod normalization;
mod primitives;
mod product_bootstrap;
mod product_config;
mod provider;
mod v1_closure;
mod web_identity;
mod workbench;
mod workspace_charter;
pub use brain_learning::*;
pub use web_identity::*;
pub use workbench::*;
pub use workspace_charter::*;

pub use content_protection::*;
pub use control::*;
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
pub use provider::*;
pub use v1_closure::*;
