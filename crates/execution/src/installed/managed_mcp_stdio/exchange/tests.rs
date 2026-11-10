//! Method sequence and hostile wire data; no subprocess or native acceptance.
use super::*;

fn spec() -> p::ManagedMcpDiscoverySpec {
    p::ManagedMcpDiscoverySpec {
        schema_version: p::SchemaVersion(1),
        server: p::McpServerRef("server:fixture".into()),
        program_ref: "program:fixture".into(),
        registration_digest: p::sha256_content_digest(b"program"),
        settings_digest: p::sha256_content_digest(b"settings"),
        timeout: p::DurationMs(60_000),
        output_budget_bytes: 1_048_576,
    }
}
fn exchange() -> ManagedMcpDiscoveryExchange {
    ManagedMcpDiscoveryExchange::new(spec(), Instant::now() + Duration::from_secs(30)).unwrap()
}
fn next(exchange: &mut ManagedMcpDiscoveryExchange, method: &str) -> Value {
    let frame = exchange.next_frame().unwrap().unwrap();
    assert_eq!(frame.last(), Some(&b'\n'));
    let value: Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(value["method"], method);
    assert_eq!(value["jsonrpc"], "2.0");
    value
}
fn response(id: u64, result: Value) -> Vec<u8> {
    let mut frame = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"result":result})).unwrap();
    frame.push(b'\n');
    frame
}
fn initialized(exchange: &mut ManagedMcpDiscoveryExchange, capabilities: Value) {
    let value = next(exchange, "initialize");
    assert_eq!(value["id"], 1);
    assert_eq!(value["params"]["protocolVersion"], VERSION);
    assert_eq!(value["params"]["capabilities"], json!({}));
    exchange
        .accept_frame(&response(
            1,
            json!({"protocolVersion":VERSION,
        "capabilities":capabilities,"serverInfo":{"name":"fixture","version":"1"}}),
        ))
        .unwrap();
    let notify = next(exchange, "notifications/initialized");
    assert!(notify.get("id").is_none());
}

#[test]
fn managed_discovery_preserves_wire_order_paginates_and_returns_only_complete_catalog() {
    let mut exchange = exchange();
    initialized(&mut exchange, json!({"tools":{},"resources":{}}));
    assert_eq!(next(&mut exchange, "tools/list")["params"], json!({}));
    exchange.accept_frame(&response(2, json!({"tools":[{"name":"fixture.sum","description":"untrusted text",
        "inputSchema":{"type":"object","properties":{"x":{"type":"number"}}}}],"nextCursor":"page:two"}))).unwrap();
    assert_eq!(
        next(&mut exchange, "tools/list")["params"],
        json!({"cursor":"page:two"})
    );
    exchange
        .accept_frame(&response(3, json!({"tools":[]})))
        .unwrap();
    assert_eq!(next(&mut exchange, "resources/list")["id"], 4);
    exchange.accept_frame(&response(4, json!({"resources":[{"uri":"fixture://catalog","name":"catalog","mimeType":"text/plain"}]}))).unwrap();
    assert!(exchange.next_frame().unwrap().is_none());
    let catalog = exchange.finish().unwrap();
    assert_eq!(catalog.tools.len(), 1);
    assert_eq!(catalog.resources.len(), 1);
    assert_eq!(catalog.trust(), p::TrustTier::Untrusted);
    let bytes = serde_json::to_vec(&catalog).unwrap();
    assert_eq!(
        p::ManagedMcpDiscoveryCatalogV1::from_slice(&bytes, &spec()).unwrap(),
        catalog
    );
}

#[test]
fn managed_discovery_has_no_implicit_calls_or_undeclared_lists() {
    for capabilities in [json!({}), json!({"resources":{}}), json!({"tools":{}})] {
        let mut exchange = exchange();
        initialized(&mut exchange, capabilities.clone());
        if capabilities.get("tools").is_some() {
            next(&mut exchange, "tools/list");
            exchange
                .accept_frame(&response(2, json!({"tools":[]})))
                .unwrap();
        }
        if capabilities.get("resources").is_some() {
            next(&mut exchange, "resources/list");
            exchange
                .accept_frame(&response(2, json!({"resources":[]})))
                .unwrap();
        }
        assert!(exchange.next_frame().unwrap().is_none());
        assert!(exchange.finish().unwrap().tools.is_empty());
    }
}

#[test]
fn managed_discovery_rejects_unsolicited_malformed_wrong_id_and_error_frames_permanently() {
    let valid = response(
        1,
        json!({"protocolVersion":VERSION,"capabilities":{},"serverInfo":{}}),
    );
    let frames = [
        b"{}\n".to_vec(),
        response(2, json!({})),
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-1,\"message\":\"fixture error\"}}\n"
            .to_vec(),
        b"{\"jsonrpc\":\"2.0\",\"method\":\"sampling/createMessage\",\"id\":1,\"params\":{}}\n"
            .to_vec(),
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"id\":1,\"result\":{}}\n".to_vec(),
        valid[..valid.len() - 1].to_vec(),
        [valid.as_slice(), valid.as_slice()].concat(),
        vec![0xff, b'\n'],
    ];
    for frame in frames {
        let mut exchange = exchange();
        next(&mut exchange, "initialize");
        assert!(exchange.accept_frame(&frame).is_err());
        assert!(exchange.next_frame().is_err());
        assert!(exchange.accept_frame(&valid).is_err());
        assert!(exchange.finish().is_err());
    }
    let mut exchange = exchange();
    assert!(exchange.accept_frame(&valid).is_err());
    assert!(exchange.next_frame().is_err());
}

#[test]
fn managed_discovery_version_capabilities_and_premature_next_are_closed() {
    for result in [
        json!({"protocolVersion":"unsupported","capabilities":{},"serverInfo":{}}),
        json!({"protocolVersion":VERSION,"capabilities":{"tools":true},"serverInfo":{}}),
        json!({"protocolVersion":VERSION,"capabilities":[],"serverInfo":{}}),
    ] {
        let mut exchange = exchange();
        next(&mut exchange, "initialize");
        assert!(exchange.accept_frame(&response(1, result)).is_err());
        assert!(exchange.next_frame().is_err());
    }
    let mut exchange = exchange();
    next(&mut exchange, "initialize");
    assert!(exchange.next_frame().is_err());
    assert!(exchange.finish().is_err());
}

#[test]
fn managed_discovery_duplicate_oversized_schema_and_item_overflow_never_publish() {
    let tool = json!({"name":"fixture","inputSchema":{"type":"object"}});
    for tools in [
        json!([tool.clone(), tool.clone()]),
        json!([{"name":"bad","inputSchema":[]}]),
        json!([{"name":"large","inputSchema":{"description":"x".repeat(65536)}}]),
        Value::Array(
            (0..129)
                .map(|i| json!({"name":format!("tool-{i}"),"inputSchema":{}}))
                .collect(),
        ),
    ] {
        let mut exchange = exchange();
        initialized(&mut exchange, json!({"tools":{}}));
        next(&mut exchange, "tools/list");
        assert!(exchange
            .accept_frame(&response(2, json!({"tools":tools})))
            .is_err());
        assert!(exchange.finish().is_err());
    }
    let mut exchange = exchange();
    initialized(&mut exchange, json!({"resources":{}}));
    next(&mut exchange, "resources/list");
    let resource = json!({"uri":"fixture://one","name":"fixture"});
    assert!(exchange
        .accept_frame(&response(
            2,
            json!({"resources":[resource.clone(),resource]})
        ))
        .is_err());
    assert!(exchange.finish().is_err());
}

#[test]
fn managed_discovery_cursor_bounds_and_repetition_apply_across_pages() {
    for bad in ["".to_owned(), "x".repeat(2049), "cursor\nnext".into()] {
        let mut exchange = exchange();
        initialized(&mut exchange, json!({"tools":{}}));
        next(&mut exchange, "tools/list");
        assert!(exchange
            .accept_frame(&response(2, json!({"tools":[],"nextCursor":bad})))
            .is_err());
    }
    for repeated in [true, false] {
        let mut exchange = exchange();
        initialized(&mut exchange, json!({"tools":{}}));
        for page in 1..=8 {
            let request = next(&mut exchange, "tools/list");
            let cursor = if repeated {
                "same".into()
            } else {
                format!("page-{page}")
            };
            let result = exchange.accept_frame(&response(
                request["id"].as_u64().unwrap(),
                json!({"tools":[],"nextCursor":cursor}),
            ));
            if (repeated && page == 2) || page == 8 {
                assert!(result.is_err());
                break;
            }
            result.unwrap();
        }
        assert!(exchange.next_frame().is_err());
        assert!(exchange.finish().is_err());
    }
}

#[test]
fn managed_discovery_shares_original_deadline_and_output_budget_including_stderr() {
    let mut exchange = exchange();
    initialized(&mut exchange, json!({"tools":{}}));
    exchange.deadline = Instant::now();
    assert!(exchange.next_frame().is_err());
    exchange.deadline = Instant::now() + Duration::from_secs(1);
    assert!(exchange.next_frame().is_err());
    let mut exchange = super::tests::exchange();
    initialized(&mut exchange, json!({}));
    assert!(exchange.account_stderr(1_048_576).is_err());
    assert!(exchange.finish().is_err());
    let mut small = spec();
    small.output_budget_bytes = 256;
    let mut exchange =
        ManagedMcpDiscoveryExchange::new(small, Instant::now() + Duration::from_secs(1)).unwrap();
    initialized(&mut exchange, json!({}));
    assert!(exchange.finish().is_err());
    assert!(
        ManagedMcpDiscoveryExchange::new(spec(), Instant::now() + Duration::from_secs(61)).is_err()
    );
}
