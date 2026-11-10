use super::*;
use std::time::Duration;

fn exchange(budget: u64) -> CallExchange {
    CallExchange::new(
        "fixture.inspect".into(),
        json!({"value":7}),
        Instant::now() + Duration::from_secs(10),
        budget,
    )
    .unwrap()
}
fn frame(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}
fn initialization() -> Vec<u8> {
    frame(json!({"jsonrpc":"2.0","id":1,"result":{
        "protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{}}}))
}
fn initialize(value: &mut CallExchange) -> Vec<String> {
    let first = value.next_frame().unwrap().unwrap();
    assert!(!first.tool_call && first.reply);
    let first: Value = serde_json::from_slice(&first.bytes).unwrap();
    assert_eq!(first["id"], 1);
    value.accept(&initialization()).unwrap();
    let second = value.next_frame().unwrap().unwrap();
    assert!(!second.tool_call && !second.reply);
    let second: Value = serde_json::from_slice(&second.bytes).unwrap();
    assert!(second.get("id").is_none());
    let third = value.next_frame().unwrap().unwrap();
    assert!(third.tool_call && third.reply);
    let third: Value = serde_json::from_slice(&third.bytes).unwrap();
    assert_eq!(third["id"], 2);
    assert_eq!(
        third["params"],
        json!({"name":"fixture.inspect","arguments":{"value":7}})
    );
    [first, second, third]
        .into_iter()
        .map(|item| item["method"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn managed_call_sends_one_exact_call_and_preserves_known_success_or_tool_error() {
    for error in [false, true] {
        let mut value = exchange(4096);
        assert_eq!(
            initialize(&mut value),
            ["initialize", "notifications/initialized", "tools/call"]
        );
        let body =
            json!({"content":[{"type":"text","text":"finite fixture output"}],"isError":error});
        value
            .accept(&frame(json!({"jsonrpc":"2.0","id":2,"result":body})))
            .unwrap();
        assert!(value.next_frame().unwrap().is_none());
        let result = value.finish(0).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&result.bytes).unwrap(),
            body
        );
        assert_eq!(
            result.outcome,
            if error {
                p::WorkerOutcome::Failed
            } else {
                p::WorkerOutcome::Completed
            }
        );
    }
    let mut value = exchange(4096);
    initialize(&mut value);
    value
        .accept(&frame(
            json!({"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"fixture rejection"}}),
        ))
        .unwrap();
    assert_eq!(value.finish(0).unwrap().outcome, p::WorkerOutcome::Failed);
}

#[test]
fn managed_call_bad_handshake_or_response_permanently_prevents_more_requests() {
    for initialization in [
        json!({"protocolVersion":"old","capabilities":{"tools":{}},"serverInfo":{}}),
        json!({"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{}}),
        json!({"protocolVersion":"2025-06-18","capabilities":{"tools":false},"serverInfo":{}}),
    ] {
        let mut value = exchange(4096);
        value.next_frame().unwrap();
        assert!(value
            .accept(&frame(
                json!({"jsonrpc":"2.0","id":1,"result":initialization})
            ))
            .is_err());
        assert!(value.next_frame().is_err());
    }
    for response in [
        json!({"jsonrpc":"2.0","id":3,"result":{"content":[]}}),
        json!({"jsonrpc":"2.0","method":"server/execute","id":2,"params":{}}),
        json!({"jsonrpc":"2.0","id":2,"result":{"content":"bad"}}),
        json!({"jsonrpc":"2.0","id":2,"result":{"content":[],"isError":"bad"}}),
        json!({"jsonrpc":"2.0","id":2,"result":{},"error":{"code":1,"message":"both"}}),
        json!({"jsonrpc":"2.0","id":2,"result":null,"error":{"code":1,"message":"both"}}),
        json!({"jsonrpc":"2.0","id":2,"result":{"content":[]},"error":null}),
        json!({"jsonrpc":"2.0","id":2,"result":{"content":[],"structuredContent":[]}}),
    ] {
        let mut value = exchange(4096);
        initialize(&mut value);
        assert!(value.accept(&frame(response)).is_err());
        assert!(value.next_frame().is_err());
        assert!(value.finish(0).is_err());
    }
    let mut value = exchange(4096);
    initialize(&mut value);
    let result = frame(json!({"jsonrpc":"2.0","id":2,"result":{"content":[]}}));
    value.accept(&result).unwrap();
    assert!(value.accept(&result).is_err());
    assert!(value.next_frame().is_err());
}

#[test]
fn managed_call_accounts_handshake_and_stderr_and_cannot_continue_after_expiry() {
    let result = frame(json!({"jsonrpc":"2.0","id":2,"result":{"content":[]}}));
    let budget = (initialization().len() + result.len()) as u64;
    let mut value = exchange(budget);
    initialize(&mut value);
    value.accept(&result).unwrap();
    assert!(value.finish(1).is_err());
    let mut value = exchange(budget);
    initialize(&mut value);
    value.accept(&result).unwrap();
    assert!(value.finish(0).is_ok());
    let mut value = exchange(4096);
    value.next_frame().unwrap();
    value.deadline = Instant::now();
    assert!(value.accept(&initialization()).is_err());
    assert!(value.next_frame().is_err());
    assert!(CallExchange::new("fixture.inspect".into(), json!({}), Instant::now(), 4096).is_err());
}
