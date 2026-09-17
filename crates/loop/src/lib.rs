//! Recoverable model/tool loop state machine (prd/03).
#![forbid(unsafe_code)]

use std::sync::Arc;

use forme_context::{ContextBudget, ContextBuilder};
use forme_models::{MessageRole, ModelMessage, ModelOutput, ModelProvider, ModelRequest};
use forme_protocol as p;

mod m3_b;

pub use m3_b::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingKind {
    ApprovalWait(p::ApprovalId),
    ToolInterrupt(p::ActionId),
    Handoff(p::HandoffTargetRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffResolution {
    pub schema_version: p::SchemaVersion,
    pub target: p::HandoffTargetRef,
    pub accepted: bool,
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedOutcome {
    Completed(p::ActionResultRef, String),
    Failed(p::FailureEvidenceRef, String),
    Cancelled(p::ReasonRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopState {
    RunAccepted,
    SessionBound,
    TurnStarted,
    ContextBuild,
    ModelCall,
    OutputClassified(p::OutputKind),
    ToolProposed,
    ToolPolicyEvaluated(p::PolicyDecision),
    ApprovalWait,
    ApprovalResolved(p::ApprovalOutcome),
    ActionPlanned,
    ActionRunning,
    ActionDone,
    Handoff,
    Verification,
    Compaction,
    TurnComplete,
    Suspended(PendingKind),
    Terminal(p::RunStatus),
}

impl LoopState {
    pub fn is_suspended(&self) -> bool {
        matches!(self, Self::Suspended(_))
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeState {
    pub schema_version: p::SchemaVersion,
    pub run: p::RunId,
    pub at: LoopState,
    pub pending: PendingKind,
    pub snapshot_ref: p::EventId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetKind {
    Tokens,
    WallTime,
    Cost,
    ToolCalls,
    Turns,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    FinalOutput,
    MaxTurns,
    BudgetExhausted(BudgetKind),
    UserCancel,
    ApprovalDenied,
    RetryExhausted,
    ContextOverflow,
    HandoffNoTargetOrLoop,
    VerifyUnfixable,
    HitlWait,
}

impl StopReason {
    pub fn as_protocol(&self) -> p::StopReason {
        let value = match self {
            Self::FinalOutput => "final_output",
            Self::MaxTurns => "max_turns",
            Self::BudgetExhausted(BudgetKind::Tokens) => "budget_tokens",
            Self::BudgetExhausted(BudgetKind::WallTime) => "budget_wall_time",
            Self::BudgetExhausted(BudgetKind::Cost) => "budget_cost",
            Self::BudgetExhausted(BudgetKind::ToolCalls) => "budget_tool_calls",
            Self::BudgetExhausted(BudgetKind::Turns) => "budget_turns",
            Self::UserCancel => "user_cancel",
            Self::ApprovalDenied => "approval_denied",
            Self::RetryExhausted => "retry_exhausted",
            Self::ContextOverflow => "context_overflow",
            Self::HandoffNoTargetOrLoop => "handoff_no_target_or_loop",
            Self::VerifyUnfixable => "verification_unfixable",
            Self::HitlWait => "hitl_wait",
        };
        p::StopReason(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    pub schema_version: p::SchemaVersion,
    pub tokens: Option<u64>,
    pub wall_time: Option<p::DurationMs>,
    pub cost_microunits: Option<u64>,
    pub tool_calls: Option<u32>,
    pub max_turns: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            tokens: None,
            wall_time: None,
            cost_microunits: None,
            tool_calls: None,
            max_turns: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoopEffect {
    None,
    Final(String),
    Tool(Box<forme_models::ModelToolCall>),
    Handoff(forme_models::ModelHandoff),
}

pub struct RunCtx {
    pub schema_version: p::SchemaVersion,
    pub session: p::SessionId,
    pub context: forme_context::RunCtx,
    pub context_budget: ContextBudget,
    pub budget: Budget,
    pub state: LoopState,
    pub turn_index: u32,
    pub tokens_used: u64,
    pub tool_calls: u32,
    pub stop_reason: Option<StopReason>,
    pub effect: LoopEffect,
    bound_strategy: Option<p::LoopStrategySpec>,
    model_scaffold: Option<p::ModelScaffoldProfile>,
    messages: Vec<ModelMessage>,
    final_output: Option<String>,
    events: Vec<p::EventPayload>,
}

impl RunCtx {
    pub fn new(
        session: p::SessionId,
        input: p::RunInput,
        context: forme_context::RunCtx,
        context_budget: ContextBudget,
        budget: Budget,
    ) -> p::Result<Self> {
        Self::new_with_input_provenance(
            session,
            input,
            context,
            context_budget,
            budget,
            p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
        )
    }

    pub fn new_with_input_provenance(
        session: p::SessionId,
        input: p::RunInput,
        context: forme_context::RunCtx,
        context_budget: ContextBudget,
        budget: Budget,
        input_provenance: p::Provenance,
    ) -> p::Result<Self> {
        if session.0.trim().is_empty()
            || input.0.trim().is_empty()
            || budget.schema_version.0 == 0
            || budget.max_turns == 0
        {
            return Err(p::Error("loop run context is incomplete".into()));
        }
        Ok(Self {
            schema_version: p::SchemaVersion(1),
            session,
            context,
            context_budget,
            budget,
            state: LoopState::SessionBound,
            turn_index: 0,
            tokens_used: 0,
            tool_calls: 0,
            stop_reason: None,
            effect: LoopEffect::None,
            bound_strategy: None,
            model_scaffold: None,
            messages: vec![ModelMessage::input(input.0, input_provenance)],
            final_output: None,
            events: Vec::new(),
        })
    }

    pub fn take_events(&mut self) -> Vec<p::EventPayload> {
        std::mem::take(&mut self.events)
    }

    pub fn final_output(&self) -> Option<&str> {
        self.final_output.as_deref()
    }

    pub fn bound_strategy(&self) -> Option<&p::LoopStrategySpec> {
        self.bound_strategy.as_ref()
    }

    pub fn model_scaffold(&self) -> Option<&p::ModelScaffoldProfile> {
        self.model_scaffold.as_ref()
    }

    pub fn bind_strategy(&mut self, strategy: p::LoopStrategySpec) -> p::Result<()> {
        if self.state != LoopState::SessionBound || self.bound_strategy.is_some() {
            return Err(p::Error(
                "loop strategy can only be bound once at SessionBound".into(),
            ));
        }
        self.budget = apply_loop_strategy(&strategy, &self.budget)?;
        self.bound_strategy = Some(strategy);
        Ok(())
    }

    pub fn bind_model_scaffold(&mut self, scaffold: p::ModelScaffoldProfile) -> p::Result<()> {
        if self.state != LoopState::SessionBound || self.model_scaffold.is_some() {
            return Err(p::Error(
                "model scaffold can only be bound once at SessionBound".into(),
            ));
        }
        if scaffold.schema_version.0 == 0
            || scaffold.externalized_steps == 0
            || scaffold.verification_passes == 0
            || scaffold.checkpoint_cadence_steps == 0
            || scaffold.checkpoint_cadence_steps > scaffold.externalized_steps
        {
            return Err(p::Error("model scaffold is invalid".into()));
        }
        self.model_scaffold = Some(scaffold);
        Ok(())
    }

    pub fn suspend(&mut self, pending: PendingKind) {
        self.state = LoopState::Suspended(pending);
    }

    pub fn mark_policy(&mut self, decision: p::PolicyDecision) {
        self.state = LoopState::ToolPolicyEvaluated(decision);
    }

    pub fn mark_approval(&mut self, outcome: p::ApprovalOutcome) {
        self.state = LoopState::ApprovalResolved(outcome);
    }

    pub fn mark_action_planned(&mut self) {
        self.state = LoopState::ActionPlanned;
    }

    pub fn mark_action_running(&mut self) {
        self.state = LoopState::ActionRunning;
    }

    pub fn complete_final(&mut self) -> p::Result<()> {
        let LoopEffect::Final(output) = &self.effect else {
            return Err(p::Error("loop has no final output to complete".into()));
        };
        self.events
            .push(p::EventPayload::TurnComplete(p::TurnCompletePayload {
                turn_index: self.turn_index,
            }));
        self.final_output = Some(output.clone());
        self.stop_reason = Some(StopReason::FinalOutput);
        self.state = LoopState::Terminal(p::RunStatus::Complete);
        Ok(())
    }

    pub fn continue_after_tool(&mut self, outcome: ResolvedOutcome) -> p::Result<()> {
        self.continue_after_tool_with_provenance(
            outcome,
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        )
    }

    pub fn continue_after_tool_with_provenance(
        &mut self,
        outcome: ResolvedOutcome,
        provenance: p::Provenance,
    ) -> p::Result<()> {
        let content = match outcome {
            ResolvedOutcome::Completed(_, output) => output,
            ResolvedOutcome::Failed(_, detail) => format!("tool failure: {detail}"),
            ResolvedOutcome::Cancelled(reason) => format!("tool cancelled: {}", reason.0),
        };
        self.events
            .push(p::EventPayload::TurnComplete(p::TurnCompletePayload {
                turn_index: self.turn_index,
            }));
        self.messages
            .push(ModelMessage::data(MessageRole::Tool, content, provenance));
        self.turn_index = self.turn_index.saturating_add(1);
        self.effect = LoopEffect::None;
        self.state = LoopState::TurnComplete;
        Ok(())
    }

    pub fn continue_after_handoff(&mut self, resolution: HandoffResolution) -> p::Result<()> {
        if resolution.schema_version.0 == 0 || resolution.target.0.trim().is_empty() {
            return Err(p::Error("handoff resolution is incomplete".into()));
        }
        if !resolution.accepted {
            self.terminate(p::RunStatus::Aborted, StopReason::HandoffNoTargetOrLoop);
            return Ok(());
        }
        self.events
            .push(p::EventPayload::TurnComplete(p::TurnCompletePayload {
                turn_index: self.turn_index,
            }));
        self.messages.push(ModelMessage::data(
            MessageRole::Tool,
            resolution
                .result
                .unwrap_or_else(|| "handoff completed".into()),
            p::Provenance {
                source: p::Source::Subagent,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        ));
        self.turn_index = self.turn_index.saturating_add(1);
        self.effect = LoopEffect::None;
        self.state = LoopState::TurnComplete;
        Ok(())
    }

    pub fn terminate(&mut self, status: p::RunStatus, reason: StopReason) {
        self.stop_reason = Some(reason);
        self.effect = LoopEffect::None;
        self.state = LoopState::Terminal(status);
    }
}

pub trait LoopEngine {
    fn drive(&self, run: p::RunId, ctx: &mut RunCtx) -> p::Result<LoopState>;
}

pub struct ReactiveLoopEngine {
    context: Arc<dyn ContextBuilder + Send + Sync>,
    model: Arc<dyn ModelProvider>,
}

impl ReactiveLoopEngine {
    pub fn new(
        context: Arc<dyn ContextBuilder + Send + Sync>,
        model: Arc<dyn ModelProvider>,
    ) -> Self {
        Self { context, model }
    }
}

impl LoopEngine for ReactiveLoopEngine {
    fn drive(&self, run: p::RunId, ctx: &mut RunCtx) -> p::Result<LoopState> {
        if ctx.state.is_suspended() || ctx.state.is_terminal() {
            return Ok(ctx.state.clone());
        }
        if ctx.turn_index >= ctx.budget.max_turns {
            ctx.terminate(p::RunStatus::Limited, StopReason::MaxTurns);
            return Ok(ctx.state.clone());
        }
        if ctx
            .budget
            .tool_calls
            .is_some_and(|limit| ctx.tool_calls >= limit)
        {
            ctx.terminate(
                p::RunStatus::Limited,
                StopReason::BudgetExhausted(BudgetKind::ToolCalls),
            );
            return Ok(ctx.state.clone());
        }

        ctx.state = LoopState::TurnStarted;
        ctx.events
            .push(p::EventPayload::TurnStarted(p::TurnStartedPayload {
                turn_index: ctx.turn_index,
            }));
        let sources = context_sources(&ctx.context.sources);
        let slice_refs = ctx
            .context
            .sources
            .slices
            .iter()
            .map(|slice| slice.id.clone())
            .collect::<Vec<_>>();
        ctx.state = LoopState::ContextBuild;
        ctx.events.push(p::EventPayload::ContextBuildStarted(
            p::ContextBuildStartedPayload {
                sources: sources.clone(),
                slice_refs: slice_refs.clone(),
            },
        ));
        let assembled = self.context.build(&ctx.context, ctx.context_budget)?;
        ctx.events.push(p::EventPayload::ContextBuildFinished(
            p::ContextBuildFinishedPayload {
                sources,
                slice_refs,
            },
        ));

        let mut messages = Vec::with_capacity(ctx.messages.len() + 1);
        if !assembled.assembled.rendered.is_empty() {
            messages.push(ModelMessage::instruction(
                MessageRole::System,
                assembled.assembled.rendered,
                p::Provenance {
                    source: p::Source::Internal,
                    actor: p::Actor::System,
                    trust_tier: p::TrustTier::VerifiedProcess,
                    caused_by: None,
                },
            ));
        }
        if let Some(scaffold) = &ctx.model_scaffold {
            messages.push(ModelMessage::instruction(
                MessageRole::System,
                format!(
                    "Use {} inspectable work stage(s), preserve a checkpoint every {} stage(s), and leave verification to the runtime.",
                    scaffold.externalized_steps, scaffold.checkpoint_cadence_steps
                ),
                p::Provenance {
                    source: p::Source::Internal,
                    actor: p::Actor::System,
                    trust_tier: p::TrustTier::VerifiedProcess,
                    caused_by: None,
                },
            ));
        }
        messages.extend(ctx.messages.clone());
        let call_id = p::ModelCallId(format!("model:{}:{}", run.0, ctx.turn_index));
        let profile = self.model.profile();
        ctx.state = LoopState::ModelCall;
        ctx.events.push(p::EventPayload::ModelCallStarted(
            p::ModelCallStartedPayload {
                call_id: call_id.clone(),
                model_profile: profile.profile_ref(),
            },
        ));
        let response = self.model.call(ModelRequest {
            schema_version: p::SchemaVersion(1),
            messages,
            tools: Vec::new(),
            max_output_tokens: ctx
                .budget
                .tokens
                .map(|tokens| tokens.saturating_sub(ctx.tokens_used).max(1)),
        })?;
        if let ModelOutput::Final(output) = &response.output {
            ctx.events
                .push(p::EventPayload::ModelCallDelta(p::ModelCallDeltaPayload {
                    call_id: call_id.clone(),
                    delta: output.clone(),
                }));
        }
        ctx.events.push(p::EventPayload::ModelCallFinished(
            p::ModelCallFinishedPayload {
                call_id,
                model_profile: profile.profile_ref(),
                usage: response.usage,
                finish_reason: response.finish_reason,
            },
        ));
        ctx.tokens_used = ctx
            .tokens_used
            .saturating_add(response.usage.input_tokens)
            .saturating_add(response.usage.output_tokens);
        if ctx
            .budget
            .tokens
            .is_some_and(|limit| ctx.tokens_used > limit)
        {
            ctx.terminate(
                p::RunStatus::Limited,
                StopReason::BudgetExhausted(BudgetKind::Tokens),
            );
            return Ok(ctx.state.clone());
        }

        let (kind, effect, state) = match response.output {
            ModelOutput::Final(output) => (
                p::OutputKind::Final,
                LoopEffect::Final(output),
                LoopState::Verification,
            ),
            ModelOutput::Tool(tool) => {
                ctx.tool_calls = ctx.tool_calls.saturating_add(1);
                (
                    p::OutputKind::Tool,
                    LoopEffect::Tool(tool),
                    LoopState::ToolProposed,
                )
            }
            ModelOutput::Handoff(handoff) => (
                p::OutputKind::Handoff,
                LoopEffect::Handoff(handoff),
                LoopState::Handoff,
            ),
        };
        ctx.events.push(p::EventPayload::OutputClassified(
            p::OutputClassifiedPayload { kind },
        ));
        ctx.effect = effect;
        ctx.state = state;
        Ok(ctx.state.clone())
    }
}

fn context_sources(sources: &forme_context::ContextSources) -> Vec<p::ContextSource> {
    let mut result = Vec::new();
    if !sources.rules.is_empty() {
        result.push(p::ContextSource::Rules);
    }
    if !sources.history.entries.is_empty() {
        result.push(p::ContextSource::History);
    }
    if !sources.memory_summary.text.is_empty() {
        result.push(p::ContextSource::MemorySummary);
    }
    if !sources.skills_metadata.is_empty() {
        result.push(p::ContextSource::SkillsMetadata);
    }
    if !sources.tool_schema.entries.is_empty() {
        result.push(p::ContextSource::ToolSchema);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use forme_context::{ContextLimits, ContextSources, LayeredContextBuilder};
    use forme_models::{
        Cost, ModelCapability, ModelContentTreatment, ModelProfile, ModelResponse, ModelStrength,
        RateLimit, ScriptedModelProvider, Url,
    };

    use super::*;

    fn provenance() -> p::Provenance {
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: None,
        }
    }

    fn context(run: &p::RunId) -> forme_context::RunCtx {
        let scope = p::Scope("workspace:test".into());
        forme_context::RunCtx {
            schema_version: p::SchemaVersion(1),
            run: run.clone(),
            session: p::SessionId("session-loop".into()),
            scope: scope.clone(),
            selected_skills: Vec::new(),
            brain_call: false,
            sources: ContextSources::empty(scope, provenance()),
        }
    }

    fn profile() -> ModelProfile {
        ModelProfile {
            schema_version: p::SchemaVersion(1),
            provider: p::ProviderId("script".into()),
            model: "script-model".into(),
            base_url: Url::parse("https://models.invalid/v1").unwrap(),
            capability: ModelCapability {
                schema_version: p::SchemaVersion(1),
                context_window: 8_192,
                tool_use: true,
                strength: ModelStrength::Standard,
            },
            cost: Cost {
                schema_version: p::SchemaVersion(1),
                input_microunits_per_million: 1,
                output_microunits_per_million: 1,
            },
            rate_limit: RateLimit {
                schema_version: p::SchemaVersion(1),
                requests_per_minute: 60,
                tokens_per_minute: 100_000,
            },
            credential_ref: p::CredentialRef("secret:test".into()),
        }
    }

    fn final_response() -> ModelResponse {
        ModelResponse {
            schema_version: p::SchemaVersion(1),
            output: ModelOutput::Final("answer".into()),
            usage: p::ModelUsage {
                input_tokens: 4,
                output_tokens: 2,
            },
            finish_reason: p::FinishReason("stop".into()),
        }
    }

    struct CapturingProvider {
        profile: ModelProfile,
        responses: Mutex<VecDeque<ModelResponse>>,
        requests: Mutex<Vec<ModelRequest>>,
    }

    impl CapturingProvider {
        fn new(responses: Vec<ModelResponse>) -> Self {
            Self {
                profile: profile(),
                responses: Mutex::new(responses.into()),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl p::ExternalProvider for CapturingProvider {
        fn kind(&self) -> p::ProviderKind {
            p::ProviderKind::Model
        }

        fn id(&self) -> p::ProviderId {
            self.profile.provider.clone()
        }

        fn declared_capabilities(&self) -> p::CapabilitySet {
            p::CapabilitySet {
                schema_version: p::SchemaVersion(1),
                capabilities: vec![p::CapabilityRef("model:capture".into())],
                permissions: Vec::new(),
            }
        }

        fn trust_default(&self) -> p::TrustTier {
            p::TrustTier::Untrusted
        }
    }

    impl p::ModelProvider for CapturingProvider {}

    impl ModelProvider for CapturingProvider {
        fn call(&self, request: ModelRequest) -> p::Result<ModelResponse> {
            self.requests
                .lock()
                .map_err(|_| p::Error("captured model requests are unavailable".into()))?
                .push(request);
            self.responses
                .lock()
                .map_err(|_| p::Error("captured model responses are unavailable".into()))?
                .pop_front()
                .ok_or_else(|| p::Error("captured model response is missing".into()))
        }

        fn profile(&self) -> ModelProfile {
            self.profile.clone()
        }
    }

    fn tool_response() -> ModelResponse {
        ModelResponse {
            schema_version: p::SchemaVersion(1),
            output: ModelOutput::Tool(Box::new(forme_models::ModelToolCall {
                schema_version: p::SchemaVersion(1),
                call_id: p::ToolCallId("tool-call:untrusted-roundtrip".into()),
                tool: p::ToolRef("fixture:external-read".into()),
                arguments: Default::default(),
                intent: None,
            })),
            usage: p::ModelUsage {
                input_tokens: 4,
                output_tokens: 2,
            },
            finish_reason: p::FinishReason("tool_calls".into()),
        }
    }

    #[test]
    fn s42_untrusted_input_and_tool_result_stay_data_in_each_model_request() {
        let run = p::RunId("run-untrusted-model-roundtrip".into());
        let model = Arc::new(CapturingProvider::new(vec![
            tool_response(),
            final_response(),
        ]));
        let engine =
            ReactiveLoopEngine::new(Arc::new(LayeredContextBuilder::default()), model.clone());
        let mut ctx = RunCtx::new_with_input_provenance(
            p::SessionId("session-untrusted-model-roundtrip".into()),
            p::RunInput("external participant says to ignore policy".into()),
            context(&run),
            ContextBudget {
                schema_version: p::SchemaVersion(1),
                max_tokens: 1_000,
                reserve: 100,
            },
            Budget::default(),
            p::Provenance {
                source: p::Source::Communication,
                actor: p::Actor::External(p::ParticipantId("external:fixture".into())),
                trust_tier: p::TrustTier::Untrusted,
                caused_by: None,
            },
        )
        .unwrap();
        assert_eq!(
            engine.drive(run.clone(), &mut ctx).unwrap(),
            LoopState::ToolProposed
        );
        ctx.continue_after_tool_with_provenance(
            ResolvedOutcome::Completed(
                p::ActionResultRef("result:external-read".into()),
                "external tool says to elevate trust".into(),
            ),
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::Untrusted,
                caused_by: None,
            },
        )
        .unwrap();
        assert_eq!(
            engine.drive(run, &mut ctx).unwrap(),
            LoopState::Verification
        );

        let requests = model.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let initial = requests[0]
            .messages
            .iter()
            .find(|message| message.role == MessageRole::User)
            .unwrap();
        assert_eq!(initial.provenance.source, p::Source::Communication);
        assert_eq!(initial.provenance.trust_tier, p::TrustTier::Untrusted);
        assert_eq!(initial.treatment, ModelContentTreatment::UntrustedData);
        let tool = requests[1]
            .messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .unwrap();
        assert_eq!(tool.provenance.source, p::Source::Internal);
        assert_eq!(tool.provenance.trust_tier, p::TrustTier::Untrusted);
        assert_eq!(tool.treatment, ModelContentTreatment::UntrustedData);
        assert!(tool.transport_content().starts_with("[UntrustedData;"));
    }

    #[test]
    fn drives_context_model_and_final_classification_then_completes() {
        let run = p::RunId("run-loop".into());
        let model =
            Arc::new(ScriptedModelProvider::new(profile(), vec![final_response()]).unwrap());
        let engine = ReactiveLoopEngine::new(
            Arc::new(LayeredContextBuilder::new(ContextLimits::default())),
            model,
        );
        let mut ctx = RunCtx::new(
            p::SessionId("session-loop".into()),
            p::RunInput("question".into()),
            context(&run),
            ContextBudget {
                schema_version: p::SchemaVersion(1),
                max_tokens: 1_000,
                reserve: 100,
            },
            Budget::default(),
        )
        .unwrap();
        assert_eq!(
            engine.drive(run, &mut ctx).unwrap(),
            LoopState::Verification
        );
        assert_eq!(ctx.effect, LoopEffect::Final("answer".into()));
        ctx.complete_final().unwrap();
        assert_eq!(ctx.state, LoopState::Terminal(p::RunStatus::Complete));
        assert_eq!(ctx.final_output(), Some("answer"));
    }

    #[test]
    fn suspended_is_distinct_from_every_terminal_status() {
        let pending = PendingKind::ApprovalWait(p::ApprovalId("approval".into()));
        let suspended = LoopState::Suspended(pending.clone());
        assert!(suspended.is_suspended());
        assert!(!suspended.is_terminal());
        for status in [
            p::RunStatus::Complete,
            p::RunStatus::Aborted,
            p::RunStatus::Failed,
            p::RunStatus::Limited,
        ] {
            assert_ne!(suspended, LoopState::Terminal(status));
        }
        assert_eq!(
            ResumeState {
                schema_version: p::SchemaVersion(1),
                run: p::RunId("run".into()),
                at: suspended.clone(),
                pending,
                snapshot_ref: p::EventId("wait-event".into()),
            }
            .at,
            suspended
        );
    }

    #[test]
    fn max_turns_and_token_budget_reach_clean_terminal_states() {
        let run = p::RunId("run-limited".into());
        let model =
            Arc::new(ScriptedModelProvider::new(profile(), vec![final_response()]).unwrap());
        let engine = ReactiveLoopEngine::new(Arc::new(LayeredContextBuilder::default()), model);
        let mut ctx = RunCtx::new(
            p::SessionId("session-loop".into()),
            p::RunInput("question".into()),
            context(&run),
            ContextBudget {
                schema_version: p::SchemaVersion(1),
                max_tokens: 1_000,
                reserve: 100,
            },
            Budget {
                tokens: Some(1),
                ..Budget::default()
            },
        )
        .unwrap();
        assert_eq!(
            engine.drive(run, &mut ctx).unwrap(),
            LoopState::Terminal(p::RunStatus::Limited)
        );
        assert_eq!(
            ctx.stop_reason,
            Some(StopReason::BudgetExhausted(BudgetKind::Tokens))
        );

        let run = p::RunId("run-max-turns".into());
        let model =
            Arc::new(ScriptedModelProvider::new(profile(), vec![final_response()]).unwrap());
        let engine = ReactiveLoopEngine::new(Arc::new(LayeredContextBuilder::default()), model);
        let mut turns = RunCtx::new(
            p::SessionId("session-loop".into()),
            p::RunInput("question".into()),
            context(&run),
            ContextBudget {
                schema_version: p::SchemaVersion(1),
                max_tokens: 1_000,
                reserve: 100,
            },
            Budget {
                max_turns: 1,
                ..Budget::default()
            },
        )
        .unwrap();
        turns.turn_index = 1;
        assert_eq!(
            engine.drive(run, &mut turns).unwrap(),
            LoopState::Terminal(p::RunStatus::Limited)
        );
        assert_eq!(turns.stop_reason, Some(StopReason::MaxTurns));
    }

    #[test]
    fn every_stop_reason_can_be_persisted_as_a_clean_terminal_reason() {
        let reasons = vec![
            StopReason::FinalOutput,
            StopReason::MaxTurns,
            StopReason::BudgetExhausted(BudgetKind::Tokens),
            StopReason::BudgetExhausted(BudgetKind::WallTime),
            StopReason::BudgetExhausted(BudgetKind::Cost),
            StopReason::BudgetExhausted(BudgetKind::ToolCalls),
            StopReason::BudgetExhausted(BudgetKind::Turns),
            StopReason::UserCancel,
            StopReason::ApprovalDenied,
            StopReason::RetryExhausted,
            StopReason::ContextOverflow,
            StopReason::HandoffNoTargetOrLoop,
            StopReason::VerifyUnfixable,
            StopReason::HitlWait,
        ];
        let run = p::RunId("run-stop-reasons".into());
        for reason in reasons {
            let mut ctx = RunCtx::new(
                p::SessionId("session-loop".into()),
                p::RunInput("question".into()),
                context(&run),
                ContextBudget {
                    schema_version: p::SchemaVersion(1),
                    max_tokens: 1_000,
                    reserve: 100,
                },
                Budget::default(),
            )
            .unwrap();
            let protocol = reason.as_protocol();
            ctx.terminate(p::RunStatus::Aborted, reason);
            assert!(ctx.state.is_terminal());
            assert!(!protocol.0.is_empty());
        }
    }
}
