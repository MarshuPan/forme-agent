//! One owner brain plans, supervises, and learns from bounded execution routes.

mod execution;
mod ledger;
mod lifecycle;
mod parent;
mod planning;
mod recovery;
mod resources;
mod support;

pub(crate) use support::signature_from_protocol;
