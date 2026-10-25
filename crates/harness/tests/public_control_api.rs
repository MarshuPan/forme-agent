use forme_harness::{AgentHarness, GatewayControl, Observability, ReactiveHarness};

fn assert_public_control_contract<T: AgentHarness + GatewayControl + Observability>() {}

#[test]
fn root_level_control_traits_remain_implemented_by_reactive_harness() {
    assert_public_control_contract::<ReactiveHarness>();
}
