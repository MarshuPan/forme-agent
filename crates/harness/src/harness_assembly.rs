//! Harness constructors, production entry points and explicit service attachment.
use super::*;

impl ReactiveHarness {
    pub fn new(
        store: SqliteEventStore,
        model: Arc<dyn ModelProvider>,
        backends: Arc<ExecutionBackendRegistry>,
        governance: GovernanceConfig,
        config: HarnessConfig,
    ) -> p::Result<Self> {
        let harness = Self::new_without_recovery(store, model, backends, governance, config)?;
        harness.recover_unknown_outcomes()?;
        Ok(harness)
    }

    /// Builds the harness without touching durable recovery state.
    ///
    /// Production composition uses this seam to install the authority lease
    /// fence before startup recovery can append an `ActionOutcomeUnknown` or
    /// `RunWaiting` event. The public `new` constructor retains its legacy
    /// test-fixture behavior and performs recovery immediately.
    pub(crate) fn new_without_recovery(
        store: SqliteEventStore,
        model: Arc<dyn ModelProvider>,
        backends: Arc<ExecutionBackendRegistry>,
        governance: GovernanceConfig,
        config: HarnessConfig,
    ) -> p::Result<Self> {
        let ecosystem = Arc::new(EcosystemHarnessRuntime::local(store.clone()));
        Self::new_without_recovery_and_ecosystem(
            store, model, backends, governance, config, ecosystem,
        )
    }

    pub(crate) fn new_without_recovery_and_ecosystem(
        store: SqliteEventStore,
        model: Arc<dyn ModelProvider>,
        backends: Arc<ExecutionBackendRegistry>,
        governance: GovernanceConfig,
        config: HarnessConfig,
        ecosystem: Arc<EcosystemHarnessRuntime>,
    ) -> p::Result<Self> {
        config.validate()?;
        model.profile().validate()?;
        let context_builder: Arc<dyn ContextBuilder + Send + Sync> =
            Arc::new(LayeredContextBuilder::default());
        let loop_engine = ReactiveLoopEngine::new(context_builder.clone(), model.clone());
        let evolution_control = Arc::new(M3EvolutionHarness::new(store.clone()));
        let federation = Arc::new(FederatedHarnessRuntime::new(store.clone()));
        let harness = Self {
            store,
            authority_guard: None,
            configured_owner: None,
            brain_services: None,
            composition_manifest: None,
            lease_heartbeat: None,
            ingress_authority: IngressAuthority::default(),
            model,
            model_registry: None,
            context_builder,
            loop_engine,
            backends,
            policy: DefaultPolicyEngine,
            governance,
            config,
            context_factory: Arc::new(default_context),
            competence: Arc::new(FixedCompetenceGate::new(
                cognition::InterventionLevel::L5HighImpact,
            )),
            competence_snapshot: cognition::CompetenceInputs::default(),
            proactivity: Arc::new(NoopProactivity),
            verifier: Arc::new(eval::DeterministicVerifier),
            coordination: None,
            capability_rechecker: None,
            desktop_backend: None,
            skill_registry: None,
            skill_runtime_lock: Mutex::new(()),
            runs: Mutex::new(BTreeMap::new()),
            child_harnesses: Mutex::new(BTreeMap::new()),
            charter_workspace_override: None,
            formal_tool_bindings: BTreeMap::new(),
            project_directory: None,
            idempotency: Mutex::new(BTreeMap::new()),
            session_locks: Mutex::new(BTreeMap::new()),
            active_sessions: Mutex::new(BTreeSet::new()),
            run_sequence: AtomicU64::new(1),
            event_sequence: Arc::new(AtomicU64::new(1)),
            tick_sequence: AtomicU64::new(1),
            tick: TickScheduler,
            scheduler: None,
            proposal_origins: Mutex::new(BTreeMap::new()),
            suppressed_origins: Mutex::new(BTreeSet::new()),
            manual_evals: eval::ManualEvalArchive::default(),
            evolution_seed: None,
            evolution_control,
            m3_domains: None,
            m3_scope_override: None,
            federation,
            ecosystem,
        };
        Ok(harness)
    }

    pub fn federation_runtime(&self) -> Arc<FederatedHarnessRuntime> {
        self.federation.clone()
    }

    /// Attaches the production authority fence. Test-only constructors remain
    /// unfenced unless a production runtime explicitly supplies this guard.
    pub fn with_authority_guard(mut self, guard: forme_store::AuthorityLeaseGuard) -> Self {
        self.authority_guard = Some(guard);
        self
    }

    /// Attaches the single production service bundle used by all entry
    /// surfaces. The bundle is intentionally optional for legacy test
    /// harnesses constructed with `new`.
    pub fn with_brain_services(mut self, services: Arc<BrainServices>) -> Self {
        self.configured_owner = Some(p::VerifiedPrincipal(services.aggregates.owner.0.clone()));
        self.brain_services = Some(services);
        self
    }

    /// Binds the production composition manifest so every run bound by this
    /// harness records which composition served it (architecture/09 §2.4).
    pub(crate) fn with_composition_manifest(mut self, digest: p::SchemaDigest) -> Self {
        self.composition_manifest = Some(digest);
        self
    }

    pub(crate) fn with_lease_heartbeat(mut self, heartbeat: production::LeaseHeartbeat) -> Self {
        self.lease_heartbeat = Some(heartbeat);
        self
    }

    pub fn composition_manifest(&self) -> Option<&p::SchemaDigest> {
        self.composition_manifest.as_ref()
    }

    pub fn brain_services(&self) -> Option<Arc<BrainServices>> {
        self.brain_services.clone()
    }

    pub fn has_production_composition(&self) -> bool {
        self.authority_guard.is_some()
            && self.configured_owner.is_some()
            && self.brain_services.is_some()
    }

    pub fn ecosystem_runtime(&self) -> Arc<EcosystemHarnessRuntime> {
        self.ecosystem.clone()
    }

    pub fn from_environment(store: SqliteEventStore) -> p::Result<Self> {
        ProductionRuntimeBuilder::from_environment()?
            .build_with_store(store)?
            .into_harness()
    }

    pub fn from_environment_local() -> p::Result<Self> {
        ProductionRuntimeBuilder::from_environment()?
            .build()?
            .into_harness()
    }

    pub fn with_context_builder(mut self, builder: Arc<dyn ContextBuilder + Send + Sync>) -> Self {
        self.context_builder = builder.clone();
        self.loop_engine = ReactiveLoopEngine::new(builder, self.model.clone());
        self
    }

    /// Installs the production registry used for attempt-level model binding.
    /// Test harnesses may omit it and retain their explicitly supplied model.
    pub fn with_model_registry(mut self, registry: Arc<ModelRegistry>) -> Self {
        self.model_registry = Some(registry);
        self
    }

    pub fn with_context_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn(&p::RunRequest, &p::RunId, &HarnessConfig) -> p::Result<forme_context::RunCtx>
            + Send
            + Sync
            + 'static,
    {
        self.context_factory = Arc::new(factory);
        self
    }

    pub fn with_capability_rechecker(
        mut self,
        rechecker: Arc<dyn ExecutionCapabilityRechecker + Send + Sync>,
    ) -> Self {
        self.capability_rechecker = Some(rechecker);
        self
    }

    pub fn with_skill_registry(mut self, registry: Arc<InMemorySkillRegistry>) -> Self {
        self.skill_registry = Some(registry);
        self
    }

    pub fn with_competence_gate(
        mut self,
        gate: Arc<dyn cognition::CompetenceGate + Send + Sync>,
    ) -> Self {
        self.competence = gate;
        self
    }

    pub fn with_competence_snapshot(
        mut self,
        snapshot: cognition::CompetenceInputs,
    ) -> p::Result<Self> {
        validate_competence_snapshot(&snapshot)?;
        self.competence_snapshot = snapshot;
        Ok(self)
    }

    pub fn with_proactivity(
        mut self,
        engine: Arc<dyn cognition::ProactivityEngine + Send + Sync>,
    ) -> Self {
        self.proactivity = engine;
        self
    }

    pub fn with_verifier(mut self, verifier: Arc<dyn eval::Verifier + Send + Sync>) -> Self {
        self.verifier = verifier;
        self
    }

    pub fn with_evolution_seed(mut self, seed: p::EvolutionSnapshot) -> p::Result<Self> {
        seed.validate()?;
        self.evolution_seed = Some(seed);
        Ok(self)
    }

    pub fn with_m3_domain_runtime(mut self, runtime: Arc<M3DomainRuntime>) -> Self {
        self.m3_domains = Some(runtime);
        self
    }

    pub(super) fn with_m3_scope_override(mut self, scope: p::Scope) -> Self {
        self.m3_scope_override = Some(scope);
        self
    }

    pub fn with_coordination(
        mut self,
        reasoner: Arc<dyn coordination::CoordinationReasoner + Send + Sync>,
    ) -> Self {
        self.coordination = Some(reasoner);
        self
    }

    pub fn with_scheduler(
        mut self,
        intentions: Arc<dyn cognition::ScheduledIntentions>,
        config: p::SchedulerConfig,
    ) -> p::Result<Self> {
        config.validate()?;
        self.scheduler = Some(SchedulerRuntime { config, intentions });
        Ok(self)
    }
}
