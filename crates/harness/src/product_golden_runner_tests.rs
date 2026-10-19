//! Controlled transport evidence, never live provider acceptance.
use super::*;
use forme_models::{ModelRuntimeConfig, ModelStrength, ToolDefinition};
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

const TEST_KEY_NAME: &str = "FORME_C24_RUNNER_CONTROLLED_TEST_KEY";
const TEST_KEY: &str = "c24-runner-controlled-credential-983adf5b";

struct ControlledProvider {
    config: ModelRegistryConfig,
    stopped: Arc<AtomicBool>,
    calls: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ControlledProvider {
    fn new(answer: &'static str, status: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stopped = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicU64::new(0));
        let stop = stopped.clone();
        let count = calls.clone();
        let thread = std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let body = read_body(&mut stream);
                        count.fetch_add(1, Ordering::SeqCst);
                        let messages = body["messages"].as_array().unwrap();
                        let needs_selection = messages.iter().any(|message| {
                            message["content"].as_str().is_some_and(|s| {
                                s.contains("Select at most") && s.contains("relevant memory ids")
                            })
                        });
                        let tools = body["tools"].as_array().filter(|tools| !tools.is_empty());
                        let tool_finished = messages.iter().any(|message| {
                            message["content"].as_str().is_some_and(|s| {
                                s.contains("file Write:")
                                    || s.contains("Action result:")
                                    || s.contains("file write:")
                            })
                        });
                        if tool_finished {
                            let index = messages.iter().position(|m| m["role"] == "tool").unwrap();
                            assert!(index > 0);
                            let call = &messages[index - 1];
                            let result = &messages[index];
                            assert_eq!(call["role"], "assistant");
                            assert_eq!(call["tool_calls"][0]["id"], "c24-controlled-write");
                            assert_eq!(result["tool_call_id"], call["tool_calls"][0]["id"]);
                            assert_eq!(
                                call["tool_calls"][0]["function"]["name"],
                                ToolDefinition::catalog_alias("capability:file").unwrap()
                            );
                            let arguments: p::ActionParameters = serde_json::from_str(
                                call["tool_calls"][0]["function"]["arguments"]
                                    .as_str()
                                    .unwrap(),
                            )
                            .unwrap();
                            assert!(matches!(arguments, p::ActionParameters::File {
                                operation: p::FileOperation::Write, path, ..
                            } if path == FILE_TARGET));
                            assert!(result["content"].as_str().unwrap().starts_with("[Data;"));
                        }
                        let response = if needs_selection {
                            final_response("[]")
                        } else if let Some(tools) = tools.filter(|_| !tool_finished) {
                            let tool = tools[0]["function"]["name"].as_str().unwrap();
                            assert_eq!(
                                tool,
                                ToolDefinition::catalog_alias("capability:file").unwrap()
                            );
                            let arguments = p::ActionParameters::File {
                                operation: p::FileOperation::Write,
                                path: FILE_TARGET.into(),
                                content: Some(b"47\n".to_vec()),
                            };
                            serde_json::json!({
                                "choices": [{"message": {"tool_calls": [{"id": "c24-controlled-write", "type": "function", "function": {
                                    "name": tool, "arguments": serde_json::to_string(&arguments).unwrap()
                                }}]}, "finish_reason": "tool_calls"}],
                                "usage": {"prompt_tokens": 19, "completion_tokens": 7}
                            })
                        } else {
                            final_response(answer)
                        };
                        let encoded = response.to_string();
                        let _ = write!(stream, "HTTP/1.1 {status} Controlled\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{encoded}", encoded.len());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
        });
        std::env::set_var(TEST_KEY_NAME, TEST_KEY);
        let config = ModelRegistryConfig {
            schema_version: p::SchemaVersion(1),
            profiles: vec![ModelRuntimeConfig {
                id: "model:c24-controlled".into(),
                provider: "controlled-provider".into(),
                model: "controlled-model".into(),
                base_url: format!("http://127.0.0.1:{port}/v1"),
                credential_env: TEST_KEY_NAME.into(),
                context_window: 32768,
                tool_use: true,
                strength: ModelStrength::Standard,
                input_microunits_per_million: 0,
                output_microunits_per_million: 0,
                requests_per_minute: 600,
                tokens_per_minute: 1_000_000,
                timeout_ms: 5000,
                fallback_group: None,
                default: true,
                tool_schema_versions: vec![1],
            }],
        };
        Self {
            config,
            stopped,
            calls,
            thread: Some(thread),
        }
    }
}

impl Drop for ControlledProvider {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
        // The test-only name has no production meaning. Other tests use the
        // same public fake value, so leave it while parallel providers run.
    }
}

fn final_response(answer: &str) -> serde_json::Value {
    serde_json::json!({
        "choices": [{"message": {"content": answer}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 19, "completion_tokens": 7}
    })
}

fn read_body(stream: &mut TcpStream) -> serde_json::Value {
    let mut buffer = Vec::new();
    loop {
        let mut chunk = [0; 8192];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "controlled request ended before body");
        buffer.extend_from_slice(&chunk[..count]);
        assert!(
            buffer.len() <= 2_000_000,
            "controlled request exceeded bound"
        );
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&buffer[..end]).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            if buffer.len() >= end + 4 + length {
                return serde_json::from_slice(&buffer[end + 4..end + 4 + length]).unwrap();
            }
        }
    }
}

#[test]
fn six_workflows_preserve_full_witnesses_files_restart_learning_and_failed_route() {
    let provider = ControlledProvider::new("47", 200);
    let registry = ModelRegistry::from_config(provider.config.clone()).unwrap();
    let privacy = PrivacyGuard::for_models(&provider.config);
    let artifact = run_suite("a".repeat(40), &provider.config, &registry, 2, &privacy).unwrap();
    assert_eq!(artifact.grade, ProductEvidenceGrade::Controlled);
    for task in &artifact.tasks {
        assert_eq!(task.forme.len(), 2);
        assert_eq!(task.direct_model.len(), 2);
        for primary in &task.forme {
            assert_eq!(
                primary.unavailable, None,
                "{:?}: {:?}",
                task.case, primary.unavailable
            );
            assert_eq!(primary.output.as_deref(), Some("47"), "{:?}", task.case);
            assert!(primary.cost_microunits.is_none());
            assert!(primary
                .events
                .iter()
                .all(forme_eval::portable_product_event));
            assert!(primary.witnesses.len() > primary.events.len());
            for (run, head) in &primary.stream_heads {
                assert_eq!(
                    primary.witnesses.iter().filter(|w| &w.run == run).count() as u64,
                    *head
                );
            }
        }
        for baseline in &task.direct_model {
            assert_eq!(baseline.output.as_deref(), Some("47"));
            assert!(
                baseline.events.is_empty()
                    && baseline.witnesses.is_empty()
                    && baseline.stream_heads.is_empty()
            );
            assert_eq!(
                baseline.model_profiles,
                BTreeSet::from([p::ModelProfileRef(
                    "controlled-provider:controlled-model".into()
                )])
            );
            assert_eq!(baseline.cost_microunits, None);
        }
    }
    let score = artifact.verify().unwrap();
    assert_eq!(
        score.verdict,
        ProductVerdict::Unverifiable,
        "a controlled server must not satisfy live acceptance"
    );
    for task in &score.tasks {
        assert_eq!(task.workflow_passes, 2, "{:?}", task.case);
        assert_eq!(task.quality_passes, 2);
        assert_eq!(task.baseline_quality_passes, 2);
    }
    let complex = &artifact
        .tasks
        .iter()
        .find(|t| t.case == ProductGoldenCase::ComplexTools)
        .unwrap()
        .forme[0];
    assert_eq!(complex.files[0].bytes, b"47\n");
    let long = &artifact
        .tasks
        .iter()
        .find(|t| t.case == ProductGoldenCase::LongWorkspace)
        .unwrap()
        .forme[0];
    let restart = long.restart.as_ref().unwrap();
    assert_eq!(restart.state_before, restart.state_after);
    assert_eq!(restart.owner_before, restart.owner_after);
    let failure = &artifact
        .tasks
        .iter()
        .find(|t| t.case == ProductGoldenCase::FailureRecoordination)
        .unwrap()
        .forme[0];
    assert!(failure.events.iter().any(|e| matches!(&e.payload,
        p::EventPayload::DecisionTraceRecorded(v) if v.orchestration.as_ref().is_some_and(|f| matches!(&f.operation,
            p::OrchestrationOperation::RouteClosed { status: p::WorkRouteStatus::Failed, .. })))));
    assert!(provider.calls.load(Ordering::SeqCst) >= 24);
    let json = serde_json::to_string(&artifact).unwrap();
    for forbidden in [
        TEST_KEY,
        TEST_KEY_NAME,
        "127.0.0.1",
        "http://",
        "forme-c24-",
    ] {
        assert!(!json.contains(forbidden));
    }
}

#[test]
fn failed_calls_keep_actual_profile_and_unknown_cost_without_fake_events() {
    let provider = ControlledProvider::new("47", 503);
    let mut config = provider.config.clone();
    config.profiles[0].input_microunits_per_million = 1_000_000;
    config.profiles[0].output_microunits_per_million = 2_000_000;
    let registry = ModelRegistry::from_config(config.clone()).unwrap();
    let attempt = direct_attempt(&registry, &PrivacyGuard::for_models(&config));
    assert_eq!(attempt.output, None);
    assert_eq!(attempt.cost_microunits, None);
    assert_eq!(
        attempt.unavailable.as_deref(),
        Some("baseline_provider_call_failed")
    );
    assert_eq!(attempt.model_profiles.len(), 1);
    assert!(attempt.events.is_empty() && attempt.witnesses.is_empty());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let primary = primary_attempt(
        &config,
        &registry,
        ProductGoldenCase::ColdDialogue,
        0,
        &PrivacyGuard::for_models(&config),
    );
    assert_eq!(primary.output, None);
    assert_eq!(primary.cost_microunits, None);
    assert_eq!(primary.model_profiles, attempt.model_profiles);
    assert!(primary.events.iter().any(|e| matches!(&e.payload,
        p::EventPayload::ModelCallFinished(v) if v.finish_reason.0 == "provider_error")));
    assert!(primary.witnesses.len() > primary.events.len());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn wrong_model_answer_and_private_response_never_become_success() {
    let provider = ControlledProvider::new("46", 200);
    let registry = ModelRegistry::from_config(provider.config.clone()).unwrap();
    let privacy = PrivacyGuard::for_models(&provider.config);
    let primary = primary_attempt(
        &provider.config,
        &registry,
        ProductGoldenCase::ColdDialogue,
        0,
        &privacy,
    );
    assert_eq!(primary.output.as_deref(), Some("46"));
    assert_eq!(primary.unavailable, None);
    let mut artifact = CoreBrainDArtifact::unavailable("b".repeat(40)).unwrap();
    artifact.grade = ProductEvidenceGrade::Controlled;
    let cold = artifact
        .tasks
        .iter_mut()
        .find(|t| t.case == ProductGoldenCase::ColdDialogue)
        .unwrap();
    cold.forme = vec![primary.clone(), primary];
    cold.direct_model = vec![
        direct_attempt(&registry, &privacy),
        direct_attempt(&registry, &privacy),
    ];
    artifact.seal().unwrap();
    assert_eq!(artifact.verify().unwrap().verdict, ProductVerdict::Fail);
    let unsafe_attempt = ProductAttempt {
        output: Some(format!("47 {TEST_KEY} C:\\private\\fixture")),
        files: vec![ProductFileEvidence {
            schema_version: p::SchemaVersion(1),
            relative_path: "result.txt".into(),
            bytes: TEST_KEY.as_bytes().to_vec(),
            digest: p::sha256_content_digest(TEST_KEY.as_bytes()),
        }],
        ..ProductAttempt::empty()
    };
    let sanitized = protect_attempt(unsafe_attempt, &privacy);
    assert_eq!(sanitized.output, None);
    assert_eq!(
        sanitized.unavailable.as_deref(),
        Some("nonportable_evidence_withheld")
    );
    assert!(!serde_json::to_string(&sanitized)
        .unwrap()
        .contains(TEST_KEY));
}

#[test]
fn live_opt_in_and_bounded_repeats_are_required() {
    let args = |args: &[&str]| args.iter().map(OsString::from).collect::<Vec<_>>();
    assert!(Options::parse(args(&["--out", "fresh"])).is_err());
    assert!(Options::parse(args(&["--live", "--out", "fresh", "--repeats", "1"])).is_err());
    assert!(Options::parse(args(&["--live", "--out", "fresh", "--repeats", "9"])).is_err());
    assert!(Options::parse(args(&["--live", "--out", "fresh", "--live"])).is_err());
    assert_eq!(
        Options::parse(args(&["--live", "--out", "fresh"]))
            .unwrap()
            .repeats,
        2
    );
}

#[test]
fn production_classifier_never_upgrades_controlled_endpoints_but_allows_private_providers() {
    let provider = ControlledProvider::new("47", 200);
    let mut config = provider.config.clone();
    for endpoint in [
        "http://127.0.0.1:1234/v1",
        "https://127.9.8.7/v1",
        "http://localhost/v1",
        "https://LOCALHOST.:443/v1",
        "http://model.localhost/v1",
        "https://models.invalid/v1",
        "https://MODELS.INVALID.:443/v1",
        "http://[::1]:1234/v1",
        "https://[0:0:0:0:0:0:0:1]/v1",
        "http://[::ffff:127.0.0.1]/v1",
        "http://[::ffff:7f00:1]/v1",
        "http://0.0.0.0/v1",
        "http://[::]/v1",
        "http://127.1/v1",
        "http://2130706433/v1",
        "http://0177.0.0.1/v1",
        "http://0x7f000001/v1",
    ] {
        config.profiles[0].base_url = endpoint.into();
        assert_eq!(evidence_grade(&config), ProductEvidenceGrade::Controlled);
    }
    for endpoint in [
        "https://10.20.30.40/v1",
        "https://172.20.30.40/v1",
        "https://192.168.30.40/v1",
        "https://[fd12:3456::1]/v1",
    ] {
        config.profiles[0].base_url = endpoint.into();
        assert_eq!(
            evidence_grade(&config),
            ProductEvidenceGrade::LiveConfigured
        );
    }
    config.profiles.push(provider.config.profiles[0].clone());
    assert_eq!(evidence_grade(&config), ProductEvidenceGrade::Controlled);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}
