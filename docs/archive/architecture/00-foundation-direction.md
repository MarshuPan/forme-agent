# forme Agent 底座方向

本文档确认 forme 当前阶段的 Agent 底座方向。它不是需求文档，不是最终技术方案，也不是实施型 PRD；它只固化一个基准判断：forme 要做的是主动式、可进化、可协调、受治理的个人/工作空间 Agent 内核，同时具备通用 Agent 工具底座能力，以及高可靠 agent runtime、工具执行、安全和上下文治理能力。

## 当前定位

forme 的目标不是先限定某个垂直场景，也不是只做 coding CLI。当前先建设一个主动式专用 Agent 内核：它具备通用 Agent 底座能力，但内核从一开始就面向主动协作、长期上下文理解、进化式学习、资源协调、场景认知地图、渐进放权、失败学习、Agent 自我理解、伙伴式成长和用户增强。

这个底座应支持：

- 多入口：CLI、未来 UI、API、Gateway、自动化任务、平台消息入口。
- 多沟通触手：文字、语音、图片、视频通过软件渠道和硬件设备进入统一 Gateway / Communication Fabric。
- 长任务：流式事件、后台运行、取消、恢复、状态查看。
- 工具执行：文件、shell、搜索、MCP、插件工具。
- 安全：权限、审批、路径/网络/命令策略、沙箱抽象。
- 上下文：rules、memory、skills、session history、压缩。
- 多 Agent：长期 agent/profile 与临时 subagent 分开。
- 扩展：plugins、MCP servers、hooks、skills。
- 主动式：观察授权信号、识别机会、判断介入时机、管理打扰成本、形成主动行动候选。
- 进化式学习：从事件、反思、候选知识、验证到长期认知提升，且支持回滚。
- 协调式行动：在目标、记忆、上下文、工具、skills、MCP、模型、subagent、用户注意力和外部资源之间选择最合适的行动路线。
- 场景认知地图：沉淀特定用户、项目、领域和 workflow 的判断框架、质量标准、盲区和资源关系。
- 渐进放权：把 permission、approval、trust、delegation 分开，按 scope、期限、预算和可撤销授权逐步开放自治。
- 失败证据：把目标误解、资源选错、工具失败、验证失败、主动打扰和放权过度作为进化输入。
- Agent 自我理解：通过任务结果、工具表现、验证、失败和用户反馈形成 `AgentSelfModel`，理解自身能力、限制、可靠性和成长状态。
- 伙伴式成长：通过 `PartnershipModel` 表达用户与 Agent 如何互补、协作、放权、纠偏和共同成长。
- 对外沟通：在用户授权范围内通过外部链接、消息渠道、电话或会议等形式进行有限目的沟通，并受披露、预算、期限和终止策略约束。
- 思想代理：在明确身份边界和代表表达策略下，代表用户讲述、介绍、教学或对接，但不能伪装成用户本人或披露未授权内容。
- Agent 协作：通过 bounded communication session 与其他 Agent 协作，默认短会话、强目的、强终止、弱记忆写入。

## Harness-first 主导原则

forme 底座的主导概念应是 `AgentHarness`，不是 CLI、Gateway、模型 provider、工具 registry 或单独的 agent loop。

这个原则来自 forme 对 Agent 工程的基本判断：模型只提供推理能力，reason/action/observation 循环、final output、handoff、tool calls、max turns、guardrails、workflow 与 agent 的区分，以及 durable execution、human-in-the-loop 和 persistence，都必须由运行容器统一承担。

`AgentHarness` 是把模型变成 Agent 的运行容器。它负责接收 run request，绑定 session/workspace/agent profile，装配上下文，治理工具和权限，驱动 loop，持久化状态，发出事件，触发 hooks/plugins，编排 subagent，并在需要时执行 compaction、resume 和 verification。

`AgentLoop` 是 harness 内部的执行状态机。它处理模型调用、tool call、tool result 回填、继续或结束，但不直接拥有入口、会话、权限、持久化和扩展生命周期。

这个原则会影响后续所有文档：需求文档先描述 harness 要支持的能力，技术方案先定义 harness API 和事件协议，实施型 PRD 再拆具体模块和任务。

第二版规划在 Harness-first 之上加入 `Cognitive Plane / CognitiveLoop`；V3 进一步把长期方向校准为“增强型个人主动智能体”；V4 将“主动式”和“进化式学习”上提为内核特征；V9 补齐 `Coordination Kernel`，把资源判断、行动路线、done contract、decision trace 和协调策略进化提升为第三个内核能力；V10 再把 `CognitiveMap`、`TrustProfile / DelegationPolicy / AutonomyEnvelope` 和 `FailureEvidence` 作为三内核补强；V11 把 `Interaction / Communication Fabric` 纳入统一 Gateway，承载文字、语音、图片、视频、软件渠道、硬件设备、对外沟通、思想代理和 Agent-to-Agent bounded session；V12 补齐时间化记忆与 `UserModel`；V13 补齐 `AgentSelfModel`、能力链和 `PartnershipModel`。`AgentLoop` 仍然只负责一次 run 的执行闭环，`CognitiveLoop` 作为 loop runner 运行选定的 `LoopSpec`，负责跨 run、跨 session 的观察、主动交互、学习、反思和候选认知更新。`Coordination Kernel` 位于 Evolution 与 Proactivity 之间，负责把目标、认知地图、记忆、用户理解、Agent 自我理解、伙伴状态、能力、模型、工具、subagent、用户注意力、信任状态、失败证据和外部资源组合成 `ResourcePlan`、`DoneContract`、`AutonomyEnvelope` 和 `ExecutionRoute`。`Cognitive Plane` 在主动式语境下扩展为 `Proactive Plane`，至少包含 ObservationFabric、OpportunityDetector、ValueGate、InterventionPolicy、IdleWorkEngine 和 ProactiveEvaluation。它只能订阅事件、提交 `ActionProposal`、`LearningTask`、`ReflectionTask`、`ProactiveJob`、`CommunicationProposal`、`CognitiveUpdateProposal`、`CognitiveMapUpdateProposal`、`AgentSelfUpdateCandidate`、`PartnershipStateCandidate`、`LoopEvolutionProposal`、`TrustUpdateProposal`、`CoordinationUpdateProposal` 和 `FailureToLearningProposal`；真实行动、对外投递、长期写入、默认策略替换、Agent 自我属性稳定化、伙伴关系稳定化和放权提升仍必须经过 `AgentHarness`、Gateway / Communication Fabric、policy、approval、event store 和提升/回滚流程。

## 总体分层

forme 底座先按 Harness-first 的三层一辅理解。

### Runtime Core

Runtime Core 是 forme 的内核，核心对象是 `AgentHarness`。它负责把一次输入变成可观察、可审计、可恢复的 agent run。它不应该绑定 CLI，也不应该直接绑定具体 UI。

职责方向：

- AgentHarness。
- Run / Session / Turn 或 Thread / Session / Turn。
- Submission / Event 模型。
- LoopEngine / Agent loop 状态机。
- Tool runtime。
- Context builder。
- Model provider 抽象。
- Approval request。
- Cancellation / interruption。
- Event log。

### Control Plane

Control Plane 负责把多个入口、多个 session、多个 agent、插件生命周期和事件流统一起来。M0 必须同时具备 CLI 与 Local Gateway/App Server 的第一版入口，让两者提交同一种 run、消费同一种 event，并共享 approval protocol。它不要求完整多端 UI 或远程云端控制面，但不能把 Gateway 只当成未来预留。

职责方向：

- CLI/API/Gateway run submission。
- Session event stream。
- Run status / wait / drain。
- Plugin lifecycle。
- Hook registration。
- Subagent lifecycle。
- Multi-agent routing。
- Auth / trust / scope 的扩展位置。

### Capability Layer

Capability Layer 负责“Agent 能做什么”。这里包括工具生态、上下文、记忆、skills、MCP、自动化等产品能力。

职责方向：

- ToolRegistry。
- Toolset。
- Tool schema filtering。
- Execution re-check。
- Context compression。
- Session store / SQLite / FTS。
- Memory manager。
- Skills。
- Delegation。
- Cron / automation。
- MCP tools/resources。

### Experience Contract

Experience Contract 不是独立模块，而是贯穿 runtime/control/capability 的产品约束。它规定用户如何理解权限、上下文、子代理、恢复和扩展。

职责方向：

- 权限模式。
- deny / ask / allow。
- Skills 按需加载。
- Subagent 独立上下文。
- Hook lifecycle。
- Context compaction boundary。
- Checkpoint 边界说明。
- MCP schema 延迟加载。

## 初始模块方向

后续 Rust 工程可按以下模块方向设计，但本文不定最终 crate 名称。

| 模块方向 | 核心责任 |
|---|---|
| `harness` | AgentHarness、HarnessConfig、Run/Session/Turn 生命周期、队列、状态机编排、wait/cancel/resume/steer。 |
| `protocol` | Submission、Event、ToolCall、Approval、Session events。 |
| `core` | LoopEngine、model orchestration、turn runtime、context orchestration 的内核实现。 |
| `workflow` | 确定性流程、verification loop、event-driven loop、approval flow。 |
| `tools` | ToolRegistry、Toolset、typed handlers、tool output contract。 |
| `exec` | shell/PTY/process manager/cwd/env/timeout/output streaming/sandbox adapter。 |
| `execution` | ActionIntent、ExecutionPlan、ActionBackend、Shell/File/MCP/Browser/Computer/API/Remote backend、结果规整和动作事件。 |
| `store` | event log、SQLite state、FTS、session lineage。 |
| `policy` | PermissionProfile、ApprovalPolicy、DelegationPolicy、AutonomyEnvelope、权限模式、path/network/tool policy。 |
| `approval` | ApprovalBroker、ApprovalRequest、ApprovalGrant、DelegationGrant、多入口审批通道、批准作用域和过期策略。 |
| `context` | rules、skills metadata、memory summary、session history、retrieved context、compression。 |
| `skills` | local skills loader、metadata、scope、按需加载。 |
| `memory` | memory scope、topic memory、provider interface、candidate/stable memory、UserModelAttribute、AgentSelfAttribute、PartnershipState、CognitiveMap 等长期对象的事实源和候选链。 |
| `cognition` | Cognitive Evolution Kernel、CognitiveLoop、LoopSpec、AgentSelfUpdateCandidate、CognitiveMapUpdateProposal、JudgmentFrame、BlindSpotModel、QualityModel、Reflection、LearningAgenda、OpportunityDetector、InterventionPolicy、IdleWorkEngine，以及 proposal 级 `augmentation_strategy`。cognition 通过 ref 读取长期对象，不重复持有事实源。 |
| `coordination` | GoalFrame、SituationModel、ResourceInventory、UserModelAttributeRef、AgentSelfAttributeRef、CapabilityEvidenceRef、PartnershipStateRef、CognitiveMapRef、TrustProfileRef、FailureEvidenceRef、ResourcePlan、DoneContract、AutonomyEnvelope、ExecutionRoute、DecisionTrace、CoordinationPolicy、CoordinationSpec。 |
| `models` | ModelProvider、ModelProfile、ModelRoutingPolicy、FallbackPolicy、ToolCompatibility、ContextBudgetPolicy。 |
| `config` | typed config schema、config precedence、profile、secrets reference、ConfigDoctor、ConfigChange event。 |
| `mcp` | stdio MCP client、tool discovery、call_tool、allowlist、timeout。 |
| `plugins` | local manifest、tools/skills/MCP/hooks contributions、trust。 |
| `hooks` | BeforeExecution、AfterExecution、SessionStarted、SessionEnded、RunStopped、BeforeCompaction/AfterCompaction 预留。 |
| `agents` | AgentProfile、SubagentRun、delegation runtime、delegation scope、agent definitions。 |
| `capabilities` | CapabilityRegistry、ToolsetResolver、CapabilityReasoner、CapabilityEvidenceStore，统一治理 tools、skills、MCP、plugins、hooks。 |
| `eval` | verifier、trace export、run scoring、tool evaluation、FailureEvidence、FailureTaxonomy、failure digest、RegressionGuard。 |
| `gateway` | WS/HTTP/App Server、event broadcast、future auth。 |
| `surfaces` | CLI、TUI、Desktop、Web、API、消息平台、cron、webhook 的 SurfaceIdentity、GatewayProfile、Ingress/Egress event。 |
| `communication` | text/voice/image/video modality、software/hardware carrier、ChannelAdapter、LocalDeviceChannelAdapter、CommunicationSession、ParticipantProfile、DisclosurePolicy、TerminationPolicy、ExternalCommunicationGrant、Agent-to-Agent bounded session。 |
| `cli` | 调用 core/gateway 的人机入口。 |

## M0 能力方向

M0 是完整核心体系的第一条可运行闭环，其中包含通用 Agent 底座必需能力，以及主动式、进化式、协调式内核必须具备的第一版运行链路。`requirements/03-foundation-scope-decisions.md` 已经固定这些能力不是缩水 MVP，也不是后续再补的扩展项。

建议 M0 默认包含：

- Rust workspace 分层。
- AgentHarness 最小实现。
- LoopEngine / agent loop 状态机最小实现。
- Run / Session / Turn。
- Submission / Event 模型。
- runId / turnId / toolCallId。
- per-session queue。
- append-only event log。
- 结构化 session state，优先考虑 SQLite/FTS。
- resume state，至少支持 approval 后恢复。
- ToolRegistry。
- Toolset。
- tool execution re-check。
- Unified exec 的最小版本。
- ActionIntent / ExecutionPlan / ActionBackend 第一版运行链路。
- ShellBackend 第一版。
- FileBackend 第一版。
- McpBackend stdio 第一版执行链路。
- ExecutionBackendRegistry。
- output budget / timeout / cancellation。
- PermissionProfile。
- ApprovalPolicy。
- ApprovalBroker 第一版。
- ApprovalRequest / ApprovalGrant 结构化对象。
- DelegationGrant / AutonomyEnvelope 运行时 enforcement。
- 权限模式。
- rules / memory / skills 分层。
- local skills 的最小版本。
- MCP stdio 第一版。
- local plugin manifest 第一版。
- BeforeExecution / AfterExecution / SessionEnded 等基础 hooks。
- CapabilityRegistry、ToolsetResolver、CapabilityEvidence event 第一版链路。
- Subagent 第一版：独立上下文、限制 toolset、summary 回传。
- RunResult / stop reason / budget limit / cancellation result。
- Verifier 第一版：确定性检查、最终输出校验、trace export。
- typed config schema。
- config precedence。
- secrets 与普通 config 分离。
- ModelProvider / ModelProfile / ModelRoutingPolicy 的配置骨架。
- CLI + Local Gateway/App Server 共享 run/session/event/approval protocol。
- Interaction / Communication Fabric 第一版边界：CommunicationEvent、CommunicationSession、ParticipantProfile、DisclosurePolicy、TerminationPolicy、ExternalCommunicationGrant、CommunicationProposal。
- ConfigDoctor 第一版。
- observability events。
- cognition / proactivity / coordination 运行事件 schema。
- UserModel / AgentSelfModel / PartnershipModel / CognitiveMap / JudgmentFrame / BlindSpotModel / LoopSpec 的存储边界；长期对象事实源归 memory/store，cognition 只负责语义治理、候选提升和冲突检查。
- AgentSelfAttribute / AgentSelfUpdateCandidate / CapabilityEvidence / CapabilityGap / LearningAgendaItem / PartnershipModel / PartnershipState 的第一版边界。
- CoordinationReasoner 第一版，覆盖 GoalFrame / ResourcePlan / DoneContract / DecisionTrace / CoordinationPolicy / TrustProfileRef / FailureEvidenceRef。
- FailureEvidence / FailureTaxonomy / failure digest 第一版。
- ProactiveEvent、ObservationSource、ObservationScope、InterventionLevel、ProactiveJob 的最小主动闭环。

## M1/M2/M3 方向

M1 适合增强产品能力：

- Gateway 控制台和后台任务体验增强。
- Web/TUI 控制台。
- SurfaceRegistry / GatewayProfile。
- 多入口 ApprovalChannel。
- background job / scheduler 第一版体验。
- BrowserBackend。
- PTY/terminal session。
- context compression 自动化。
- memory manager。
- checkpoint。
- worktree integration。
- tool search / schema 延迟加载。
- plugin runtime。
- 更多 MCP transports。
- automation / cron。
- platform adapters。
- agent definition files。
- per-agent profile。
- CognitiveLoop 体验增强：非对称交互、周期性反思、候选认知更新。
- Cognitive Plane 观察、行动提议、学习任务和认知更新提议接口。
- CognitiveMapUpdateProposal 质量增强：任务结束后形成更稳定的低置信认知地图候选。
- Failure digest 体验增强：把失败分类、影响、相关资源和修正建议写入 trace 并可查看。
- TrustProfile 基础管理：记录用户显式授权、撤销、连续成功和失败。
- seed LoopSpec：默认九阶段模板的版本化定义和事件裁剪。
- CoordinationReasoner 增强：资源选择摘要、WorkPattern 选择、done contract 和 coordination trace export。
- UserModel / AgentSelfModel / PartnershipModel 基础管理，以及主动补强策略的 proposal 级治理。
- Agent 自我审查、能力缺口候选、LearningAgendaItem 和 PartnershipState 基础管理。
- ProactiveJob 体验增强、L0-L3 主动等级、主动建议采纳/拒绝反馈。
- Communication Fabric 体验增强：CommunicationSession、DisclosurePolicy、TerminationPolicy、ExternalCommunicationGrant、受限外部链接会话、本地通知与审批通道。
- CapabilityReasoner 增强、skill 按需加载、MCP dynamic discovery/schema 延迟加载。
- model routing 第一版增强、ConfigChange event、channel-scoped memory 基础边界。

M2 适合主动学习、能力成长和平台增强：

- 完整插件市场。
- 多端原生 app。
- messaging 平台矩阵。
- AppApiBackend connector framework。
- ComputerUseBackend experimental。
- RemoteExecutorBackend。
- 完整 OS 级 sandbox。
- 云端 hosted execution。
- 企业 managed policy。
- agent marketplace / skill hub。
- distributed workers。
- 统一沟通触手矩阵：text/voice/image/video、software carrier、hardware carrier 和 channel adapter 生态。
- 主动学习系统：SourcePolicy、CandidateKnowledge、KnowledgePromotion。
- CapabilityGap、CapabilityUpdateProposal 和 AgentSelfModel 能力更新候选。
- ObservationFabric、EvidencePipeline、IdleWorkEngine、ProactiveEvaluation。
- ResourceGraph、CoordinationOutcome、long-running harness artifacts、model capability aware harness policy。
- 场景认知地图图谱化管理、失败聚类、trust-aware resource routing。
- capability gap detection、skill/tool performance scoring、managed plugin policy。
- 长期目标跟踪、主动建议和定期汇报的基础体验。
- connector identity / delegated identity、per-surface token/cost budget。
- 首批消息平台 adapter、外部沟通链接正式化、语音/会议/电话系统实验、Agent-to-Agent bounded session 实验。

M3 适合自进化和高阶治理：

- LoopRegistry。
- LoopSelectionPolicy。
- LoopFitness。
- LoopEvolutionProposal。
- LoopPromotionPolicy。
- replay / simulation / A-B 对比。
- loop promotion / rollback。
- CoordinationRegistry、CoordinationFitness、WorkPattern promotion/rollback 和协调策略 replay/eval。
- CognitiveMap promotion/replay/rollback、DelegationPolicy simulation、Trust calibration 和 failure-driven regression guard。
- 多角色认知协作网络、认知冲突图谱和长期目标跟踪治理。
- backend selection policy evolution、action risk model evolution、proactive L4 受控开放。

## 原创实现边界

forme 计划开源，因此原创实现不是风格偏好，而是工程和开源边界。forme 的实现应独立设计，尤其是 Rust runtime、安全模型、工具执行、认知进化内核和扩展边界。

后续进入实现前，必须遵守 `docs/archive/planning/09-open-source-positioning-and-kernel-boundaries.md` 的约束：

- 不复制第三方源码、文件结构、文档段落、prompt、错误信息、命令体系或测试 fixture。
- 不使用来源不明或未获授权的材料作为实现来源。
- 本地参考材料不进入主工程 build/import/include 路径。
- 遵守原创性与合规边界检查机制；只有当实现引入依赖、代码复用或协议兼容时，才记录来源、forme 重新设计、copy risk 和 license action。
- 开源发布前完成 LICENSE、NOTICE、依赖和文档原创性检查。

## 文档体系位置

本文是架构基线文档，不是最终 Rust 技术方案。它用于固定 forme 的底座方向、harness-first 主导原则和模块候选。

全局文档路线已经收敛到 `docs/README.md` 和 `docs/documentation-map.md`。当前阶段的关系是：

- `docs/archive/architecture/00-foundation-direction.md` 和 `docs/archive/architecture/01-agent-harness-and-loop.md` 作为早期架构基线，服务后续 planning、requirements 和正式技术方案。
- `docs/archive/planning/01-15` 已经把主动式、进化式学习、协调内核、认知地图、信任放权、失败证据、统一网关与沟通触手、时间化记忆与用户理解模型、Agent 自我模型与伙伴式成长、能力基质、执行触点、模型/网关配置和开源边界收口。
- `docs/requirements/01-vision-and-scope.md` 已经固定需求层面的愿景、范围和非目标。
- `docs/requirements/02-capability-requirements.md` 已经固定能力域、阶段边界和验收方向。
- `docs/requirements/03-foundation-scope-decisions.md` 已经固定 M0 是完整核心体系的第一条可运行闭环。
- `docs/requirements/04-verification-and-acceptance-strategy.md` 已经固定 M0 的验证矩阵和验收证据要求。
- 下一步应评审 M0 范围与验收策略，然后进入 `docs/architecture/03-foundation-architecture.md` 和 `docs/prd/01-foundation-implementation-prd.md`。

当前仍处于文档规划和需求收敛阶段，不代表已经可以开始编码实现。
