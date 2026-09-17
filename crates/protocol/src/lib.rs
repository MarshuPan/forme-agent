//! Shared protocol contracts for forme (prd/02).
//!
//! The event taxonomy is frozen by architecture/03 section 2.1.1. Change the
//! architecture contract before changing an event name or adding an event.
#![forbid(unsafe_code)]

mod control;
mod event;
mod lifecycle;
mod m2_c;
mod m3_a;
mod m3_b;
mod m3_c;
mod m4;
mod m5;
mod primitives;
mod provider;
mod v1_closure;

pub use control::*;
pub use event::*;
pub use lifecycle::*;
pub use m2_c::*;
pub use m3_a::*;
pub use m3_b::*;
pub use m3_c::*;
pub use m4::*;
pub use m5::*;
pub use primitives::*;
pub use provider::*;
pub use v1_closure::*;
