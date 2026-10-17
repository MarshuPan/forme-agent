#![allow(dead_code)]
use forme_harness::*;
use forme_models::{ModelRuntimeConfig, ModelStrength};
use forme_protocol as p;
use forme_store::EventStore;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static LOCK: Mutex<()> = Mutex::new(());
static SEQ: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    _lock: MutexGuard<'static, ()>,
    pub config: ProductionRuntimeConfig,
    pub calls: Arc<AtomicU64>,
    pub answer: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    root: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FORME_CORE_B_TEST_KEY", "core-b-local-fixture-key");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicU64::new(0));
        let answer = Arc::new(Mutex::new(String::from("[]")));
        let (flag, count, output) = (stop.clone(), calls.clone(), answer.clone());
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        read_request(&mut stream);
                        count.fetch_add(1, Ordering::SeqCst);
                        let content = output.lock().unwrap().clone();
                        let body = serde_json::json!({"choices":[{"message":{"content":content},"finish_reason":"stop"}],
                            "usage":{"prompt_tokens":20,"completion_tokens":4}}).to_string();
                        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(3))
                    }
                    Err(_) => break,
                }
            }
        });
        let root = std::env::temp_dir().join(format!(
            "forme-core-b-{}-{}-{}",
            std::process::id(),
            now(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let config = ProductionRuntimeConfig {
            schema_version: p::SchemaVersion(1),
            owner: p::OwnerPrincipalRef("owner:core-b".into()),
            store_path: root.join("brain.db"),
            workspace: p::WorkspaceRef("workspace:core-b".into()),
            agent_profile: p::AgentProfileRef("agent:core-b".into()),
            models: vec![ModelRuntimeConfig {
                id: "model:core-b".into(),
                provider: "core-b-provider".into(),
                model: "core-b-model".into(),
                base_url: format!("http://{address}/v1"),
                credential_env: "FORME_CORE_B_TEST_KEY".into(),
                context_window: 32768,
                tool_use: false,
                strength: ModelStrength::Standard,
                input_microunits_per_million: 10,
                output_microunits_per_million: 20,
                requests_per_minute: 600,
                tokens_per_minute: 1_000_000,
                timeout_ms: 5000,
                fallback_group: None,
                default: true,
                tool_schema_versions: vec![],
            }],
            capabilities: CapabilityRuntimeConfig::default(),
            scheduler_enabled: false,
            scheduler: p::SchedulerConfig {
                schema_version: p::SchemaVersion(1),
                tick: p::DurationMs(1000),
                lease: p::DurationMs(30_000),
                max_claims_per_tick: 1,
            },
            cognition: CognitiveRuntimeConfig::default(),
            data: DataProtectionConfig {
                schema_version: p::SchemaVersion(1),
                artifact_root: root.join("artifacts"),
            },
            authority_lease: p::DurationMs(30_000),
            context_limits: forme_context::ContextHydrationLimits::default(),
        };
        Self {
            _lock: lock,
            config,
            calls,
            answer,
            stop,
            thread: Some(thread),
            root,
        }
    }
    pub fn build(&self) -> ProductionRuntime {
        ProductionRuntimeBuilder::from_config(self.config.clone())
            .unwrap()
            .build()
            .unwrap()
    }
    pub fn owner(&self) -> p::VerifiedPrincipal {
        p::VerifiedPrincipal(self.config.owner.0.clone())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
        // This exact, freshly created fixture directory belongs to this test.
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

fn read_request(stream: &mut TcpStream) {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 8192];
        let size = stream.read(&mut buffer).unwrap();
        if size == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..size]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length = headers
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|s| s.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                break;
            }
        }
    }
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
pub fn events(runtime: &ProductionRuntime) -> Vec<p::Event> {
    let store = &runtime.services().store;
    store
        .run_ids()
        .unwrap()
        .into_iter()
        .flat_map(|run| store.read_run(run).map(Result::unwrap))
        .collect()
}
pub fn knowledge(attribute: &str, impact: p::Impact) -> p::GoalKnowledge {
    p::GoalKnowledge {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:core-b".into()),
        allow_questions: true,
        requirements: vec![p::KnowledgeRequirement {
            schema_version: p::SchemaVersion(1),
            target: p::LearningTarget::UserAttribute(p::UserAttributeRef(attribute.into())),
            reason: p::ReasonRef("the goal's risk judgement depends on this preference".into()),
            impact,
            minimum_confidence: p::Confidence(0.8),
        }],
    }
}
pub fn ask(runtime: &ProductionRuntime) -> (p::ProposalRef, p::AskToLearnRecord) {
    events(runtime)
        .into_iter()
        .find_map(|event| match event.payload {
            p::EventPayload::ProactiveProposalEmitted(payload) => payload
                .learning
                .map(|record| (payload.proposal_ref, record)),
            _ => None,
        })
        .unwrap()
}
