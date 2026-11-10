use forme_protocol as p;
use serde_json::json;

fn settings() -> p::ProductMcpServerConfig {
    p::ProductMcpServerConfig {
        schema_version: p::SchemaVersion(1),
        server_ref: "fixture:catalog".into(),
        program_ref: "program:fixture".into(),
        scope: p::Scope("workspace:test".into()),
        allowlist: p::ProductMcpAllowlist {
            schema_version: p::SchemaVersion(1),
            tools: vec!["echo".into()],
            resources: vec![],
        },
        enabled: true,
        trust: p::ProductSkillTrust::OwnerInput,
        timeout: p::DurationMs(30_000),
        output_budget: 1_048_576,
    }
}
fn spec() -> p::ManagedMcpDiscoverySpec {
    p::ManagedMcpDiscoverySpec {
        schema_version: p::SchemaVersion(1),
        server: p::McpServerRef("fixture:catalog".into()),
        program_ref: "program:fixture".into(),
        registration_digest: p::canonical_digest(&"registered-source").unwrap(),
        settings_digest: p::managed_mcp_settings_digest(&settings()).unwrap(),
        timeout: p::DurationMs(1_000),
        output_budget_bytes: 1_048_576,
    }
}
fn catalog() -> p::ManagedMcpDiscoveryCatalogV1 {
    let input_schema = json!({"type":"object","properties":{"text":{"type":"string"}}});
    let mut catalog = p::ManagedMcpDiscoveryCatalogV1 {
        schema_version: p::SchemaVersion(1),
        spec: spec(),
        tools: vec![p::ManagedMcpDiscoveryToolV1 {
            schema_version: p::SchemaVersion(1),
            name: "echo".into(),
            description: "Return text.\n\tMetadata is data.".into(),
            schema_digest: p::canonical_digest(&input_schema).unwrap(),
            input_schema,
        }],
        resources: vec![p::ManagedMcpDiscoveryResourceV1 {
            schema_version: p::SchemaVersion(1),
            uri: "fixture://readme".into(),
            name: "Readme".into(),
            description: "".into(),
            mime_type: Some("text/plain".into()),
        }],
        catalog_digest: p::SchemaDigest(String::new()),
    };
    catalog.catalog_digest = catalog.computed_digest().unwrap();
    catalog
}

#[test]
fn catalog_round_trip_is_bound_and_remains_untrusted() {
    let c = catalog();
    let bytes = serde_json::to_vec(&c).unwrap();
    assert_eq!(
        p::ManagedMcpDiscoveryCatalogV1::from_slice(&bytes, &spec()).unwrap(),
        c
    );
    assert_eq!(c.trust(), p::TrustTier::Untrusted);
    assert_eq!(
        c.catalog_digest,
        p::canonical_digest(&(
            "forme.managed-mcp.discovery.catalog.v1",
            &c.spec,
            &c.tools,
            &c.resources
        ))
        .unwrap()
    );
    let mut changed = spec();
    changed.registration_digest = p::canonical_digest(&"replacement").unwrap();
    assert!(p::ManagedMcpDiscoveryCatalogV1::from_slice(&bytes, &changed).is_err());
    assert!(
        p::ManagedMcpDiscoveryCatalogV1::from_slice(&bytes[..bytes.len() - 1], &spec()).is_err()
    );
    assert!(p::ManagedMcpDiscoveryCatalogV1::from_slice(b"\xff", &spec()).is_err());
}

#[test]
fn discovery_spec_rejects_unknown_fields_versions_and_unbounded_refs() {
    let valid = serde_json::to_value(spec()).unwrap();
    for field in [
        "tool",
        "arguments",
        "stdio",
        "command",
        "env",
        "url",
        "credential",
    ] {
        let mut value = valid.clone();
        value[field] = json!("injected");
        assert!(
            serde_json::from_value::<p::ManagedMcpDiscoverySpec>(value).is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("schema_version", json!(2)),
        ("server", json!("../server")),
        ("program_ref", json!("C:/unregistered.exe")),
        ("program_ref", json!("a".repeat(129))),
        ("registration_digest", json!("sha256:short")),
        ("settings_digest", json!("sha256:short")),
        ("timeout", json!(0)),
        ("timeout", json!(60_001)),
        ("output_budget_bytes", json!(0)),
        ("output_budget_bytes", json!(1_048_577)),
    ] {
        let mut changed = valid.clone();
        changed[field] = value;
        assert!(
            serde_json::from_value::<p::ManagedMcpDiscoverySpec>(changed)
                .unwrap()
                .validate()
                .is_err(),
            "{field}"
        );
    }
    let mut boundary = spec();
    boundary.server.0 = "a".repeat(128);
    boundary.program_ref = "z".repeat(128);
    boundary.timeout.0 = 60_000;
    boundary.validate().unwrap();
}

#[test]
fn all_current_settings_participate_in_binding() {
    let s = spec();
    let cfg = settings();
    s.validate_settings(&cfg, &cfg.scope).unwrap();
    let valid = serde_json::to_value(&cfg).unwrap();
    for (field, value) in [
        ("server_ref", json!("fixture:other")),
        ("program_ref", json!("program:other")),
        ("scope", json!("workspace:other")),
        ("enabled", json!(false)),
        ("trust", json!("untrusted")),
        ("timeout", json!(29_999)),
        ("output_budget", json!(1_048_575)),
        (
            "allowlist",
            json!({"schema_version":1,"tools":[],"resources":[]}),
        ),
    ] {
        let mut changed = valid.clone();
        changed[field] = value;
        let changed: p::ProductMcpServerConfig = serde_json::from_value(changed).unwrap();
        assert_ne!(
            p::managed_mcp_settings_digest(&changed).unwrap(),
            s.settings_digest,
            "{field}"
        );
        assert!(
            s.validate_settings(&changed, &cfg.scope).is_err(),
            "{field}"
        );
    }
    assert!(s
        .validate_settings(&cfg, &p::Scope("workspace:other".into()))
        .is_err());
    let mut over = s.clone();
    over.timeout.0 = cfg.timeout.0 + 1;
    assert!(over.validate_settings(&cfg, &cfg.scope).is_err());
    let mut smaller = cfg.clone();
    smaller.output_budget = 2048;
    let mut over = s;
    over.settings_digest = p::managed_mcp_settings_digest(&smaller).unwrap();
    assert!(over.validate_settings(&smaller, &smaller.scope).is_err());
}

#[test]
fn closed_catalog_rejects_unknown_fields_at_every_level() {
    let valid = serde_json::to_value(catalog()).unwrap();
    for pointer in ["", "/spec", "/tools/0", "/resources/0"] {
        let mut c = valid.clone();
        c.pointer_mut(pointer).unwrap()["execute"] = json!(true);
        assert!(
            p::ManagedMcpDiscoveryCatalogV1::from_slice(&serde_json::to_vec(&c).unwrap(), &spec())
                .is_err(),
            "{pointer}"
        );
    }
    for pointer in [
        "/schema_version",
        "/tools/0/schema_version",
        "/resources/0/schema_version",
    ] {
        let mut c = valid.clone();
        *c.pointer_mut(pointer).unwrap() = json!(2);
        assert!(
            p::ManagedMcpDiscoveryCatalogV1::from_slice(&serde_json::to_vec(&c).unwrap(), &spec())
                .is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn recomputing_catalog_digest_cannot_hide_invalid_metadata() {
    let valid = serde_json::to_value(catalog()).unwrap();
    for (pointer, value) in [
        ("/tools/0/name", json!("echo\n")),
        ("/tools/0/name", json!("a".repeat(2049))),
        ("/tools/0/description", json!("x\u{1b}y")),
        ("/tools/0/description", json!("x".repeat(16_385))),
        ("/tools/0/input_schema", json!([])),
        ("/tools/0/input_schema", json!({"x":"x".repeat(65_536)})),
        ("/tools/0/schema_digest", json!("sha256:wrong")),
        ("/resources/0/uri", json!("bad\turi")),
        ("/resources/0/name", json!("")),
        ("/resources/0/description", json!("x\u{7f}")),
        ("/resources/0/mime_type", json!("x".repeat(257))),
    ] {
        let mut c = valid.clone();
        *c.pointer_mut(pointer).unwrap() = value;
        let mut c: p::ManagedMcpDiscoveryCatalogV1 = serde_json::from_value(c).unwrap();
        c.catalog_digest = c.computed_digest().unwrap();
        assert!(c.validate_for(&spec()).is_err(), "{pointer}");
    }
    let mut c = catalog();
    c.tools.push(c.tools[0].clone());
    c.catalog_digest = c.computed_digest().unwrap();
    assert!(c.validate_for(&spec()).is_err());
    let mut c = catalog();
    c.resources.push(c.resources[0].clone());
    c.catalog_digest = c.computed_digest().unwrap();
    assert!(c.validate_for(&spec()).is_err());
}

#[test]
fn item_counts_total_bytes_and_original_body_budget_are_enforced() {
    let mut c = catalog();
    for n in 1..128 {
        let mut tool = c.tools[0].clone();
        tool.name = format!("tool:{n}");
        c.tools.push(tool);
    }
    c.catalog_digest = c.computed_digest().unwrap();
    c.validate_for(&spec()).unwrap();
    let mut tool = c.tools[0].clone();
    tool.name = "tool:129".into();
    c.tools.push(tool);
    c.catalog_digest = c.computed_digest().unwrap();
    assert!(c.validate_for(&spec()).is_err());
    c.tools.pop();
    for tool in &mut c.tools {
        tool.description = "x".repeat(16384);
    }
    assert!(c.computed_digest().is_err());
    assert!(c.validate_for(&spec()).is_err());
    let mut c = catalog();
    c.spec.output_budget_bytes = 4096;
    c.catalog_digest = c.computed_digest().unwrap();
    c.validate_for(&c.spec).unwrap();
    let mut bytes = serde_json::to_vec(&c).unwrap();
    bytes.resize(4096, b' ');
    p::ManagedMcpDiscoveryCatalogV1::from_slice(&bytes, &c.spec).unwrap();
    bytes.push(b' ');
    assert!(p::ManagedMcpDiscoveryCatalogV1::from_slice(&bytes, &c.spec).is_err());
    c.spec.output_budget_bytes = 1;
    assert!(c.validate_for(&c.spec).is_err());
}
