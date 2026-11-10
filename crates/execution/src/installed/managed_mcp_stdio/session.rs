//! Select a fixed exchange; ordinary calls never discover metadata again.
use super::{
    call_exchange::{CallExchange, CallFrame, CallResult},
    exchange::ManagedMcpDiscoveryExchange,
    invalid, DiscoveryAdmission,
};
use forme_protocol as p;

pub(super) enum Session {
    Discovery(Box<ManagedMcpDiscoveryExchange>),
    Call(CallExchange),
}
impl Session {
    pub(super) fn new(admission: &DiscoveryAdmission) -> p::Result<Self> {
        match &admission.call {
            Some(call) => Ok(Self::Call(CallExchange::new(
                call.tool.clone(),
                call.arguments.clone(),
                admission.deadline,
                call.output_budget,
            )?)),
            None => Ok(Self::Discovery(Box::new(ManagedMcpDiscoveryExchange::new(
                admission.spec.clone(),
                admission.deadline,
            )?))),
        }
    }
    pub(super) fn next_frame(&mut self) -> p::Result<Option<CallFrame>> {
        match self {
            Self::Call(exchange) => exchange.next_frame(),
            Self::Discovery(exchange) => Ok(exchange.next_frame()?.map(|bytes| CallFrame {
                bytes,
                tool_call: false,
                reply: exchange.awaiting_reply(),
            })),
        }
    }
    pub(super) fn accept(&mut self, bytes: &[u8]) -> p::Result<()> {
        match self {
            Self::Call(exchange) => exchange.accept(bytes),
            Self::Discovery(exchange) => exchange.accept_frame(bytes),
        }
    }
    pub(super) fn finish(self, stderr: usize) -> p::Result<CallResult> {
        match self {
            Self::Call(exchange) => exchange.finish(stderr),
            Self::Discovery(mut exchange) => {
                exchange.account_stderr(stderr)?;
                Ok(CallResult {
                    bytes: serde_json::to_vec(&exchange.finish()?).map_err(|_| invalid())?,
                    outcome: p::WorkerOutcome::Completed,
                })
            }
        }
    }
}
