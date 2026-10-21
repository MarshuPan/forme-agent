//! Focused HTTP server configuration and cursor tests.
use super::*;

fn config(bind: IpAddr) -> GatewayServerConfig {
    GatewayServerConfig {
        schema_version: p::SchemaVersion(1),
        bind,
        port: 0,
        owner: p::VerifiedPrincipal("owner:test".into()),
        agent_profile: p::AgentProfileRef("agent:test".into()),
        max_body_bytes: 1024,
        request_timeout: Duration::from_secs(1),
        max_requests_per_minute: 10,
        scheduler_enabled: true,
        scheduler: p::SchedulerConfig {
            schema_version: p::SchemaVersion(1),
            tick: p::DurationMs(1_000),
            lease: p::DurationMs(30_000),
            max_claims_per_tick: 1,
        },
        install_ref: None,
        web_bridge_token: None,
        web_assertion_verifying_key: None,
        web_session_fence_path: None,
        token: LocalBearerSecret::new("unit-test-token-0123456789abcdef0123456789abcdef".into())
            .unwrap(),
        token_path: None,
    }
}

#[test]
fn gateway_config_is_loopback_only_and_debug_redacts_the_secret() {
    let local = config(IpAddr::V4(Ipv4Addr::LOCALHOST));
    local.validate().unwrap();
    let debug = format!("{local:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("unit-test-token"));

    let public = config(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    assert!(public.validate().is_err());
}

#[test]
fn last_event_id_is_run_bound_and_cannot_disagree_with_the_query() {
    let mut headers = HeaderMap::new();
    headers.insert("last-event-id", HeaderValue::from_static("run:test|7"));
    assert_eq!(event_cursor("run:test", None, &headers).unwrap(), 7);
    assert!(event_cursor("run:other", None, &headers).is_err());
    assert!(event_cursor("run:test", Some(6), &headers).is_err());
}

#[test]
fn generated_bearer_has_full_os_entropy_width() {
    let token = generate_token().unwrap();
    assert_eq!(token.len(), 64);
    assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
}
