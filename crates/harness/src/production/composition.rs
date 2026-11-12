//! Single production composition and its startup diagnostics.
//! Ordering, authority fences and recovery are shared by every builder path.
use super::*;

pub(super) struct ManagedCompositionInputs {
    pub(super) managed_mode: bool,
    pub(super) managed_mcp: Vec<p::ProductMcpServerConfig>,
    pub(super) managed_plugins: Option<crate::managed_config::ManagedPluginSources>,
    pub(super) managed_channels: Option<crate::managed_config::ManagedChannels>,
    pub(super) worker_binding: Option<forme_execution::installed::ManagedWorkerBinding>,
    pub(super) mcp_worker_binding: Option<forme_execution::installed::ManagedWorkerBinding>,
    pub(super) desktop_placement: Option<forme_execution::installed::InstalledDesktopProvisioning>,
    pub(super) worker_file_roots: BTreeMap<String, PathBuf>,
    pub(super) publisher_keyring: Arc<forme_capabilities::InMemoryPublisherKeyring>,
}

pub(super) fn compose(
    config: ProductionRuntimeConfig,
    store: SqliteEventStore,
    credential_files: Option<builder::ManagedModelSources>,
    access: forme_store::StoreAccessGuard,
    managed: ManagedCompositionInputs,
) -> p::Result<ProductionRuntime> {
    let ManagedCompositionInputs {
        managed_mode,
        managed_mcp,
        managed_plugins,
        managed_channels,
        worker_binding,
        mcp_worker_binding,
        desktop_placement,
        worker_file_roots,
        publisher_keyring,
    } = managed;
    if !store.is_file_backed()? {
        return Err(p::Error(
            "the production brain requires a durable file-backed authority store".into(),
        ));
    }
    let owner_digest = p::sha256_content_digest(config.owner.0.as_bytes());
    let process_nonce = process_nonce();
    let now = now_ms();
    let authority = store.claim_brain_authority(BrainAuthorityClaim {
        owner_digest: owner_digest.clone(),
        process_nonce,
        now_ms: now,
        lease_ttl: config.authority_lease,
    })?;
    let guard = store.install_authority_lease_guard(&authority, config.authority_lease)?;
    guard.retain_store_access(access)?;
    let aggregates = BrainAggregateRefs::from_binding(&authority, config.owner.clone())?;
    let store = Arc::new(store);

    let (model_registry, unavailable_models) =
        builder::bind_models(&config.models, credential_files)?;
    let model_registry = Arc::new(model_registry);
    let default_ref = model_registry.default_ref().clone();
    let registered = model_registry
        .get(&default_ref)
        .ok_or_else(|| p::Error("model registry default profile disappeared".into()))?;
    let profile = registered.profile().clone();
    let provider = registered.provider();

    let memory = forme_cognition::open_persistent_memory(
        store.clone(),
        aggregates.memory_cognition.clone(),
    )?;
    let cognition = Arc::new(
        CognitiveRuntime::open(
            store.clone(),
            memory.clone(),
            aggregates.memory_cognition.clone(),
        )?
        .with_owner_evidence_lookup(),
    );
    let temporal_models = Arc::new(forme_cognition::TemporalModels::new(
        memory.clone(),
        cognition.clone(),
    ));
    let intentions = forme_cognition::event_sourced_intention_services_with_quarantine(
        store.clone(),
        aggregates.intentions.clone(),
        store.quarantined_intentions()?,
    )?;

    let mcp_binding = mcp_worker_binding.as_ref().or_else(|| {
        worker_binding
            .as_ref()
            .filter(|b| b.supports(p::BackendKind::Mcp))
    });
    let mut capability_assembly = assemble_capabilities(
        &config,
        &config.workspace,
        authority.created_at,
        store
            .content_protection()?
            .ok_or_else(|| p::Error("production content protection is not installed".into()))?,
        store.as_ref().clone(),
        managed_mode,
        managed_workers::Bindings {
            worker: worker_binding.as_ref(),
            mcp: mcp_binding,
            desktop: desktop_placement.as_ref().map(|p| p.binding()),
        },
    )?;
    channels::register(
        &mut capability_assembly,
        managed_channels.as_ref(),
        &config,
        store.as_ref().clone(),
    )?;
    let capabilities = capability_assembly.capabilities.clone();
    let skills = capability_assembly.skills.clone();
    let mcp = capability_assembly.mcp.clone();
    let plugins = capability_assembly.plugins.clone();
    let app_api = capability_assembly.app_api.clone();
    let backends = capability_assembly.backends.clone();

    let mut harness_config = HarnessConfig::for_model(&profile);
    harness_config.model_profile = default_ref.clone();
    harness_config.workspace = config.workspace.clone();
    harness_config.toolset_ref = p::ToolsetRef("toolset:forme-production".into());
    let governance = production_governance(&config.workspace, &capability_assembly);
    let m3 = crate::brain_evolution::production_m3(
        (*store).clone(),
        p::VerifiedPrincipal(config.owner.0.clone()),
        aggregates.memory_cognition.clone(),
        crate::M3RuntimeCompatibility {
            schema_version: p::SchemaVersion(1),
            event_schema: p::SchemaVersion(1),
            tool_schema: p::canonical_digest(&capability_assembly.tool_schemas)?,
            backend_schema: p::canonical_digest(
                &capability_assembly
                    .bindings
                    .values()
                    .map(|b| (&b.id, b.backend))
                    .collect::<Vec<_>>(),
            )?,
        },
    )?;

    let context_builder = Arc::new(LayeredContextBuilder::default());
    let context_source = Arc::new(context_source::ProductionContextSliceSource::new(
        store.clone(),
        cognition.clone(),
        aggregates.clone(),
        config.workspace.clone(),
    ));
    let formal_bindings = capability_assembly
        .bindings
        .values()
        .map(|binding| {
            (
                p::ToolRef(binding.id.0.clone()),
                crate::formal_tools::FormalToolBinding {
                    capability: binding.id.clone(),
                    backend: binding.backend,
                    scope: binding.scope.clone(),
                    permissions: binding.permissions.clone(),
                    risk: binding.risk,
                },
            )
        })
        .collect();
    let snapshots = Arc::new(crate::run_tools::RunToolSnapshots {
        store: store.clone(),
        owner: p::VerifiedPrincipal(config.owner.0.clone()),
        composition: std::sync::OnceLock::new(),
        access: Mutex::new(()),
        bindings: formal_bindings,
        envelope: capability_assembly.resolve_context.envelope.clone(),
        registry: mcp.clone(),
        managed_source: if managed_mode && capability_assembly.mcp_backend.is_some() {
            managed_workers::catalog_source(&config, mcp_binding, store.as_ref().clone())
        } else {
            None
        },
        plugins: managed_plugins
            .clone()
            .map(|sources| crate::run_tools::ManagedPluginTools {
                sources,
                registry: plugins.clone(),
                capabilities: capabilities.clone(),
                resolve: capability_assembly.resolve_context.clone(),
                generations: plugins
                    .statuses()
                    .into_iter()
                    .map(|s| (s.plugin, s.generation))
                    .collect(),
            }),
        managed_configs: if managed_mode {
            managed_mcp.clone()
        } else {
            Vec::new()
        },
    });
    let catalog = ProductionContextCatalogSource {
        managed_plugins: managed_plugins.clone(),
        snapshots: snapshots.clone(),
        store: store.clone(),
        capabilities: capabilities.clone(),
        skills: skills.clone(),
        mcp: mcp.clone(),
        plugin_skill_ids: capability_assembly.plugin_skill_ids.clone(),
        workspace: config.workspace.clone(),
        resolve_context: capability_assembly.resolve_context.clone(),
        rules: production_rules(),
        static_tool_schemas: capability_assembly.tool_schemas.clone(),
        app_api_schemas: capability_assembly.app_api_schemas.clone(),
    };
    let hydrator = Arc::new(
        forme_context::EventBackedContextHydrator::new_with_catalog_source(
            store.clone(),
            memory.clone(),
            context_source,
            Arc::new(catalog),
            config.context_limits,
        )?,
    );
    let hydrator_for_factory = hydrator.clone();
    let coordination: Arc<dyn CoordinationReasoner + Send + Sync> =
        Arc::new(RuleBasedCoordinationReasoner);
    let orchestration = Arc::new(SeedOrchestrationLibrary::default());
    let proactivity = Arc::new(
        M0ProactivityEngine::new(ProactivityConfig::default())
            .with_intention_store(intentions.proactive.clone()),
    );
    let verifier: Arc<dyn Verifier + Send + Sync> = Arc::new(DeterministicVerifier);

    let ecosystem = Arc::new(crate::EcosystemHarnessRuntime::new_with_keyring(
        (*store).clone(),
        p::VerifiedPrincipal(config.owner.0.clone()),
        crate::m5::default_admission_policy(),
        capabilities.clone(),
        publisher_keyring,
    )?);
    let mut harness = ReactiveHarness::new_without_recovery_and_ecosystem(
        (*store).clone(),
        provider,
        backends.clone(),
        governance,
        harness_config.clone(),
        ecosystem,
    )?
    .with_model_registry(model_registry.clone())
    .with_m3_domain_runtime(m3)
    .with_context_builder(context_builder.clone())
    .with_context_factory(move |request, run, config| {
        hydrator_for_factory.hydrate(&ContextHydrationRequest {
            schema_version: p::SchemaVersion(1),
            run: run.clone(),
            session: p::SessionId(request.session.0.clone()),
            workspace: config.workspace.clone(),
            scope: p::Scope(config.workspace.0.clone()),
            brain_call: true,
            selected_skills: Vec::new(),
            model: config.model_profile.clone(),
            budget: config.context_budget,
        })
    })
    .with_skill_registry(skills.clone())
    .with_competence_gate(Arc::new(EvidenceCompetenceGate::default()))
    .with_coordination(coordination.clone())
    .with_proactivity(proactivity.clone())
    .with_verifier(verifier.clone())
    .with_capability_rechecker(capability_assembly.rechecker.clone())
    .with_authority_guard(guard.clone());

    harness.desktop_backend = capability_assembly.desktop_backend.clone();
    harness.managed_skill_sources = managed_plugins;
    if let (Some(binding), Some(backend)) = (mcp_binding, capability_assembly.mcp_backend.clone()) {
        harness.owner_discovery =
            crate::owner_discovery::DiscoveryRuntime::new(managed_mcp, binding, backend).ok();
    }
    harness.formal_tool_bindings = snapshots.bindings.clone();
    harness.tool_snapshots = Some(snapshots.clone());

    // Startup recovery is a durable mutation boundary: the production
    // authority fence must already be installed before an incomplete action
    // can be converted into ActionOutcomeUnknown/RunWaiting events.
    harness.resume_tick_sequence()?;
    // Tick lease recovery: a window a crashed process left claimed becomes
    // abandoned, so the next pass retries it with its recorded inputs.
    store.abandon_expired_tick(&authority.owner_digest, now_ms())?;

    if config.scheduler_enabled {
        harness = harness.with_scheduler(intentions.scheduled.clone(), config.scheduler.clone())?;
    }

    let federation = harness.federation_runtime();
    let ecosystem = harness.ecosystem_runtime();
    let mut service_states = core_service_states(&config, &profile, &model_registry);
    service_states.extend(
        model_registry
            .profiles()
            .into_iter()
            .map(|reference| RuntimeServiceState::ready(format!("model:{}", reference.0))),
    );
    service_states.extend(unavailable_models.iter().map(|unavailable| {
        RuntimeServiceState::unavailable(
            format!("model:{}", unavailable.profile.0),
            RuntimeServiceStatus::Disabled,
            unavailable.reason.clone(),
        )
    }));
    service_states.extend(capability_assembly.service_states.clone());
    let doctor = ConfigDoctorSnapshot::from_states(&service_states);
    append_doctor_event(&store, &aggregates, &doctor)?;
    let capability_snapshot = capability_snapshot_digest(&capability_assembly, &service_states);
    let capability_snapshot = match worker_binding.as_ref() {
        Some(binding) => p::canonical_digest(&(
            "forme-managed-worker-composition-v1",
            capability_snapshot,
            binding.schema_version,
            &binding.endpoint,
            binding.authority_epoch,
            &binding.sandbox_profile_ref,
            &binding.backend_kinds,
        ))?,
        None => capability_snapshot,
    };
    let capability_snapshot = match desktop_placement.as_ref() {
        Some(placement) => {
            let binding = placement.binding();
            p::canonical_digest(&(
                "forme-installed-desktop-placement-v1",
                capability_snapshot,
                binding.schema_version,
                &binding.endpoint,
                binding.authority_epoch,
                &binding.sandbox_profile_ref,
                &binding.backend_kinds,
                placement.scope(),
                placement.surface_refs(),
            ))?
        }
        None => capability_snapshot,
    };
    let capability_snapshot = if worker_file_roots.is_empty() {
        capability_snapshot
    } else {
        p::canonical_digest(&(
            "forme-installed-file-placement-v1",
            capability_snapshot,
            worker_file_roots,
        ))?
    };
    let capability_snapshot = match model_registry.role_routing_digest()? {
        Some(roles) => p::canonical_digest(&(
            "forme-capabilities-with-model-roles-v1",
            capability_snapshot,
            roles,
        ))?,
        None => capability_snapshot,
    };
    let capability_snapshot = match mcp_binding {
        Some(binding) => p::canonical_digest(&(
            "forme-installed-mcp-placement-v1",
            capability_snapshot,
            binding,
            &snapshots.managed_configs,
        ))?,
        None => capability_snapshot,
    };
    let manifest = CompositionManifest::new(CompositionManifestParts {
        owner_digest,
        store_identity: authority.store_identity.clone(),
        aggregate_refs: aggregates.clone(),
        workspace: config.workspace.clone(),
        agent_profile: config.agent_profile.clone(),
        policy_profile: harness_config.policy_profile.clone(),
        toolset_ref: harness_config.toolset_ref.clone(),
        model_profiles: model_registry.profiles(),
        capability_snapshot,
        service_states,
    });
    manifest.validate()?;
    snapshots
        .composition
        .set(manifest.digest.clone())
        .map_err(|_| p::Error("tool snapshot composition was already bound".into()))?;
    let services = Arc::new(BrainServices {
        data_access: std::sync::RwLock::new(()),
        store,
        authority,
        authority_guard: guard,
        aggregates,
        model_registry,
        memory,
        cognition,
        temporal_models,
        intentions,
        capabilities,
        skills,
        mcp,
        plugins,
        app_api,
        backends,
        coordination,
        orchestration,
        proactivity,
        context_builder,
        context_hydrator: hydrator,
        verifier,
        federation,
        ecosystem,
        doctor,
    });
    // The heartbeat lives with the harness so compatibility callers that
    // unwrap the runtime into a bare harness keep the lease alive as well.
    harness = harness
        .with_brain_services(services.clone())
        .with_composition_manifest(manifest.digest.clone())
        .with_lease_heartbeat(LeaseHeartbeat::start(
            services.authority_guard.clone(),
            config.authority_lease,
        )?);
    harness.recover_unknown_outcomes()?;
    harness.recover_owner_discoveries()?;
    let harness = Arc::new(harness);
    // The production guard is also held by the harness, so the lease remains
    // alive for compatibility callers that unwrap the runtime into a harness.
    let runtime = ProductionRuntime {
        data_operation_root: None,
        harness,
        services,
        manifest,
        config,
        tick_attempts: AtomicU64::new(1),
        learning_commands: Mutex::new(()),
        orchestration_commands: Mutex::new(()),
        orchestration_active: Mutex::new(BTreeSet::new()),
    };
    // Complete observations lost between a terminal append and candidate creation.
    // Deterministic candidate identities make startup backfill idempotent.
    for run in runtime.services().store.run_ids()? {
        let events = runtime
            .services()
            .store
            .read_run(run)
            .collect::<p::Result<Vec<_>>>()?;
        runtime
            .services()
            .temporal_models
            .observe_terminal_run(&events)?;
    }
    for (route, record) in runtime.orchestration_snapshot()?.routes {
        if !record.closed {
            runtime.recover_route_nodes(&route)?;
        }
    }
    Ok(runtime)
}

fn core_service_states(
    config: &ProductionRuntimeConfig,
    profile: &ModelProfile,
    registry: &ModelRegistry,
) -> Vec<RuntimeServiceState> {
    let mut states = vec![
        RuntimeServiceState::ready("authority-store"),
        RuntimeServiceState::ready("owner-brain-aggregates"),
        RuntimeServiceState::ready("model-registry"),
        RuntimeServiceState::ready("event-sourced-memory"),
        RuntimeServiceState::ready("cognitive-runtime"),
        RuntimeServiceState::ready("coordination-reasoner"),
        RuntimeServiceState::ready("orchestration-library"),
        RuntimeServiceState::ready("proactivity-engine"),
        RuntimeServiceState::ready("context-hydrator"),
        RuntimeServiceState::ready("execution-backend-registry"),
        RuntimeServiceState::ready("evaluation-verifier"),
        RuntimeServiceState::ready("federation-runtime"),
        RuntimeServiceState::ready("ecosystem-runtime"),
    ];
    if config.scheduler_enabled {
        states.push(RuntimeServiceState::ready("scheduler"));
    } else {
        states.push(RuntimeServiceState::unavailable(
            "scheduler",
            RuntimeServiceStatus::Disabled,
            "scheduler disabled by explicit configuration",
        ));
    }
    if config.cognition.tick_enabled {
        states.push(RuntimeServiceState::ready("cognitive-tick"));
    } else {
        states.push(RuntimeServiceState::unavailable(
            "cognitive-tick",
            RuntimeServiceStatus::Disabled,
            "cognitive tick disabled by explicit configuration",
        ));
    }
    if registry.is_empty() || profile.model.trim().is_empty() {
        states.push(RuntimeServiceState::unavailable(
            "model-selection",
            RuntimeServiceStatus::Blocking,
            "the default model profile is not selectable",
        ));
    }
    states
}

fn capability_snapshot_digest(
    assembly: &CapabilityAssembly,
    service_states: &[RuntimeServiceState],
) -> p::SchemaDigest {
    let bindings = assembly
        .bindings
        .values()
        .map(|binding| {
            format!(
                "{}|{:?}|{:?}|{}|{:?}|{}",
                binding.id.0,
                binding.capability,
                binding.backend,
                binding
                    .permissions
                    .iter()
                    .map(|permission| permission.0.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                binding.risk,
                binding.scope.0
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let states = service_states
        .iter()
        .map(|state| {
            format!(
                "{}|{:?}|{}",
                state.name,
                state.status,
                state
                    .reason
                    .as_ref()
                    .map(|reason| reason.0.as_str())
                    .unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let value = format!("forme-capabilities-v2\n{bindings}\n{states}");
    p::sha256_content_digest(value.as_bytes())
}

fn append_doctor_event(
    store: &Arc<SqliteEventStore>,
    aggregates: &BrainAggregateRefs,
    doctor: &ConfigDoctorSnapshot,
) -> p::Result<()> {
    let marker = p::ConfigFindingRef(format!("runtime-doctor:{}", doctor.digest.0));
    let existing = store
        .read_run(aggregates.memory_cognition.clone())
        .collect::<p::Result<Vec<_>>>()?
        .iter()
        .any(|event| {
            matches!(
                &event.payload,
                p::EventPayload::ConfigDoctorReport(payload) if payload.findings.contains(&marker)
            )
        });
    if existing {
        return Ok(());
    }
    let checks = doctor
        .checks
        .iter()
        .map(|state| match state.name.as_str() {
            "model-registry" | "model-selection" => p::ConfigCheck::Provider,
            "authority-store" | "owner-brain-aggregates" => p::ConfigCheck::Context,
            "context-hydrator" => p::ConfigCheck::Context,
            "mcp-registry" => p::ConfigCheck::Mcp,
            "plugin-registry" => p::ConfigCheck::Plugin,
            "scheduler" => p::ConfigCheck::Scheduler,
            "notification-backend" => p::ConfigCheck::Notification,
            "execution-backend-registry" => p::ConfigCheck::Capability,
            _ => p::ConfigCheck::ReleaseAudit,
        })
        .collect::<Vec<_>>();
    let event = p::Event::new(
        p::EventId(format!("runtime-doctor:{}", doctor.digest.0)),
        aggregates.memory_cognition.clone(),
        None,
        p::EventPayload::ConfigDoctorReport(p::ConfigDoctorReportPayload {
            checks,
            findings: vec![marker],
        }),
        p::SchemaVersion(1),
        now_ms(),
        trusted_provenance(),
    );
    store.append(event).map(|_| ())
}
