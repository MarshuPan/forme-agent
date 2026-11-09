use super::*;

pub(crate) fn fixture(root: &Path) -> serde_json::Value {
    let key = ed25519_dalek::SigningKey::from_bytes(&[61; 32])
        .verifying_key()
        .to_bytes();
    serde_json::json!({
        "schema_version":1,"install_ref":"installation:managed-test","revision":1,"drain_timeout_ms":1000,
        "brain": {
            "schema_version":1,"owner":"owner:managed-test","store_path":root.join("brain.sqlite"),
            "artifact_root":root.join("artifacts"),"workspace":"workspace:managed-test","agent_profile":"agent:forme-local",
            "models":[{"schema_version":1,"credential_path":root.join("model.key"),"profile":{
                "id":"model:managed-test","provider":"managed-test","model":"fixture-model",
                "base_url":"http://127.0.0.1:9/v1","credential_env":"FORME_MANAGED_TEST_NEVER_READ",
                "context_window":32768,"default":true,"timeout_ms":1000
            }}],
            "scheduler_enabled":false,"scheduler":{"schema_version":1,"tick":1000,"lease":30000,"max_claims_per_tick":1},
            "cognition":{"schema_version":1,"relevance_mode":"DirectModel","tick_enabled":false,"tick_interval":60000,"stable_evidence_max_age":2592000000_u64},
            "authority_lease":30000
        },
        "transport": {
            "schema_version":1,"bind":"127.0.0.1:28777","owner_credential_path":root.join("owner.token"),
            "web_credential_path":root.join("web.token"),"web_assertion_verifying_key":key,
            "web_session_fence_path":root.join("fence.sqlite"),"max_body_bytes":262144,
            "request_timeout_ms":30000,"max_requests_per_minute":120
        }
    })
}

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).unwrap();
        let root = std::env::temp_dir().join(format!(
            "forme-managed-core-{:x}",
            u128::from_le_bytes(bytes)
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn configured(&self) -> CoreServiceConfig {
        std::fs::write(
            self.0.join("model.key"),
            "repository-owned-model-credential\n",
        )
        .unwrap();
        std::fs::write(
            self.0.join("owner.token"),
            "repository-owner-token-0123456789abcdef",
        )
        .unwrap();
        std::fs::write(
            self.0.join("web.token"),
            "repository-web-token-0123456789abcdef01",
        )
        .unwrap();
        let value = fixture(&self.0);
        // Use the protocol's real serde form rather than assuming enum casing.
        let mut value = value;
        value["brain"]["cognition"]["relevance_mode"] =
            serde_json::to_value(p::RelevanceMode::DirectModel).unwrap();
        serde_json::from_value(value).unwrap()
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn managed_config_uses_explicit_credentials_and_reopens_the_same_production_brain() {
    let temp = Temporary::new();
    let config = temp.configured();
    let gateway = config.transport().unwrap();
    assert!(gateway
        .token
        .matches("repository-owner-token-0123456789abcdef"));
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.brain.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.owner().0, "owner:managed-test");
    assert_eq!(
        runtime.manifest().model_profiles,
        vec![p::ModelProfileRef("model:managed-test".into())]
    );
    assert!(!runtime.config().capabilities.shell.enabled);
    assert!(!runtime.config().capabilities.file.enabled);
    let digest = runtime.manifest().digest.clone();
    drop(runtime);
    let reopened = ProductionRuntimeBuilder::from_managed_config(config.brain.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(reopened.manifest().digest, digest);
}

#[test]
fn managed_configuration_fails_closed_for_missing_secrets_versions_and_aliases() {
    let temp = Temporary::new();
    let config = temp.configured();
    let mut invalid_config = config.clone();
    invalid_config.revision = 0;
    assert!(invalid_config.validate().is_err());
    let mut invalid_config = config.clone();
    invalid_config.transport.bind = "0.0.0.0:8765".parse().unwrap();
    assert!(invalid_config.validate().is_err());
    let mut invalid_config = config.clone();
    invalid_config.transport.web_credential_path =
        invalid_config.transport.owner_credential_path.clone();
    assert!(invalid_config.validate().is_err());
    std::fs::write(
        &config.transport.web_credential_path,
        "repository-owner-token-0123456789abcdef",
    )
    .unwrap();
    assert!(config.transport().is_err());
    std::fs::remove_file(temp.0.join("model.key")).unwrap();
    let result = ProductionRuntimeBuilder::from_managed_config(config.brain)
        .unwrap()
        .build();
    assert!(result.is_err());
    assert!(!result.err().unwrap().0.contains(temp.0.to_str().unwrap()));
}

#[test]
fn managed_config_rejects_unknown_fields_relative_paths_and_oversize_files() {
    let temp = Temporary::new();
    let _ = temp.configured();
    let mut value = fixture(&temp.0);
    value["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CoreServiceConfig>(value).is_err());
    let mut config = temp.configured();
    config.brain.store_path = "relative.sqlite".into();
    assert!(config.validate().is_err());
    let path = temp.0.join("config.json");
    std::fs::write(&path, vec![b' '; 256 * 1024 + 1]).unwrap();
    assert!(CoreServiceConfig::load(&path).is_err());
}

#[path = "publisher_tests.rs"]
mod publishers;
