//! C24's six frozen workflows, using the production composition and real stores.
//!
//! Run `cargo run -p forme-harness --bin forme-core-brain-d-golden --
//! --live --out <new-directory> [--repeats 2..8]`.
//! Only model configuration is inherited from FORME_MODEL_*; owner, capabilities,
//! files and SQLite are fresh runner-owned fixtures. Tool workflows require a
//! configured tool-capable profile with tool_schema_versions=[1]. The legacy
//! flat model config does not declare tool support and is not silently upgraded.
//!
//! The direct baseline answers the same factual question without tools, memory,
//! orchestration or fallback. Its answer is not evidence of a completed workflow.
//! Controlled tests cannot produce a LiveConfigured artifact through this module.
#![forbid(unsafe_code)]

use forme_approval::{ApprovalGrant, ApprovalRequest, GrantScope};
use forme_eval::{
    CoreBrainDArtifact, ProductAttempt, ProductEvidenceGrade, ProductFileEvidence,
    ProductGoldenCase, ProductRestartEvidence, ProductVerdict,
};
use forme_harness::{
    AgentHarness, CapabilityRuntimeConfig, CognitiveRuntimeConfig, CognitiveTickOutcome,
    DataProtectionConfig, ProductionRuntime, ProductionRuntimeBuilder, ProductionRuntimeConfig,
    ResumeInput,
};
use forme_models::{ModelOutput, ModelProfile, ModelRegistry, ModelRegistryConfig, ModelRequest};
use forme_protocol as p;
use forme_store::EventStore;
use serde::Serialize;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

type RunnerResult<T> = Result<T, &'static str>;
const QUESTION: &str =
    "Calculate 13 + 17 + 17. Reply with only the integer, without prose or formatting.";
const PROJECT: &str = "project:c24-sum";
const FILE_TARGET: &str = "files/result.txt";
const MAX_PUBLIC_BYTES: usize = 65_536;
const MAX_APPROVALS: usize = 24;
static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Options {
    out: PathBuf,
    repeats: usize,
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = OsString>) -> RunnerResult<Self> {
        let mut args = arguments.into_iter();
        let (mut live, mut out, mut repeats) = (false, None, None);
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--live") if !live => live = true,
                Some("--out") if out.is_none() => {
                    out = Some(PathBuf::from(
                        args.next().ok_or("missing_output_directory")?,
                    ));
                }
                Some("--repeats") if repeats.is_none() => {
                    let value = args.next().ok_or("missing_repeat_count")?;
                    repeats = Some(
                        value
                            .to_str()
                            .and_then(|v| v.parse::<usize>().ok())
                            .filter(|n| (2..=8).contains(n))
                            .ok_or("repeat_count_must_be_2_through_8")?,
                    );
                }
                _ => return Err("invalid_arguments"),
            }
        }
        if !live {
            return Err("explicit_live_flag_required");
        }
        let out = out
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or("output_directory_required")?;
        Ok(Self {
            out,
            repeats: repeats.unwrap_or(2),
        })
    }
}

pub fn entry() -> i32 {
    match run_command(std::env::args_os().skip(1)) {
        Ok(code) => code,
        Err(code) => {
            // Errors are fixed codes. In particular, do not format provider,
            // serde, filesystem or process errors here.
            eprintln!("C24 runner: FAIL ({code})");
            eprintln!(
                "usage: forme-core-brain-d-golden --live --out <new-directory> [--repeats 2..8]"
            );
            1
        }
    }
}

fn run_command(arguments: impl IntoIterator<Item = OsString>) -> RunnerResult<i32> {
    let options = Options::parse(arguments)?;
    let source = source_commit()?;
    // Atomic creation is the overwrite guard, including an existing empty dir.
    std::fs::create_dir(&options.out).map_err(|_| "output_directory_must_be_new")?;
    let models = match ModelRegistryConfig::from_environment() {
        Ok(config) => config,
        Err(_) => return emit_unavailable(&options.out, source),
    };
    let privacy = PrivacyGuard::for_models(&models);
    let registry = match ModelRegistry::from_config(models.clone()) {
        Ok(registry) => registry,
        Err(_) => return emit_unavailable(&options.out, source),
    };
    // Actual provider profile references go into evidence, never endpoints or
    // credential identifiers. Reject unsafe configured labels before any call.
    for reference in registry.profiles() {
        let model = registry.get(&reference).ok_or("configured_model_missing")?;
        if !privacy.permits(&model.profile().profile_ref())? {
            return emit_unavailable(&options.out, source);
        }
    }
    let artifact = run_suite(source, &models, &registry, options.repeats, &privacy)?;
    persist_artifact(&options.out, &artifact)
}

fn emit_unavailable(out: &Path, source: String) -> RunnerResult<i32> {
    let artifact =
        CoreBrainDArtifact::unavailable(source).map_err(|_| "unavailable_artifact_failed")?;
    persist_artifact(out, &artifact)
}

fn source_commit() -> RunnerResult<String> {
    let result = Command::new("git")
        .args(["-C", env!("CARGO_MANIFEST_DIR"), "rev-parse", "HEAD"])
        .output()
        .map_err(|_| "source_commit_unavailable")?;
    let source = String::from_utf8(result.stdout).map_err(|_| "source_commit_unavailable")?;
    let source = source.trim().to_owned();
    if !result.status.success()
        || !matches!(source.len(), 40 | 64)
        || !source.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("source_commit_unavailable");
    }
    Ok(source)
}

fn run_suite(
    source: String,
    models: &ModelRegistryConfig,
    registry: &ModelRegistry,
    repeats: usize,
    privacy: &PrivacyGuard,
) -> RunnerResult<CoreBrainDArtifact> {
    let mut artifact =
        CoreBrainDArtifact::unavailable(source).map_err(|_| "artifact_creation_failed")?;
    artifact.grade = evidence_grade(models);
    artifact.configuration_digest = Some(configuration_digest(models, repeats)?);
    for task in &mut artifact.tasks {
        for repeat in 0..repeats {
            // Alternate ordering to reduce a fixed warmup/order bias. Every
            // primary attempt still gets its own store, owner and file roots.
            let (forme, direct) = if repeat % 2 == 0 {
                let primary = primary_attempt(models, registry, task.case, repeat, privacy);
                (primary, direct_attempt(registry, privacy))
            } else {
                let baseline = direct_attempt(registry, privacy);
                (
                    primary_attempt(models, registry, task.case, repeat, privacy),
                    baseline,
                )
            };
            println!(
                "C24 {:?}: repeat {}/{} captured",
                task.case,
                repeat + 1,
                repeats
            );
            task.forme.push(forme);
            task.direct_model.push(direct);
        }
    }
    artifact.seal().map_err(|_| "artifact_seal_failed")?;
    Ok(artifact)
}

// This classifier is identical in the production CLI and its tests. A live
// flag cannot upgrade a local controlled server to live provider evidence.
fn evidence_grade(models: &ModelRegistryConfig) -> ProductEvidenceGrade {
    if models.profiles.is_empty()
        || models
            .profiles
            .iter()
            .any(|model| controlled_endpoint(&model.base_url))
    {
        ProductEvidenceGrade::Controlled
    } else {
        ProductEvidenceGrade::LiveConfigured
    }
}

fn controlled_endpoint(endpoint: &str) -> bool {
    let Some((scheme, rest)) = endpoint.split_once("://") else {
        return true;
    };
    if !matches!(scheme, "http" | "https") {
        return true;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Ambiguous URL authority syntax is not evidence of a remote provider.
    if authority.is_empty()
        || !authority.is_ascii()
        || authority.contains(['@', '%', '\\'])
        || authority
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
    {
        return true;
    }
    let default_port = if scheme == "https" { 443 } else { 80 };
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some((host, suffix)) = bracketed.split_once(']') else {
            return true;
        };
        let port = if suffix.is_empty() {
            default_port
        } else {
            let Some(port) = suffix.strip_prefix(':').and_then(|p| p.parse::<u16>().ok()) else {
                return true;
            };
            port
        };
        if host.parse::<std::net::Ipv6Addr>().is_err() {
            return true;
        }
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        let Ok(port) = port.parse::<u16>() else {
            return true;
        };
        (host, port)
    } else {
        (authority, default_port)
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || port == 0
        || host == "localhost"
        || host.ends_with(".localhost")
        || host == "invalid"
        || host.ends_with(".invalid")
    {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return local_address(ip);
    }
    // Reject legacy numeric IP spellings rather than let URL and DNS parsers
    // disagree about shorthand, octal or integer loopback addresses.
    if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') || host.starts_with("0x") {
        return true;
    }
    match (host.as_str(), port).to_socket_addrs() {
        Ok(mut addresses) => match addresses.next() {
            None => true,
            Some(first) => local_address(first.ip()) || addresses.any(|a| local_address(a.ip())),
        },
        Err(_) => true,
    }
}

fn local_address(ip: IpAddr) -> bool {
    // Private addresses are allowed: an operator may configure a real provider
    // behind a private gateway. Endpoint checks cannot attest that provenance.
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_unspecified(),
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip
                    .to_ipv4()
                    .is_some_and(|ip| ip.is_loopback() || ip.is_unspecified())
        }
    }
}

fn configuration_digest(
    models: &ModelRegistryConfig,
    repeats: usize,
) -> RunnerResult<p::SchemaDigest> {
    let public = models.profiles.iter().map(|m| {
        serde_json::json!({
            "id": m.id, "provider": m.provider, "model": m.model,
            "context_window": m.context_window, "tool_use": m.tool_use,
            "tool_schema_versions": m.tool_schema_versions, "strength": format!("{:?}", m.strength),
            "input_rate": m.input_microunits_per_million,
            "output_rate": m.output_microunits_per_million,
            "timeout_ms": m.timeout_ms, "fallback_group": m.fallback_group,
            "requests_per_minute": m.requests_per_minute, "tokens_per_minute": m.tokens_per_minute,
            "default": m.default
        })
    }).collect::<Vec<_>>();
    p::canonical_digest(&serde_json::json!({
        "fixture": "c24-six-workflows-v1", "models": public, "repeats": repeats,
        "question": QUESTION, "baseline": "direct-configured-default-factual-only",
        "baseline_max_output_tokens": 512, "formal_budget_units": 8,
        "file_backend": true, "tick_interval_ms": 1
    }))
    .map_err(|_| "configuration_digest_failed")
}

fn direct_attempt(registry: &ModelRegistry, privacy: &PrivacyGuard) -> ProductAttempt {
    let started = Instant::now();
    let mut attempt = ProductAttempt {
        ..ProductAttempt::empty()
    };
    let result = (|| -> RunnerResult<()> {
        let provider = registry
            .provider(registry.default_ref())
            .map_err(|_| "baseline_provider_unavailable")?;
        let profile = provider.profile();
        attempt.model_profiles.insert(profile.profile_ref());
        let mut request = ModelRequest::text(QUESTION);
        request.max_output_tokens = Some(512);
        // One concrete provider call, with no wrapper, runtime event fiction,
        // tools, retry, fallback, or manufactured file/workflow results.
        let response = provider
            .call(request)
            .map_err(|_| "baseline_provider_call_failed")?;
        attempt.usage = response.usage;
        attempt.cost_microunits = priced_usage(&profile, response.usage);
        let ModelOutput::Final(output) = response.output else {
            return Err("baseline_nonfinal_response");
        };
        attempt.output = Some(output);
        Ok(())
    })();
    if let Err(code) = result {
        attempt.unavailable = Some(code.into());
        attempt.cost_microunits = None;
    }
    attempt.latency_ms = elapsed_ms(started);
    protect_attempt(attempt, privacy)
}

fn primary_attempt(
    models: &ModelRegistryConfig,
    registry: &ModelRegistry,
    case: ProductGoldenCase,
    repeat: usize,
    privacy: &PrivacyGuard,
) -> ProductAttempt {
    let started = Instant::now();
    let mut attempt = ProductAttempt {
        ..ProductAttempt::empty()
    };
    let mut session = None;
    let result = (|| -> RunnerResult<()> {
        session = Some(GoldenSession::new(models, case, repeat)?);
        execute_case(
            session.as_mut().ok_or("runtime_missing")?,
            case,
            &mut attempt,
        )
    })();
    if let Err(code) = result {
        attempt.unavailable = Some(code.into());
    }
    if let Some(session) = &session {
        match session.events() {
            Ok(all_store_events) => {
                // Full, contiguous streams are mandatory: capture_events
                // filters public payloads and binds ALL positions via witnesses.
                if attempt.capture_events(&all_store_events).is_err() {
                    attempt.unavailable = Some("event_capture_failed".into());
                }
                attempt.usage = event_usage(&all_store_events);
                if attempt.unavailable.is_none() {
                    attempt.cost_microunits = event_cost(&all_store_events, registry);
                }
            }
            Err(code) => attempt.unavailable = Some(code.into()),
        }
    }
    if attempt.unavailable.is_some() {
        attempt.cost_microunits = None;
    }
    attempt.latency_ms = elapsed_ms(started);
    // Drop all runtime/store/provider handles before removing this owned root.
    drop(session);
    protect_attempt(attempt, privacy)
}

struct GoldenSession {
    // Field order ensures the runtime is dropped before the scratch directory.
    runtime: Option<ProductionRuntime>,
    scratch: Scratch,
    config: ProductionRuntimeConfig,
    saved_events: Vec<p::Event>,
}

impl GoldenSession {
    fn new(
        models: &ModelRegistryConfig,
        case: ProductGoldenCase,
        repeat: usize,
    ) -> RunnerResult<Self> {
        let scratch = Scratch::new()?;
        let files = scratch.path.join("files");
        std::fs::create_dir(&files).map_err(|_| "fixture_directory_failed")?;
        let mut capabilities = CapabilityRuntimeConfig {
            notification_enabled: false,
            ..CapabilityRuntimeConfig::default()
        };
        if case == ProductGoldenCase::ComplexTools {
            capabilities.file.enabled = true;
            // The lexical policy root and requested File path agree. The
            // backend resolves this root while cwd is the owned scratch root.
            capabilities.file.roots = vec!["files".into()];
        }
        let config = ProductionRuntimeConfig {
            schema_version: p::SchemaVersion(1),
            owner: p::OwnerPrincipalRef(format!("owner:c24:{case:?}:{repeat}")),
            store_path: scratch.path.join("brain.db"),
            workspace: p::WorkspaceRef("workspace:c24".into()),
            agent_profile: p::AgentProfileRef("agent:c24".into()),
            models: models.profiles.clone(),
            capabilities,
            scheduler_enabled: false,
            scheduler: p::SchedulerConfig {
                schema_version: p::SchemaVersion(1),
                tick: p::DurationMs(1),
                lease: p::DurationMs(30_000),
                max_claims_per_tick: 1,
            },
            // Real wall-clock ticks; a short fixture cadence avoids fabricated
            // future timestamps and long sleeps between checkpoint segments.
            cognition: CognitiveRuntimeConfig {
                tick_interval: p::DurationMs(1),
                ..CognitiveRuntimeConfig::default()
            },
            data: DataProtectionConfig {
                schema_version: p::SchemaVersion(1),
                artifact_root: files,
            },
            authority_lease: p::DurationMs(30_000),
            context_limits: forme_context::ContextHydrationLimits::default(),
        };
        let runtime = if case == ProductGoldenCase::ComplexTools {
            let _working_directory = WorkingDirectory::enter(&scratch.path)?;
            build_runtime(&config)?
        } else {
            build_runtime(&config)?
        };
        Ok(Self {
            runtime: Some(runtime),
            scratch,
            config,
            saved_events: Vec::new(),
        })
    }

    fn runtime(&self) -> RunnerResult<&ProductionRuntime> {
        self.runtime.as_ref().ok_or("runtime_unavailable")
    }

    fn owner(&self) -> p::VerifiedPrincipal {
        p::VerifiedPrincipal(self.config.owner.0.clone())
    }

    fn events(&self) -> RunnerResult<Vec<p::Event>> {
        match &self.runtime {
            Some(runtime) => all_events(runtime),
            None => Ok(self.saved_events.clone()),
        }
    }

    fn restart(&mut self) -> RunnerResult<ProductRestartEvidence> {
        let runtime = self.runtime()?;
        let before = restart_state(runtime)?;
        let owner_before = runtime.manifest().owner_digest.clone();
        self.saved_events = all_events(runtime)?;
        // No copied DB, in-memory substitute or retained Arc: close and reopen
        // the actual SQLite and its content-key companion from the same paths.
        drop(self.runtime.take());
        self.runtime = Some(build_runtime(&self.config)?);
        let after = self.runtime()?;
        Ok(ProductRestartEvidence {
            schema_version: p::SchemaVersion(1),
            owner_before,
            owner_after: after.manifest().owner_digest.clone(),
            state_before: before,
            state_after: restart_state(after)?,
        })
    }
}

fn build_runtime(config: &ProductionRuntimeConfig) -> RunnerResult<ProductionRuntime> {
    ProductionRuntimeBuilder::from_config(config.clone())
        .and_then(ProductionRuntimeBuilder::build)
        .map_err(|_| "production_runtime_unavailable")
}

fn execute_case(
    session: &mut GoldenSession,
    case: ProductGoldenCase,
    attempt: &mut ProductAttempt,
) -> RunnerResult<()> {
    match case {
        ProductGoldenCase::ColdDialogue => attempt.output = ask_sum(session.runtime()?, "cold")?,
        ProductGoldenCase::AskToLearn => {
            let runtime = session.runtime()?;
            runtime.register_learning_goal(
                &session.owner(), p::IdempotencyKey("c24-learning-goal".into()),
                p::GoalFrameRef("goal:c24-result-presentation".into()),
                p::GoalKnowledge {
                    schema_version: p::SchemaVersion(1), scope: p::Scope(session.config.workspace.0.clone()),
                    allow_questions: true,
                    requirements: vec![p::KnowledgeRequirement {
                        schema_version: p::SchemaVersion(1),
                        target: p::LearningTarget::UserAttribute(p::UserAttributeRef("c24-result-presentation".into())),
                        reason: p::ReasonRef("the owner's presentation preference determines how to report the arithmetic result".into()),
                        impact: p::Impact::High, minimum_confidence: p::Confidence(0.8),
                    }],
                },
            ).map_err(|_| "learning_goal_failed")?;
            tick(runtime)?;
            let proposal = all_events(runtime)?
                .into_iter()
                .find_map(|e| match e.payload {
                    p::EventPayload::ProactiveProposalEmitted(v) if v.learning.is_some() => {
                        Some(v.proposal_ref)
                    }
                    _ => None,
                })
                .ok_or("learning_proposal_unavailable")?;
            let candidate = runtime
                .answer_learning(
                    &session.owner(),
                    p::IdempotencyKey("c24-owner-answer".into()),
                    p::LearningReplyRecord {
                        schema_version: p::SchemaVersion(1),
                        proposal,
                        value: p::UserAttributeValueRef(
                            "content:c24-owner-prefers-exact-integer".into(),
                        ),
                    },
                )
                .map_err(|_| "learning_answer_failed")?;
            if runtime
                .services()
                .memory
                .user_candidate(&candidate)
                .is_none()
                || !runtime.services().cognition.stable_objects().is_empty()
            {
                return Err("learning_candidate_boundary_failed");
            }
            attempt.output = ask_sum(runtime, "learning-result")?;
        }
        ProductGoldenCase::ComplexTools => complex_file_task(session, attempt)?,
        ProductGoldenCase::MultiAgent => {
            // Record the owner's actual task turn through the formal ingress
            // before using the owner project controls. Child-only routes do
            // not currently carry the composition manifest in SessionBound.
            attempt.output = ask_sum(session.runtime()?, "multi-owner-turn")?;
            let spec = project(session, review_signature());
            setup_project(session, spec.clone())?;
            let route = plan_route(session, &spec, "initial")?;
            let record = execute_route(session, &route)?;
            attempt.output = route_output(&record);
        }
        ProductGoldenCase::LongWorkspace => {
            attempt.output = ask_sum(session.runtime()?, "workspace-owner-turn")?;
            let mut signature = review_signature();
            signature.partition = p::TaskPartition::Iterative;
            signature.extent = p::TaskExtent::LongRunning;
            let spec = project(session, signature);
            setup_project(session, spec)?;
            let first = supervised_route(session)?;
            let record = execute_route(session, &first)?;
            attempt.output = route_output(&record);
            if !record.closed
                || !record.checkpointed
                || record.status != p::WorkRouteStatus::Completed
            {
                return Ok(()); // Keep the actual failed segment for frozen selectors.
            }
            attempt.restart = Some(session.restart()?);
            let second = supervised_route(session)?;
            if second.route == first.route {
                return Err("second_segment_was_not_fresh");
            }
            attempt.output = route_output(&execute_route(session, &second)?);
        }
        ProductGoldenCase::FailureRecoordination => {
            let mut spec = project(session, single_signature());
            // This file is deliberately absent, no file capability is granted,
            // and no fixture code writes it. A final answer cannot satisfy it.
            spec.requirements.push(p::RouteRequirement::FileDigest {
                relative_path: "required-but-absent.txt".into(),
                digest: p::sha256_content_digest(b"independent-required-evidence\n").0,
            });
            setup_project(session, spec.clone())?;
            let failed =
                execute_route(session, &plan_route(session, &spec, "intentional-failure")?)?;
            attempt.output = route_output(&failed);
            if failed.status != p::WorkRouteStatus::Failed || !failed.closed || !failed.checkpointed
            {
                return Err("intentional_failed_route_not_observed");
            }
            let previous = p::canonical_digest(&spec).map_err(|_| "project_digest_failed")?;
            spec.requirements = vec![p::RouteRequirement::OutputContains("47".into())];
            spec.summary =
                format!("Recompute after the prior missing-file verification failure. {QUESTION}");
            session
                .runtime()?
                .revise_project(
                    &session.owner(),
                    p::IdempotencyKey("c24-revise-after-failure".into()),
                    spec,
                    previous,
                )
                .map_err(|_| "project_revision_failed")?;
            // The owner revision plus actual failure evidence enters the shared
            // supervision tick. It plans a fresh independently reviewed route.
            let revised = supervised_route(session)?;
            if revised.route == failed.declaration.route || revised.nodes.len() < 2 {
                return Err("failure_did_not_recoordinate");
            }
            attempt.output = route_output(&execute_route(session, &revised)?);
        }
    }
    Ok(())
}

fn ask_sum(runtime: &ProductionRuntime, key: &str) -> RunnerResult<Option<String>> {
    let run = submit_formal(runtime, key, QUESTION)?;
    runtime
        .harness()
        .output_text(run)
        .map_err(|_| "formal_sum_output_unavailable")
}

fn submit_formal(runtime: &ProductionRuntime, key: &str, input: &str) -> RunnerResult<p::RunId> {
    let run = runtime
        .harness()
        .submit_run(p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::UserTurn,
            session: p::SessionRef(format!("session:c24:{key}")),
            agent_profile: runtime.config().agent_profile.clone(),
            input: p::RunInput(input.into()),
            budget: Some(p::Budget("units:8".into())),
            idempotency_key: Some(p::IdempotencyKey(format!("c24:{key}"))),
        })
        .map_err(|_| "formal_sum_submission_failed")?;
    Ok(run)
}

fn complex_file_task(session: &GoldenSession, attempt: &mut ProductAttempt) -> RunnerResult<()> {
    // File actions use relative paths within this runner process's own fixture
    // directory. This keeps the actual approval request portable; no event is
    // rewritten or removed from the witness set to conceal an absolute path.
    // The guard restores cwd before GoldenSession can remove its scratch root.
    let _working_directory = WorkingDirectory::enter(&session.scratch.path)?;
    let runtime = session.runtime()?;
    let prompt = format!("{QUESTION} Before your final answer, use the granted File tool to write the computed integer followed by exactly one newline to relative path {FILE_TARGET} in the granted working directory. Use File Write with UTF-8 bytes, then return only the integer. Do not put file paths in the final answer.");
    let run = submit_formal(runtime, "complex-file", &prompt)?;
    for _ in 0..MAX_APPROVALS {
        let events = runtime
            .services()
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()
            .map_err(|_| "file_run_events_unavailable")?;
        if events
            .iter()
            .any(|e| e.kind == p::EventKind::ActionOutcomeUnknown)
        {
            return Err("file_outcome_unknown");
        }
        let requests = runtime
            .harness()
            .pending_approvals(p::SessionId("session:c24:complex-file".into()))
            .map_err(|_| "file_approvals_unavailable")?;
        if requests.is_empty() {
            attempt.output = runtime
                .harness()
                .output_text(run.clone())
                .map_err(|_| "file_result_unavailable")?;
            if let Some(file) =
                file_evidence(&session.config.data.artifact_root, &session.scratch.path)?
            {
                attempt.files.push(file);
            }
            return Ok(());
        }
        for request in requests {
            if !permitted_file_approval(&request, &events, FILE_TARGET) {
                return Err("unexpected_fixture_approval");
            }
            runtime
                .harness()
                .resume(
                    run.clone(),
                    ResumeInput::Approval(approval(&session.owner(), &request)),
                )
                .map_err(|_| "file_approval_resume_failed")?;
        }
    }
    Err("fixture_approval_bound_exceeded")
}

fn single_signature() -> p::TaskSignature {
    p::TaskSignature {
        schema_version: p::SchemaVersion(1),
        partition: p::TaskPartition::Whole,
        dependency: p::TaskDependency::Independent,
        verification: p::TaskVerification::SelfCheck,
        clarity: p::TaskClarity::Clear,
        extent: p::TaskExtent::Single,
        reversibility: p::TaskReversibility::LowRiskReversible,
        anchors: Vec::new(),
    }
}

fn review_signature() -> p::TaskSignature {
    p::TaskSignature {
        partition: p::TaskPartition::Sequential,
        dependency: p::TaskDependency::Interdependent,
        verification: p::TaskVerification::IndependentReview,
        extent: p::TaskExtent::MultiStage,
        ..single_signature()
    }
}

fn project(session: &GoldenSession, signature: p::TaskSignature) -> p::WorkspaceProject {
    p::WorkspaceProject {
        schema_version: p::SchemaVersion(1),
        reference: PROJECT.into(),
        workspace: session.config.workspace.clone(),
        goal: p::GoalRef("goal:c24-sum".into()),
        done: p::DoneContractRef("done:c24-sum".into()),
        summary: QUESTION.into(),
        signature,
        requirements: vec![p::RouteRequirement::OutputContains("47".into())],
        scope: p::Scope(session.config.workspace.0.clone()),
        capabilities: Vec::new(),
        model: p::ModelProfileRef(
            session
                .config
                .models
                .iter()
                .find(|m| m.default)
                .map(|m| m.id.clone())
                .unwrap_or_default(),
        ),
        budget_units: 96,
        node_units: 8,
        cadence_ms: 1,
        deadline: None,
    }
}

fn setup_project(session: &GoldenSession, project: p::WorkspaceProject) -> RunnerResult<()> {
    let runtime = session.runtime()?;
    let charter = p::WorkspaceCharterRecord {
        schema_version: p::SchemaVersion(1),
        workspace: session.config.workspace.clone(),
        version: 1,
        goals: Vec::new(),
        constraints: vec![p::Constraint("max-nodes:8".into())],
        prohibitions: Vec::new(),
        done_contract: None,
        review_cadence: Some(p::DurationMs(1)),
        actor: p::Actor::Owner,
        digest: p::SchemaDigest(String::new()),
    };
    let change = runtime
        .prepare_workspace_charter(&session.owner(), charter, 0)
        .map_err(|_| "charter_preparation_failed")?;
    runtime
        .apply_workspace_charter(
            &session.owner(),
            p::IdempotencyKey("c24-charter".into()),
            change,
        )
        .map_err(|_| "charter_application_failed")?;
    runtime
        .register_project(
            &session.owner(),
            p::IdempotencyKey("c24-project".into()),
            project,
        )
        .map_err(|_| "project_registration_failed")?;
    Ok(())
}

fn plan_route(
    session: &GoldenSession,
    spec: &p::WorkspaceProject,
    key: &str,
) -> RunnerResult<p::RouteDeclaration> {
    session
        .runtime()?
        .plan_project_route(
            &session.owner(),
            p::IdempotencyKey(format!("c24-plan:{key}")),
            PROJECT,
            spec.signature.clone(),
            p::canonical_digest(&(spec, key)).map_err(|_| "route_input_digest_failed")?,
            now(),
        )
        .map_err(|_| "route_planning_failed")
}

fn tick(runtime: &ProductionRuntime) -> RunnerResult<()> {
    match runtime
        .cognitive_tick_at(now())
        .map_err(|_| "supervision_tick_failed")?
    {
        CognitiveTickOutcome::Ran(report) if !report.failed() => Ok(()),
        _ => Err("supervision_tick_unavailable"),
    }
}

fn supervised_route(session: &GoldenSession) -> RunnerResult<p::RouteDeclaration> {
    let runtime = session.runtime()?;
    tick(runtime)?;
    let state = runtime
        .orchestration_state(&session.owner())
        .map_err(|_| "route_projection_failed")?;
    let mut pending = state.routes.values().filter(|r| !r.closed);
    let route = pending.next().ok_or("supervised_route_unavailable")?;
    if pending.next().is_some() {
        return Err("supervision_created_ambiguous_segments");
    }
    Ok(route.declaration.clone())
}

fn execute_route(
    session: &GoldenSession,
    route: &p::RouteDeclaration,
) -> RunnerResult<p::WorkRouteRecord> {
    let record = session
        .runtime()?
        .execute_project_route(&session.owner(), &route.route)
        .map_err(|_| "project_execution_failed")?;
    // Project cases have no granted tools. Only the isolated File workflow
    // approves actions; suspended or uncertain routes are never re-driven here.
    if record.status == p::WorkRouteStatus::OutcomeUnknown {
        return Err("route_outcome_unknown");
    }
    if !record.closed {
        return Err("project_route_did_not_close");
    }
    Ok(record)
}

fn permitted_file_approval(request: &ApprovalRequest, events: &[p::Event], target: &str) -> bool {
    let expected = p::ResourceRef(format!("path:{target}"));
    request.scope == p::Scope("workspace:c24".into())
        && request.affected_resources == vec![expected]
        && events.iter().rev().find_map(|e| match &e.payload {
            p::EventPayload::ToolCallProposed(v) => Some(&v.args),
            _ => None,
        }).and_then(|args| serde_json::from_value::<p::ActionParameters>(args.clone()).ok())
            .is_some_and(|params| matches!(params,
                p::ActionParameters::File { operation: p::FileOperation::Write, path, content: Some(content) }
                    if path == target && content.len() <= MAX_PUBLIC_BYTES))
}

fn approval(owner: &p::VerifiedPrincipal, request: &ApprovalRequest) -> ApprovalGrant {
    ApprovalGrant {
        schema_version: p::SchemaVersion(1),
        approval_id: request.approval_id.clone(),
        outcome: p::ApprovalOutcome::Granted,
        granted_scope: GrantScope::OneShot,
        approver: owner.clone(),
        bound_plan_digest: request.plan_digest.clone(),
        normalized_action_digest: request.normalized_action_digest.clone(),
        affected_resources: request.affected_resources.clone(),
        policy_version: request.policy_version,
        tool_schema_version: request.tool_schema_version,
        nonce: p::Nonce(format!("c24-once:{}", request.approval_id.0)),
        use_by: request.expires_at,
    }
}

fn route_output(record: &p::WorkRouteRecord) -> Option<String> {
    let node = record
        .declaration
        .nodes
        .last()
        .unwrap_or(&record.declaration.main);
    record
        .nodes
        .get(&node.id)
        .filter(|n| !n.summary.is_empty())
        .map(|n| n.summary.clone())
}

fn all_events(runtime: &ProductionRuntime) -> RunnerResult<Vec<p::Event>> {
    let store = &runtime.services().store;
    let mut events = Vec::new();
    for run in store
        .run_ids()
        .map_err(|_| "event_stream_inventory_failed")?
    {
        events.extend(
            store
                .read_run(run)
                .collect::<p::Result<Vec<_>>>()
                .map_err(|_| "event_stream_read_failed")?,
        );
    }
    events.sort_by(|a, b| {
        a.run_id
            .cmp(&b.run_id)
            .then(a.stream_seq.cmp(&b.stream_seq))
    });
    Ok(events)
}

fn restart_state(runtime: &ProductionRuntime) -> RunnerResult<p::SchemaDigest> {
    let services = runtime.services();
    p::canonical_digest(&(
        &runtime.manifest().store_identity,
        (
            &services.aggregates.owner,
            &services.aggregates.memory_cognition,
            &services.aggregates.intentions,
            &services.aggregates.charter,
            &services.aggregates.dashboard,
            &services.aggregates.data_lifecycle,
        ),
        services
            .store
            .workspace_charter(&runtime.config().workspace)
            .map_err(|_| "restart_charter_read_failed")?,
        runtime
            .orchestration_state(&p::VerifiedPrincipal(runtime.owner().0.clone()))
            .map_err(|_| "restart_projection_read_failed")?,
        all_events(runtime)?,
    ))
    .map_err(|_| "restart_state_digest_failed")
}

fn file_evidence(directory: &Path, scratch: &Path) -> RunnerResult<Option<ProductFileEvidence>> {
    let path = directory.join("result.txt");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("file_evidence_unreadable"),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_PUBLIC_BYTES as u64
        || !path.canonicalize().is_ok_and(|p| p.starts_with(scratch))
    {
        return Err("file_evidence_outside_bound");
    }
    let bytes = std::fs::read(&path).map_err(|_| "file_evidence_unreadable")?;
    Ok(Some(ProductFileEvidence {
        schema_version: p::SchemaVersion(1),
        relative_path: "result.txt".into(),
        digest: p::sha256_content_digest(&bytes),
        bytes,
    }))
}

fn event_usage(events: &[p::Event]) -> p::ModelUsage {
    events.iter().fold(
        p::ModelUsage {
            input_tokens: 0,
            output_tokens: 0,
        },
        |mut usage, e| {
            if let p::EventPayload::ModelCallFinished(v) = &e.payload {
                usage.input_tokens = usage.input_tokens.saturating_add(v.usage.input_tokens);
                usage.output_tokens = usage.output_tokens.saturating_add(v.usage.output_tokens);
            }
            usage
        },
    )
}

fn priced_usage(profile: &ModelProfile, usage: p::ModelUsage) -> Option<u64> {
    let rates = &profile.cost;
    // Legacy/default zero means unknown, not a confirmed free model.
    if rates.input_microunits_per_million == 0 || rates.output_microunits_per_million == 0 {
        return None;
    }
    let amount = u128::from(usage.input_tokens) * u128::from(rates.input_microunits_per_million)
        + u128::from(usage.output_tokens) * u128::from(rates.output_microunits_per_million);
    u64::try_from(amount.div_ceil(1_000_000)).ok()
}

fn event_cost(events: &[p::Event], registry: &ModelRegistry) -> Option<u64> {
    let mut pending = BTreeSet::new();
    let mut cost = 0_u64;
    let mut completed = 0_u64;
    for e in events {
        match &e.payload {
            p::EventPayload::ModelCallStarted(v) => {
                pending.insert(v.call_id.clone());
            }
            p::EventPayload::ModelCallFinished(v) => {
                if !pending.remove(&v.call_id)
                    || v.finish_reason.0.contains("error")
                    || v.finish_reason.0.contains("unknown")
                {
                    return None;
                }
                let profile = registry.profiles().into_iter().find_map(|reference| {
                    registry
                        .get(&reference)
                        .map(|m| m.profile())
                        .filter(|profile| profile.profile_ref() == v.model_profile)
                })?;
                cost = cost.checked_add(priced_usage(profile, v.usage)?)?;
                completed += 1;
            }
            _ => {}
        }
    }
    (completed > 0 && pending.is_empty()).then_some(cost)
}

struct PrivacyGuard {
    forbidden: Vec<String>,
}

impl PrivacyGuard {
    fn for_models(models: &ModelRegistryConfig) -> Self {
        let mut forbidden = models
            .profiles
            .iter()
            .flat_map(|m| {
                let mut terms = vec![m.base_url.clone(), m.credential_env.clone()];
                if let Ok(value) = std::env::var(&m.credential_env) {
                    if !value.is_empty() {
                        terms.push(value);
                    }
                }
                terms
            })
            .collect::<Vec<_>>();
        forbidden.extend(
            ["USERPROFILE", "HOME"]
                .into_iter()
                .filter_map(|name| std::env::var(name).ok())
                .filter(|v| v.len() >= 6),
        );
        Self { forbidden }
    }

    fn permits(&self, value: &impl Serialize) -> RunnerResult<bool> {
        let value = serde_json::to_value(value).map_err(|_| "privacy_encoding_failed")?;
        Ok(self.safe_value(&value))
    }

    fn safe_value(&self, value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(s) => self.safe_string(s),
            serde_json::Value::Array(values) => values.iter().all(|v| self.safe_value(v)),
            serde_json::Value::Object(values) => values
                .iter()
                .all(|(k, v)| self.safe_string(k) && self.safe_value(v)),
            _ => true,
        }
    }

    fn safe_string(&self, value: &str) -> bool {
        let lower = value.to_ascii_lowercase();
        ![
            "http://",
            "https://",
            "authorization:",
            "bearer ",
            "secret:",
            "credential:",
            "/home/",
            "/users/",
            "/private/",
            "/tmp/",
            "/var/tmp/",
            "\\\\",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
            && !value
                .as_bytes()
                .windows(3)
                .any(|v| v[0].is_ascii_alphabetic() && v[1] == b':' && matches!(v[2], b'/' | b'\\'))
            && !self.forbidden.iter().any(|term| value.contains(term))
    }
}

fn protect_attempt(attempt: ProductAttempt, privacy: &PrivacyGuard) -> ProductAttempt {
    let bounded = attempt
        .output
        .as_ref()
        .is_none_or(|s| s.len() <= MAX_PUBLIC_BYTES);
    // Files serialize bytes as integers, so also inspect their UTF-8 content.
    let files_safe = attempt
        .files
        .iter()
        .all(|file| std::str::from_utf8(&file.bytes).is_ok_and(|s| privacy.safe_string(s)));
    if bounded && files_safe && privacy.permits(&attempt).unwrap_or(false) {
        attempt
    } else {
        ProductAttempt {
            latency_ms: attempt.latency_ms,
            usage: attempt.usage,
            unavailable: Some("nonportable_evidence_withheld".into()),
            ..ProductAttempt::empty()
        }
    }
}

fn persist_artifact(out: &Path, artifact: &CoreBrainDArtifact) -> RunnerResult<i32> {
    write_new(
        out.join("product-golden.json"),
        &serde_json::to_vec_pretty(artifact).map_err(|_| "artifact_encoding_failed")?,
    )?;
    let score = match artifact.verify() {
        Ok(score) => score,
        Err(_) => {
            write_new(out.join("report.txt"), b"FAIL: artifact invariant or evidence verification failed. No live success is claimed.\n")?;
            println!("C24 runner: FAIL (artifact_verification_failed)");
            return Ok(1);
        }
    };
    write_new(
        out.join("score.json"),
        &serde_json::to_vec_pretty(&score).map_err(|_| "score_encoding_failed")?,
    )?;
    let report = format!(
        "C24: {:?}\nEvidence grade: {:?}\nSource commit: {}\n\nPrimary protocol: fresh production runtime/SQLite/File fixtures; all store stream positions witnessed. Multi-agent and long-workspace cases start with a real owner turn through formal ingress before owner project controls; route output determines their final answer. Child-only sessions currently do not inherit the composition manifest.\nBaseline: one direct call to the configured default provider for the same arithmetic question, without tools, memory, orchestration or fallback. It measures factual answer quality, not baseline workflow completion.\nOwner learning answers and file approvals are self-authored fixture-owner controls.\nProduction and test builds use the same grade classifier. Loopback, localhost, .invalid and unresolvable endpoints remain Controlled even with --live. Private IPs alone do not exclude a configured real provider. Endpoint classification checks current address resolution and cannot attest the operator-declared upstream model or proxy provenance. Raw endpoints are never exported.\nLatency is measured wall-clock time including primary runtime construction and restart, excluding scratch cleanup. Usage sums recorded finishes; failed-call usage may be unknown. Cost is a configured-rate estimate when both rates are known; failed calls and unknown prices remain None.\nThe short supervision cadence exercises two checkpointed segments across an actual close/reopen; it is not a longitudinal value study.\nThis artifact does not run CLI/Gateway release gates or isolate activation/memory/routing ablations, and is not a V1 closure declaration. Source commit identifies HEAD; this runner does not assert a clean working tree.\n",
        score.verdict, artifact.grade, artifact.source_commit
    );
    write_new(out.join("report.txt"), report.as_bytes())?;
    println!("C24 runner: {:?} ({:?})", score.verdict, artifact.grade);
    Ok(match score.verdict {
        ProductVerdict::Pass => 0,
        ProductVerdict::Fail => 1,
        ProductVerdict::Unverifiable => 2,
    })
}

fn write_new(path: PathBuf, bytes: &[u8]) -> RunnerResult<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "artifact_create_failed")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "artifact_write_failed")
}

struct Scratch {
    path: PathBuf,
}

struct WorkingDirectory {
    previous: PathBuf,
}

impl WorkingDirectory {
    fn enter(directory: &Path) -> RunnerResult<Self> {
        let previous =
            std::env::current_dir().map_err(|_| "fixture_working_directory_unavailable")?;
        std::env::set_current_dir(directory).map_err(|_| "fixture_working_directory_failed")?;
        Ok(Self { previous })
    }
}

impl Drop for WorkingDirectory {
    fn drop(&mut self) {
        if std::env::set_current_dir(&self.previous).is_err() {
            eprintln!("C24 runner: working_directory_restore_failed");
        }
    }
}

impl Scratch {
    fn new() -> RunnerResult<Self> {
        let parent = std::env::temp_dir()
            .canonicalize()
            .map_err(|_| "fixture_temp_unavailable")?;
        for _ in 0..16 {
            let seq = SCRATCH_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            let path = parent.join(format!("forme-c24-{}-{}-{seq}", std::process::id(), now()));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(Self {
                        path: path
                            .canonicalize()
                            .map_err(|_| "fixture_path_unavailable")?,
                    })
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err("fixture_temp_creation_failed"),
            }
        }
        Err("fixture_unique_directory_unavailable")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Only this exact successfully-created child of the temp directory is
        // removable. Revalidate aliases before any recursive deletion.
        let safe = std::env::temp_dir()
            .canonicalize()
            .ok()
            .is_some_and(|parent| {
                self.path.parent() == Some(parent.as_path())
                    && self
                        .path
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("forme-c24-"))
                    && std::fs::symlink_metadata(&self.path)
                        .is_ok_and(|m| !m.file_type().is_symlink())
                    && self.path.canonicalize().is_ok_and(|p| p == self.path)
            });
        if !safe || std::fs::remove_dir_all(&self.path).is_err() {
            eprintln!("C24 runner: fixture_cleanup_incomplete");
        }
    }
}

fn now() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
#[path = "product_golden_runner_tests.rs"]
mod tests;
