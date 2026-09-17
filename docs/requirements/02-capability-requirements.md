# 能力需求与阶段边界

本文档是 forme 的第二份需求文档。它承接 `01-vision-and-scope.md`，把前期 `docs/planning/` 中已经形成的规划判断转化为需求层的能力域、阶段优先级、边界和验收方向。

本文仍然不进入 Rust crate 划分、API 设计、数据库 schema、具体算法和实施步骤。这些内容应在后续 `docs/architecture/03-foundation-architecture.md` 和 `docs/prd/01-foundation-implementation-prd.md` 中展开。

本文之后，`03-foundation-scope-decisions.md` 进一步固定 M0 的范围决策，`04-verification-and-acceptance-strategy.md` 进一步固定 M0 的验证与验收策略。本文不再把 M0 理解为缩水 MVP，而是把 M0 视为完整核心体系的第一条可运行闭环。

## 目标

本文回答：

- forme 第一阶段必须具备哪些能力域。
- 哪些能力属于 M0、M1、M2、M3。
- 每个能力域的用户价值是什么。
- 每个能力域的最低验收方向是什么。
- 哪些能力必须在 M0 形成第一版可运行闭环，哪些属于后续体验、生态或高阶自治增强。
- 哪些能力暂时只保留概念，不进入实现承诺。

本文不回答：

- 具体 Rust 模块和 crate 如何划分。
- 具体接口、事件字段、数据库表和配置 schema 如何定义。
- 每个功能如何编码实现。
- UI 如何设计。
- 具体模型供应商如何选择。
- 具体插件市场如何运营。

## 上游依据

本文主要承接以下文档：

| 上游文档 | 对本文的作用 |
|---|---|
| `01-vision-and-scope.md` | 固定愿景、用户、范围、非目标和主动等级。 |
| `../archive/architecture/01-agent-harness-and-loop.md` | 固定 `AgentHarness` 高于 `AgentLoop` 的运行边界。 |
| `../archive/planning/05-proactive-specialized-agent-kernel.md` | 固定主动式、进化式学习、L0-L5 主动等级和主动式对象边界。 |
| `../archive/planning/06-cognitive-evolution-kernel-assessment.md` | 固定 Cognitive Evolution Kernel、多时间尺度记忆、稳定性模型和进化治理方向。 |
| `../archive/planning/07-capability-substrate-mcp-skills-plugins.md` | 固定 MCP、skills、plugins、hooks、tool registry、toolset、语义选择的能力需求。 |
| `../archive/planning/08-execution-touchpoints-model-gateway-config.md` | 固定 shell、file、browser、computer use、MCP、API connector、gateway、model config 和 config doctor 的边界。 |
| `../archive/planning/09-open-source-positioning-and-kernel-boundaries.md` | 固定开源原创实现和原创性与合规边界检查要求。 |
| `../archive/planning/10-coordination-kernel-and-resource-orchestration.md` | 固定 Coordination Kernel、资源选择、行动路线、DoneContract、DecisionTrace 和协调策略进化方向。 |
| `../archive/planning/11-cognitive-map-trust-and-failure-evidence.md` | 固定 CognitiveMap、TrustProfile、DelegationPolicy、AutonomyEnvelope、FailureEvidence 作为三内核补强，而不是新增第四内核。 |
| `../archive/planning/12-unified-gateway-and-communication-fabric.md` | 固定文字/语音/图片/视频、软件/硬件承载、统一网关、CommunicationSession、对外沟通、思想代理和 Agent-to-Agent bounded session 的规划边界。 |
| `../archive/planning/13-temporal-memory-and-user-model.md` | 固定时间化记忆、UserModel、用户属性候选、ImportedHistoricalEvidence、过程证据优先级和多时间尺度记忆的规划边界。 |
| `../archive/planning/14-agent-self-model-and-partnership-growth.md` | 固定 AgentSelfModel、AgentSelfUpdateCandidate、CapabilityEvidence、CapabilityGap、LearningAgendaItem、PartnershipModel 和 PartnershipState 的规划边界。 |

## 阶段定义

本文使用 M0/M1/M2/M3 表示产品与工程阶段。

| 阶段 | 定义 | 判断标准 |
|---|---|---|
| M0 | 完整核心体系的第一条可运行闭环。 | 没有它就无法证明 forme 是可靠、受治理、可扩展的主动式 Agent 内核。 |
| M1 | 可用体验增强。 | 在 M0 闭环之上增强真实个人/项目工作体验，而不是补 M0 缺失的内核。 |
| M2 | 主动协作。 | 引入多来源观察、后台任务、跨系统触点、受控主动学习和能力成长。 |
| M3 | 自进化治理。 | 引入 loop/strategy/capability 的受控进化、replay/eval/promotion/rollback。 |

本文同时区分认知成熟度阶段，避免把“产品阶段”和“认知进化能力”混为一谈。

| 认知阶段 | 定义 |
|---|---|
| C0 | 记录事件，保留事实，不自动解释为稳定认知。 |
| C1 | 形成反思和候选认知，但不自动稳定化。 |
| C2 | 形成学习议题、候选知识和能力更新。 |
| C3 | 形成跨任务 schema、项目模型和用户模型更新候选。 |
| C4 | 形成 loop、策略和能力组合的候选进化。 |
| C5 | 通过 replay、evaluation、approval 提升默认策略。 |

M0 必须至少跑通 C0/C1 的事实记录、反思候选、候选认知和候选认知地图路径，但不承诺 C3-C5 的完整自动化提升能力。

## 全局优先级

forme 的能力优先级不是“工具越多越好”，而是：

1. 运行时边界必须先正确。
2. 行动必须受治理。
3. 事件和状态必须可观察、可恢复、可审计。
4. 主动式能力必须分级、可暂停、可解释、可撤销。
5. 学习和认知更新必须先候选、后验证、再提升。
6. 资源选择、行动路线和完成标准必须可解释、可评估、可回滚。
7. 场景认知地图、信任放权和失败证据必须进入三内核闭环。
8. 用户理解必须建立在时间化过程证据上，历史导入证据只能作为低权重候选线索，不能直接塑造稳定用户画像。
9. Agent 自我理解必须建立在任务结果、工具表现、验证、失败和用户反馈上，不能用静态 profile 或 prompt persona 假装成长。
10. 用户与 Agent 的伙伴关系必须受用户目标、授权边界、失败证据和长期反馈约束，不能成为越权理由。
11. 能力、执行、入口、模型和配置必须统一进入 harness 生命周期。
12. 开源实现必须从问题出发独立设计。

## 能力域总览

| 能力域 | M0 要求 | M1 要求 | M2/M3 方向 |
|---|---|---|---|
| AgentHarness 与运行生命周期 | 必须 | 增强 | 持续扩展 |
| AgentLoop 与任务闭环 | 必须 | 增强 | 评估驱动优化 |
| 主动式交互 | 最小主动闭环 | 体验增强 | 主动协作 |
| Cognitive Evolution Kernel | 候选生成闭环，包含 UserModel 与 AgentSelfModel 基础边界 | 候选反思、自我审查和伙伴关系增强 | 受控进化 |
| Coordination Kernel | CoordinationReasoner 第一版 | 资源编排增强 | 协调策略进化 |
| Context、Memory 与 Compaction | 基础分层、时间化记忆边界、UserModel 候选链路 | 自动压缩、记忆管理和用户属性审查 | 多时间尺度稳定化 |
| Capability Substrate | 必须 | 语义选择增强 | 能力成长和推荐 |
| Execution / Embodiment Substrate | Shell/File/MCP 第一版 backend | Browser/PTY/Scheduler | API/Computer/Remote |
| Policy、Approval 与 Audit | 必须 | 多入口审批 | 组织级治理 |
| Surface / Gateway / Communication Fabric / Background Job | CLI + Local Gateway/App Server，CommunicationSession 边界 | 后台任务、控制台、通知、受限外部沟通增强 | 多平台入口、软硬件触手、Agent 间协作 |
| Model、Config 与 Doctor | 骨架必须 | 路由和热更新 | 成本/质量策略进化 |
| Subagent 与 Delegation | 简化模型 | 可配置 agent profile | 多 agent 协作治理 |
| Observability、Evaluation 与 Verification | 基础事件和验证 | trace/replay 基础 | eval/promotion 闭环 |
| 开源原创与合规边界 | 原创性与合规边界检查 | 具体来源记录按需常态化 | 发布审计 |

`CognitiveMap`、`Trust / Delegation` 和 `FailureEvidence` 不单独成为新的能力域。它们是跨域约束：

- `CognitiveMap` 进入 R4，并影响 R5 的资源选择和主动式判断。
- `Trust / Delegation` 进入 R5、R9 和 R12，并由 harness 最终执行。
- `FailureEvidence` 进入 R4、R5、R7、R13，并影响能力、认知、协调和信任更新。

## R1. AgentHarness 与运行生命周期

### 用户价值

用户需要一个稳定的 Agent 运行时，而不是一个临时 chat loop。无论从 CLI、未来 UI、API、后台任务还是消息入口进入，用户都应能理解当前运行属于哪个 session、什么权限、哪个模型、哪些工具、能否取消、能否恢复、产生了哪些事件。

### M0 需求

M0 必须固定以下需求：

- 所有入口都应归一化为 run。
- run 必须绑定 session、workspace、agent profile、policy profile、model profile 和 toolset。
- run 必须产生可观察事件。
- run 必须可等待、取消、失败收尾。
- approval 等待态必须可表达。
- 工具中断或审批后恢复必须有边界。
- CLI 不能直接拥有模型循环；CLI 只能提交 run 和消费 event。
- harness 必须拥有 context、tool、policy、event、store、extension、subagent 和 cognition 的治理入口。

M0 不要求：

- 完整 Gateway daemon。
- 多端 UI。
- 远程 worker。
- 高级任务编排平台。

### M1/M2/M3/M4 方向

- M1：支持后台 run、local gateway/app-server、长任务状态查看、跨入口审批。
- M2：支持平台事件、webhook、scheduler、message channel 进入统一 run。
- M3：支持复杂工作流、长期项目 run、multi-agent run graph 和 replay/eval 驱动优化。
- M4：支持单 authority 下受认证 remote executor、只读 replica、跨设备 owner control 和 verified checkpoint handoff；远程 peer 不成为第二 Harness 或第二 event writer。

### 验收方向

- 任意用户请求都能追踪到 run/session/event。
- 用户可以看到运行状态、停止原因和等待原因。
- 权限审批、取消、失败、完成都进入同一事件流。
- 没有入口绕过 harness 直接执行模型或工具循环。

## R2. AgentLoop 与任务闭环

### 用户价值

Agent 需要能把模型推理、工具调用、工具结果、验证、压缩、继续或结束组成可靠闭环，避免“模型说完就结束”或“工具失败后状态丢失”。

### M0 需求

M0 必须具备：

- 模型调用、工具调用、工具结果回填、继续/结束的基础 loop。
- 明确停止条件：final output、max turns、用户取消、审批拒绝、工具错误、预算限制、上下文超限、验证失败等。
- 工具调用必须被视为 proposal，执行前必须重新校验。
- loop 必须输出结构化 run result 和 stop reason。
- loop 事件必须能被 UI、日志、测试和后续 eval 使用。
- 最小 deterministic verifier 边界：命令、文件、schema、必填字段、禁止项等确定性检查。

M0 不要求：

- 完整 LLM judge。
- 自动 rubric retry。
- 完整 workflow engine。
- 多模型自辩论。

### M1/M2/M3/M4 方向

- M1：增强 verification、compaction、tool result validator、trace export。
- M2：支持复杂 workflow、并行任务、长任务 monitor 和失败恢复策略。
- M3：支持 trace-driven improvement、loop fitness、loop promotion 和 rollback。
- M4：长期任务只在 verified durable checkpoint 之间切换 executor；每段是新的 Harness run 并重新绑定 snapshot/approval/lease。

### 验收方向

- loop 状态可以解释“现在为什么继续、暂停或结束”。
- 工具失败、审批拒绝、预算耗尽不会导致状态丢失。
- final output 可以关联到关键工具结果和验证事件。

## R3. 主动式交互

### 用户价值

用户需要的不是另一个“更像自己”的分身，而是足够理解用户、能补强用户、能在合适时机主动协作的增强型助手。主动式必须可控，不能变成噪声、越权或无限自动化。

### M0 需求

M0 必须跑通最小主动式闭环：

- 按 `../architecture/canonical-contract.md` 定义主动等级 L0-L5。
- 定义 observation source、observation scope、proactive event、proactive job、intervention level 的需求边界，并至少支持授权范围内 observation 进入事件流。
- 支持 OpportunityDetector 的第一版判断：从授权信号、当前目标、近期事件或失败证据中识别一个可解释的主动机会。
- 支持 ValueGate 的第一版判断：记录为什么值得提醒、为什么保持沉默、为什么只准备不打扰。
- 支持 CompetenceGate 的第一版判断：记录当前认知、能力、信任或近期失败是否足以支撑主动介入，不足时必须降级或只记录。
- 主动动作必须转成 ActionProposal 或 ActionIntent，再交给 harness、policy 和 approval。
- 高影响动作必须审批。
- 主动行为必须记录原因、scope、风险和结果。
- 用户必须能暂停、拒绝、延后、撤销或降低主动等级。
- 主动建议必须产生反馈事件，采纳、拒绝、延后和撤销都应进入 proactive evidence。

M0 不做：

- 常驻桌面观察。
- 默认读取用户全部文件、聊天、办公系统。
- 无监督主动上网学习。
- 自动发送外部消息。
- L4/L5 默认自治。

### M1 需求

M1 应增强主动式体验：

- 更稳定的 L1 延迟总结、轻提醒。
- 更稳定的 L1 主动追问、风险提醒、遗漏提示，以及 L2 准备资料、草稿和分析。
- 更完整的手动或半自动 proactive job。
- 用户可查看、暂停、删除 proactive task。
- 主动建议反馈进入统计和策略候选。

### M2/M3 方向

- M2：多来源 ObservationFabric、IdleWorkEngine、长期目标跟进、低风险资料准备。
- M3：主动策略进化、打扰成本模型、长期协作策略 replay/eval。

### 验收方向

- 用户能明确知道 Agent 为什么主动出现。
- 主动建议可以被接受、拒绝、延后、撤销。
- 主动任务不会自动扩大权限。
- 系统能记录误报、低价值建议和打扰成本。

## R4. Cognitive Evolution Kernel

### 用户价值

Agent 要长期变强，不能只靠聊天摘要。它需要区分事实、经历、反思、知识、技能、原则和策略，并通过候选、验证、冲突检查、提升和回滚形成稳定认知。

### M0 需求

M0 必须跑通认知进化的候选生成闭环：

- Fixed Kernel 不可被模型自改。
- 长期写入必须 candidate-before-promotion。
- 定义 UserModel、AgentSelfModel、PartnershipModel、CognitiveMap、ProjectModel、TaskModel、SkillModel、RiskModel、ContextGraph 等认知对象的需求边界；主动补强作为 `ActionProposal`、`CommunicationProposal` 或 `LearningTask` 的 `augmentation_strategy` 决策输出，不作为独立持久模型。
- `UserModel` 必须被视为全维度、可演化、有置信度、有时间尺度的用户理解模型，而不是静态 profile 或 prompt 文本。
- `UserModel` 属性必须至少支持 source evidence、confidence、first_observed_at、last_updated_at、stability、decay policy、scope、contradictions、promotion history 和 user feedback 的需求边界。
- `UserModel` 必须区分 `session_state`、`current_focus`、`working_preferences`、`stable_traits` 和 `identity_constraints` 等不同变化速度。
- 用户属性必须先进入 `UserAttributeCandidate`，再经过冲突检查、置信度更新、用户反馈或证据累积后进入稳定属性。
- 历史导入资料必须标记为 `ImportedHistoricalEvidence`，只能作为低权重候选线索，不能直接生成稳定用户画像、自动放权或高置信行为推断。
- `AgentSelfModel` 必须被视为 Agent 对自身能力、限制、风格、可靠性和成长状态的时间化理解模型，而不是静态 Agent profile 或 prompt persona。
- `AgentSelfModel` 属性必须至少支持 source evidence、confidence、first_observed_at、last_updated_at、stability、scope、related_user_goal、related_tool_or_skill、related_failure_evidence、verification_result、promotion_history、rollback_policy 和 user feedback 的需求边界。
- Agent 自我属性必须先进入 `AgentSelfUpdateCandidate`，再经过验证、失败证据、用户反馈、replay 或多次表现积累后进入稳定属性。
- 定义 `CapabilityEvidence`、`CapabilityGap` 和 `LearningAgendaItem` 的需求边界，让工具表现、任务成功、验证失败和用户纠正能进入 Agent 自身成长链。
- 定义 `PartnershipModel` 和 `PartnershipState` 的需求边界，用于表达用户与 Agent 如何互补、协作、放权、纠偏和共同成长。
- 定义 `CognitiveMap`、`JudgmentFrame`、`BlindSpotModel`、`QualityModel` 的需求边界，用于表达特定用户、项目、领域和 workflow 的场景认知地图。
- 每类认知对象必须有稳定性等级、证据要求、更新权限、过期/回滚规则。
- 所有长期对象必须使用 `../architecture/canonical-contract.md` 定义的 fixed / constitutional / stable / working / session / ephemeral 稳定性枚举。
- 主动输出必须经过 `CompetenceGate`：当 MapConfidence、AgentSelfModel、CapabilityEvidence、TrustProfile 或 FailureEvidence 不满足当前 scope 阈值时，主动等级必须降级或只记录。
- 稳定认知对象必须记录 evidence/object 双向血缘，撤销或纠错时触发派生对象再评估候选。
- event、candidate 和 stable object 必须有 schema version；replay/eval/promotion 必须记录所用 schema、policy、LoopSpec、model profile 和 tool schema snapshot。
- 定义 EventMemory、EpisodicMemory、ReflectionMemory、SemanticMemory、ProceduralMemory、PrincipleMemory、StrategyMemory 的分层需求。
- 支持任务结束、主动反馈或失败 digest 触发低置信 `CognitiveMapUpdateProposal`。
- `CognitiveMapUpdateProposal` 必须能表达地图节点、关系、判断框架、质量标准和盲区模型的候选更新。
- 定义并接入 `FailureEvidence` 作为认知更新输入，失败先被记录为事实，再进入反思和候选认知。
- 反思、学习结果和认知地图更新默认进入候选层，不自动成为稳定层。
- 用户必须能查看、拒绝、降级或标记候选认知。
- 用户必须能查看、拒绝、降级或确认关键 `UserAttributeCandidate`。

M0 不做：

- 自动修改核心原则。
- 自动提升新知识为稳定长期认知。
- 自动进化 LoopSpec。
- 模型自主改默认策略。
- 自动把一次失败或一次成功提升为稳定场景认知。

### M1 需求

- 更完整的周期性 reflection candidate。
- LearningAgenda 手动/半自动维护。
- AgentSelfModel 可以记录能力成功、失败、置信度候选、工具熟练度候选和自我审查结果。
- 用户可以查看或纠正 Agent 对自身能力、协作方式和伙伴关系的候选判断。
- 用户可以标记“这次判断框架错了”“这是长期偏好”“这是单次例外”。

### M2/M3 方向

- M2：SourcePolicy、EvidencePipeline、CandidateKnowledge、KnowledgePromotion、CapabilityUpdateProposal。
- M3：EvolutionGovernor、LoopEvolutionProposal、simulation/replay、promotion/rollback、strategy memory evolution。

### 验收方向

- 任意长期认知都能追溯到来源、证据和提升过程。
- 任意稳定用户属性都能追溯到过程证据、历史导入证据、置信度变化和用户反馈。
- 任意稳定 Agent 自我属性都能追溯到任务结果、工具表现、验证结果、失败证据、用户反馈和提升过程。
- 新反思不会直接覆盖稳定原则。
- 用户能撤销或降级错误认知。
- 系统能区分“记录事实”和“形成判断”。
- 系统能区分普通 memory 和特定场景的认知地图。
- 系统能区分过程证据和历史导入证据，并在提升规则中体现权重差异。
- 系统能区分 AgentProfile、AgentIdentityBaseline、AgentSelfModel 和 PartnershipModel，避免把配置、身份底线、自我认知和伙伴关系混为一谈。

## R5. Coordination Kernel

### 用户价值

Agent 不能只是拥有很多工具、skills、MCP、模型、记忆和子代理。用户需要的是 Agent 能面对一个目标，判断哪些资源值得使用、如何组合这些资源、做到什么程度算完成、什么时候应该停、什么时候应该请求用户、什么时候应该验证或转交。

协调内核让认知进化和主动式行动之间有一个明确的元决策层，避免资源堆叠、过度工具调用、自评陷阱、长任务失控和主动式噪声。

### M0 需求

M0 必须实现最小 `CoordinationReasoner`，并定义协调内核的数据边界：

- `GoalFrame`：目标、约束、风险、预算和成功标准。
- `SituationModel`：当前上下文、已知状态、缺失信息、证据和不确定性。
- `ResourceInventory`：当前可用的 context、memory、tools、skills、MCP、models、subagents、external sources 和 user attention。
- `CognitiveMapRef`：本次协调引用了哪些场景认知地图、判断框架、盲区和质量标准。
- `UserModelAttributeRef`：本次协调引用了哪些用户属性、稳定性、置信度和时间尺度。
- `TemporalMemoryRef`：本次协调引用了哪些时间化记忆、近期 focus、阶段目标或 session 状态。
- `ImportedHistoricalEvidenceRef`：本次协调是否引用了历史导入证据，以及它为什么只能作为辅助背景。
- `AgentSelfAttributeRef`：本次协调引用了哪些 Agent 自身能力、可靠性、短板、工具熟练度或当前自我状态。
- `CapabilityEvidenceRef`：本次协调引用了哪些能力成功、失败、验证或用户反馈证据。
- `PartnershipStateRef`：本次协调引用了哪些用户与 Agent 的协作阶段、放权状态、互补方式和纠偏机制。
- `TrustProfileRef`：本次协调引用了哪些 trust、delegation、capability 和 source 信任证据。
- `FailureEvidenceRef`：本次协调是否参考了近期失败、同类失败、工具失败或主动式失败。
- `ResourcePlan`：选定资源、使用顺序、权限要求、失败降级和验证要求。
- `DoneContract`：动手前定义完成标准、验收方式、停止条件和不做事项。
- `AutonomyEnvelope`：当前 run/job/action 可以自主做到什么程度，哪些必须问，哪些必须停。
- `DecisionTrace`：记录目标如何理解、资源为什么被选中、行动路线为什么成立。
- `CoordinationPolicy`：资源选择、风险降级、用户打扰、外部检索、subagent 和 evaluator 的基础策略边界。
- `CoordinationUpdateProposal`：协调策略更新只能作为候选，不直接替换默认策略。
- `CoordinationReasoner` 第一版必须能基于上述对象生成资源选择摘要、`ResourcePlan`、`DoneContract`、`AutonomyEnvelope` 和 `DecisionTrace`。
- 简单 `WorkPattern` 选择进入 M0：direct answer、retrieve then answer、tool loop、ask then act、generator/evaluator、subagent fanout。
- coordination trace 必须进入 event log，并能被 failure evidence 和用户反馈引用。

M0 不做：

- 自动进化默认 `CoordinationSpec`。
- 自动改变模型路由、权限或主动等级。
- 自动生成复杂多 agent 团队。
- 默认把所有工具、记忆和外部资源都暴露给模型。
- 让协调内核绕过 harness 直接执行工具或写长期状态。
- 绕过 trust/delegation 边界把建议升级为自主行动。

### M1 需求

- 增强 `CoordinationReasoner` 的资源质量判断和上下文预算判断。
- 长任务 checkpoint、handoff 和 done contract 体验增强。
- coordination trace viewer/export。
- 用户反馈可以标记资源用多、用少、选错、打扰或完成标准不清。
- 用户反馈可以标记“这类任务可准备但不可执行”“这个工具在此项目不可信”“该判断框架应修正”。

### M2/M3 方向

- M2：`ResourceGraph`、tool/skill/model/memory/source performance scoring、long-running harness artifacts、context/memory budget optimizer、model capability aware harness policy。
- M3：`CoordinationRegistry`、`CoordinationFitness`、WorkPattern promotion/rollback、coordination replay/eval、model-capability-driven harness adaptation。

### 验收方向

- 用户和开发者能知道一次 run 为什么使用某些工具、记忆、skills、模型或 subagent。
- 用户和开发者能知道一次 run 引用了哪些认知地图、信任证据和失败证据。
- 用户和开发者能知道一次 run 引用了哪些用户属性、时间化记忆和历史导入证据。
- 长任务有明确完成标准和 checkpoint/handoff。
- 系统能区分“工具不可用”“资源选错”“完成标准不清”“自评失败”等协调错误。
- 协调策略变更有证据链、候选流程和回滚边界。

## R6. Context、Memory 与 Compaction

### 用户价值

用户需要 Agent 能长期理解项目和工作空间，但不希望它把所有历史、文件和工具 schema 粗暴塞进上下文。上下文必须是资源分配问题，memory 必须是受治理的长期能力。

### M0 需求

M0 必须支持：

- context 分层：system base、agent profile、rules、session history、memory summary、skills metadata、retrieved context、volatile context、tool schema。
- tool schema 只暴露当前 toolset。
- skills 先暴露 metadata，正文按需加载。
- memory 至少区分 session history、memory summary、candidate memory、stable memory 的边界。
- memory 必须区分 Raw Storage、Memory Substrate、UserModel 和 Cognitive Evolution Layer 的需求边界。
- memory 必须支持过程证据优先于历史导入证据的提升规则。
- memory 必须能表达 `ImportedHistoricalEvidence`，并限制其直接影响稳定认知、稳定用户属性、放权和高影响主动行为。
- memory 必须为 `UserModel` 保留结构化边界，不能只把用户理解写成 prompt profile。
- compaction 是 harness/session 生命周期的一部分，不是普通工具。
- compaction 必须产生 lineage 或摘要边界。

M0 不要求：

- 自动长期 memory manager。
- 完整 semantic memory。
- 全自动 context compression。
- 跨项目全局记忆检索。
- 一次性全量外部平台数据湖。

### M1/M2/M3/M4 方向

- M1：自动 compaction、topic memory、session search、skill 按需加载。
- M2：多时间尺度 memory、source freshness、privacy policy、channel scoped memory、selective resource intake。
- M3：memory promotion/decay/replay、认知冲突图谱、跨任务 schema 更新。
- M4：authority memory/event source 保持唯一；只向授权 Replica 输出 filtered/redacted projection，并只在 verified checkpoint 间携带长期任务状态。

### 验收方向

- 用户能知道当前 run 使用了哪些上下文来源。
- 压缩不会丢失审批、工具结果和关键决策 lineage。
- 不同 workspace/project/channel 的 memory 不会默认混用。
- 过程证据、历史导入证据、用户属性候选和稳定用户属性不会被混用。

## R7. Capability Substrate

### 用户价值

Agent 需要知道自己能做什么、什么时候该用什么、哪些能力风险高、哪些能力表现差、哪些能力缺失。MCP、skills、plugins、hooks、tools 不能散落在不同入口里。

### M0 需求

M0 必须固定：

- CapabilityRegistry 的总索引边界。
- ToolRegistry 与 ToolsetResolver 分离。
- 模型可见工具不等于执行许可。
- 工具执行前必须重新检查 toolset scope。
- SkillRegistry 的 metadata 与按需加载边界。
- MCP tools 与 MCP resources 区分。
- local plugin manifest 的最小边界。
- hook contract 至少包含 BeforeExecution、AfterExecution、SessionEnded。
- capability evidence event：记录能力使用、失败、验证和用户反馈。
- 工具 metadata 至少能表达 risk、read/write、stateful、requires_approval、requires_sequential。

M0 不做：

- 插件市场。
- 全自动 skill 生成。
- 全自动 tool search。
- 复杂远端插件安装。
- MCP marketplace。

### M1/M2/M3 方向

- M1：MCP dynamic discovery、schema 延迟加载、skill 按需加载、plugin runtime lifecycle、CapabilityReasoner 增强。
- M2：capability gap detection、skill update proposal、tool/skill performance scoring、managed plugin policy。
- M3：capability promotion/rollback、tool selection policy evolution、plugin trust scoring。

### 验收方向

- 一个能力能说明来源、类型、作用域、风险和状态。
- 当前任务只暴露相关能力。
- 执行层能拒绝模型越权调用。
- 能力表现能反馈到 AgentSelfModel 候选更新。
- 能力缺口能形成 LearningAgendaItem，但不能自动提升为稳定能力或扩大权限。

## R8. Execution / Embodiment Substrate

### 用户价值

Agent 的“手脚”包括 shell、file、MCP、browser、computer use、API connector、scheduler、notification、remote executor 等。它们必须能真实执行，也必须安全、可观察、可取消、可审计。

### M0 需求

M0 必须包含：

- ActionIntent、ExecutionPlan、ActionBackend 的第一版运行边界。
- ShellBackend 第一版。
- FileBackend 第一版。
- McpBackend stdio 第一版。
- ExecutionBackendRegistry。
- output budget、timeout、cancellation。
- action event：started、output、completed、failed、denied、cancelled。
- shell、file、MCP 都必须接入 policy、approval、event 和 evidence。
- shell 不能作为所有动作的默认兜底。
- 文件写入需要 diff 或变更摘要。

M0 不做：

- 完整 BrowserBackend。
- ComputerUseBackend。
- RemoteExecutorBackend。
- 完整 AppApi connector framework。
- 真实桌面控制。
- L4 自动行动。

### M1/M2/M3/M4 方向

- M1：PTY、BrowserBackend、SchedulerBackend、NotificationBackend、background job。
- M2：AppApiBackend、message gateway、ComputerUseBackend experimental、richer sandbox。
- M3：backend selection policy evolution、action risk model evolution、long-horizon autonomous project work。
- M4：`RemoteExecutorBackend`、plan-bound one-shot lease/fencing、authenticated encrypted transport、unknown-outcome recovery 和远端结果地面真值核验。

### 验收方向

- 每个真实动作都能说明 backend、scope、风险和结果。
- 高风险动作进入 approval。
- 输出进入 event 和 tool result，不丢失错误语义。
- backend 的可回滚边界必须明确，不泛化承诺。

## R9. Policy、Approval 与 Audit

### 用户价值

用户必须能理解、控制和审计 Agent 的行动。权限不能只写在 prompt 中，必须成为 runtime 的结构化治理能力。

### M0 需求

M0 必须包含：

- PermissionProfile。
- PolicyEngine。
- ApprovalPolicy。
- ApprovalBroker 第一版。
- `DelegationGrant` 的运行边界：用户显式授权的放权范围、期限、预算、工具、动作类型和撤销条件。
- `AutonomyEnvelope` enforcement：harness 必须能检查当前动作是否在本次自治边界内。
- allow / ask / deny。
- approval request 结构化摘要。
- approval grant 的作用域和过期边界。
- event-sourced audit。
- deny 优先于 ask，ask 优先于 allow。
- 主动任务默认不能扩大权限。
- hook/plugin/MCP 不能绕过 managed/user deny。
- trust 不能覆盖 permission；信任只影响推荐路线和放权建议，不能突破硬权限。

M0 不做：

- 企业组织级 managed policy 全量能力。
- 多平台审批 UI。
- 复杂权限 DSL。
- 自动批准高风险动作。

### M1/M2/M3/M4 方向

- M1：多入口 ApprovalChannel、权限模式、配置化 profile。
- M2：managed policy、channel/workspace identity、data policy、预算策略。
- M3：风险/信任/委派策略 replay/eval/promotion/rollback；不进入组织级身份或自动扩权。
- M4：peer grant、authority epoch、lease/fence、跨设备 owner approval/cancel 和 replication checkpoint 的 event-sourced audit；不进入组织 RBAC。

### 验收方向

- 用户能看到每次 ask 的原因、影响范围、风险和可回滚性。
- 拒绝审批后 run 能正确暂停、降级或结束。
- 审批记录可追溯。
- 权限变化不会无事件发生。
- 用户能撤回、过期或降级已经授予的 delegation。

## R10. Surface / Gateway / Communication Fabric / Background Job

### 用户价值

forme 不应该只活在一次 CLI 对话里。未来的桌面、Web、API、消息入口、cron、webhook、monitor 都应进入同一个 runtime。

同时，forme 的长期目标不是只有“用户对 Agent 说话”这一种交互。Agent 需要通过文字、语音、图片、视频与用户、外部人、外部系统和其他 Agent 沟通；也需要通过软件渠道和硬件设备承载这些沟通；还需要在用户授权下进行有限的对外沟通、代表表达和 Agent 间协作。

这些沟通触手必须进入统一 Gateway / Communication Fabric，而不是直接接到 harness 或散落在工具层。

### M0 需求

M0 要求 CLI 与 Local Gateway/App Server 同时进入第一版闭环：

- run/session/event/approval protocol 不能绑定 CLI。
- CLI 和 Local Gateway/App Server 都必须提交同一种 `RunRequest`，消费同一种 event stream。
- approval request、approval resolved、run waiting、run resumed 必须 Gateway-compatible。
- surface identity、workspace、agent、session、job scope 的需求边界要明确。
- background job 的数据边界要预留，但 M0 不要求完整 scheduler。
- session event stream 要能被 CLI、Local Gateway/App Server 和未来 UI 消费。
- `Interaction / Communication Fabric` 作为 Gateway 的一等子域进入需求边界。
- 沟通形式必须先按 text、voice、image、video 分层，再按软件承载和硬件承载建模。
- software carrier 和 hardware carrier 必须通过 `ChannelAdapter` 或 `LocalDeviceChannelAdapter` 进入标准事件链路。
- 定义 `CommunicationEvent`、`CommunicationSession`、`ParticipantProfile`、`CommunicationPurpose`、`DisclosurePolicy`、`TerminationPolicy`、`TranscriptPolicy`、`ExternalCommunicationGrant` 的需求边界。
- 支持 text modality 的最小路径：CLI / Local Gateway 进入标准 `CommunicationEvent`，再转成 `RunRequest` 或 `CommunicationSession`。
- 支持 `CommunicationProposal`：主动式、协调内核或用户请求可以提出沟通动作，但必须由 Gateway、policy、approval 和 harness 治理。
- 对外沟通、思想代理和 Agent-to-Agent 协作必须有 scope、TTL、budget、disclosure、termination 和 audit 边界。
- 外部沟通 transcript 默认只能进入 evidence/candidate 层，不自动写入 stable memory。

M0 不做：

- Web UI。
- 真实消息、协作、邮件等平台入口。
- 多端同步。
- 真实多平台消息或协作 adapter。
- 真实麦克风、摄像头、电话系统接入。
- 公网外部沟通链接。
- 实时语音/视频。
- Agent-to-Agent 协议实现。
- 默认常驻监听、看屏幕或自动外发消息。

### M1/M2/M3/M4 方向

- M1：SurfaceRegistry、GatewayProfile、background job/scheduler 第一版体验、控制台体验增强。
- M1：Notification / ApprovalChannel、本地或受限外部链接会话、CommunicationSession trace viewer、text + image 输入增强、显式授权下的 LocalDeviceAdapter 实验边界。
- M2：消息平台、webhook、AppApi connector、channel scoped memory、per-surface token/cost budget、首批平台 adapter、外部沟通链接、voice/meeting/phone system 实验、Agent-to-Agent bounded session 实验。
- M3：沟通策略 replay/eval/promotion/rollback、代表表达质量评估和 proactive communication policy 受控进化；不引入多组织或自主 Agent team。
- M4：单 owner 的受认证 OwnerClient/Executor/Replica peer、跨设备 approval/cancel、有限静态 peer 只读复制；不进入多租户或公网匿名控制面。

### 验收方向

- CLI 不是唯一运行边界。
- 未来入口可以通过协议提交 run、读取事件、响应审批、取消任务。
- 后台任务可以被观察、取消、恢复或收尾。
- 同一沟通事件必须能说明 modality、carrier、channel adapter、participant、scope、session、policy 和 audit。
- 外部沟通必须能证明 disclosure policy、TTL、budget 和 termination policy 生效。
- Agent 代表用户表达时必须能证明身份边界、披露边界和必要审批。
- Agent-to-Agent 会话必须能证明目标、预算、终止条件和候选记忆边界。

## R11. Model、Config 与 Doctor

### 用户价值

模型选择、provider、上下文长度、tool calling、vision、computer use、成本、延迟、凭据和失败降级都会影响 Agent 行为。它们必须是配置和治理的一部分，而不是 UI 下拉选项。

### M0 需求

M0 必须包含：

- ModelProvider、ModelProfile、ModelRoutingPolicy 的需求边界。
- provider、model、credential、base_url、capability、rate limit、cost profile 分离。
- secrets 与普通 config 分离。
- config precedence 的需求边界。
- permission、tools、MCP、skills、plugins、hooks、gateway、execution、memory、proactive、budget、observability 的配置分类边界。
- ConfigDoctor 第一版：检查 config schema、provider credential、model availability、MCP server、shell、filesystem permission、plugin manifest 等。

M0 不做：

- 全自动 model routing。
- 模型质量评测平台。
- GUI 配置中心。
- 企业 secret 管理平台。

### M1/M2/M3/M4 方向

- M1：model routing 增强、fallback、config hot reload、ConfigChange event。
- M2：per-surface/per-job budget、review model policy、connector credential scope。
- M3：模型选择策略 eval、成本/质量/风险联合优化。
- M4：authority/executor/replica mode、TLS identity profile、peer allowlist、executor-local credential slot 和 federation ConfigDoctor。

### 验收方向

- 配置错误能被 doctor 解释，而不是由模型猜。
- 密钥不混入普通配置。
- 模型是否支持某工具由 profile 和 compatibility 决定。
- 关键 run 能记录使用了哪个模型和配置来源。

## R12. Subagent 与 Delegation

### 用户价值

用户需要 Agent 能把复杂任务拆给子任务处理，但不能让 subagent 共享过多上下文、越权使用工具或把结果变成不可审计的长文本。

### M0 需求

M0 要求最小 subagent 边界：

- subagent 是 child run 或受 harness 管理的委派任务。
- subagent 必须 独立上下文。
- subagent 必须限制 toolset、model、permission 和 budget。
- subagent 必须绑定 delegation scope 和 `AutonomyEnvelope`。
- subagent 结果必须有 summary/result contract。
- subagent 不能绕过 parent policy。

M0 不做：

- 多 agent 群聊。
- 长期 agent marketplace。
- 自动 agent spawn network。
- 自主团队组织。

### M1/M2/M3/M4 方向

- M1：agent definition/profile、delegation templates、结果验证。
- M2：多 agent 并行任务、specialized agent、cross-session delegation。
- M3：subagent execution governance、role weight/WorkPattern evolution 和结果 replay/eval；子代理仍无独立认知。
- M4：checkpoint 之间可把受限执行放到 remote executor；remote executor 仍是无认知执行单元，不拥有 subagent memory、AgentSelfModel、policy、strategy 或 stable write 权。

### 验收方向

- parent run 能追踪 subagent 的目标、权限、工具和结果。
- subagent 不直接污染主 session memory。
- subagent 失败能被 parent run 正确处理。
- subagent 的成功和失败能进入 TrustProfile 和 FailureEvidence 候选更新。

## R13. Observability、Evaluation 与 Verification

### 用户价值

主动式、可进化 Agent 必须可观察、可评估、可回放。否则用户无法信任它，开发者也无法改进它。

### M0 需求

M0 必须具备：

- append-only event log。
- run/session/turn/action/tool/approval/model/context/verification 关键事件。
- trace export 的需求边界。
- deterministic verifier 第一版。
- tool result validator。
- final output validator。
- capability evidence event。
- `FailureEvidence`：记录失败类型、scope、影响、相关资源、验证结果、用户反馈、可回滚性和后续修正动作。
- `FailureTaxonomy` 初始边界：goal framing、context、cognitive map、resource selection、execution、verification、trust、proactivity、learning、handoff、self-eval trap、policy failure。
- failure digest：把失败分类、影响、相关资源、用户反馈和建议修正写入 trace。
- proactive feedback event：采纳、拒绝、延后、撤销。

M0 不做：

- 完整 eval 平台。
- 自动 regression suite 生成。
- 大规模 LLM judge。
- 自动 prompt hill-climbing。

### M1/M2/M3 方向

- M1：trace viewer/export、run scoring、manual eval set、failure digest 体验增强。
- M2：LLM judge、proactive evaluation、capability performance scoring。
- M3：replay/simulation、loop fitness、promotion/rollback eval。

### 验收方向

- 用户和开发者能解释一次 run 发生了什么。
- 验证失败不会被伪装成成功。
- 主动建议质量能被统计。
- 能力和认知更新有证据链。
- 失败能被分类、回放，并能影响后续认知地图、协调策略、能力评分和信任放权。

## R14. 开源原创与合规边界

### 用户价值

forme 计划开源，必须保持原创实现。用户和贡献者需要明确知道依赖、标准兼容和原创实现的边界。

### M0 需求

M0 必须固定原创性与合规边界检查机制：

- 独立设计，不复制表达。
- 不复制第三方源码、目录结构、prompt、文档、错误信息、命令体系、测试 fixture。
- 不把来源不明或未获授权的材料作为实现来源。
- 本地参考材料（`research/upstreams/**`，不纳入仓库）不进入主工程 build/import/include 路径。
- 文档和实现要以 forme 自己的需求、架构和命名为准。
- 在实现阶段如引入依赖、代码复用或协议兼容，再记录具体来源。
- 文档必须原创，引用必须清楚。

M0 不要求：

- 完整法律审计。
- 开源发布最终 LICENSE/NOTICE 完成。
- 第三方依赖最终锁定。

### M1/M2/M3 方向

- M1：实现阶段对实际引入的依赖、代码复用和协议兼容建立来源记录。
- M2：dependency/license/security review 常态化。
- M3：发布前完整开源审计、NOTICE、release tree 检查。

### 验收方向

- 每个实际进入实现或接口兼容的外部来源都能说明来源、抽象思想和 forme 重新设计。
- 主工程不依赖本地参考材料。
- 文档不出现对第三方文案的近似复制。

## M0 完整核心需求闭环

M0 的目标不是做“大而全”的产品广度，也不是做缩水 MVP，而是形成一个可靠、受治理、可继续演进的主动式 Agent 内核第一条可运行闭环。

M0 完整核心闭环应包含：

1. CLI 或 Local Gateway/App Server 提交 run。
2. AgentHarness 绑定 session、context、model、toolset、policy。
3. Context Builder 装配上下文、memory summary、skills metadata 和当前可见 toolset。
4. CoordinationReasoner 形成 GoalFrame、ResourcePlan、DoneContract、AutonomyEnvelope 和 DecisionTrace。
5. AgentLoop 执行模型和工具闭环。
6. 工具调用执行前经过 scope re-check、policy、approval、delegation 和 autonomy envelope enforcement。
7. Shell/File/MCP 等 action backend 产生结构化结果。
8. event log 记录 run、turn、tool、approval、action、verification、failure、candidate update。
9. context 和 memory 区分 session history、summary、candidate 和 stable。
10. Capability Substrate 统一管理 tools、skills、MCP、plugins、hooks，并生成当前可用能力和工具可见集。
11. Gateway / Communication Fabric 统一管理沟通形式、承载、channel adapter、participant、disclosure、delivery、termination 和 audit 边界。
12. 主动式最小闭环能从 observation 进入 opportunity、value gate、proposal、feedback 和 evidence。
13. CognitiveMapUpdateProposal 能生成低置信候选，但不自动提升为稳定认知。
14. FailureEvidence 能分类并生成 failure digest。
15. TrustProfile、DelegationGrant 和 AutonomyEnvelope 能限制放权，trust 不能覆盖 permission。
16. AgentSelfModel、CapabilityEvidence、CapabilityGap 和 PartnershipState 能记录 Agent 自身成长证据，并进入 coordination trace。
17. ConfigDoctor 能解释基础环境和配置问题。
18. 原创性与合规边界检查机制存在，避免第三方内容进入主工程。

如果 M0 做到了这些，即使 UI 很简、入口数量有限、主动体验还朴素，forme 仍然是正确方向上的完整核心底座。

## 当前明确不做

以下能力不进入 M0 承诺：

- 完整 Web/Desktop UI。
- 完整 Gateway daemon。
- 真实多平台 Communication Adapter 矩阵。
- 真实公网外部沟通链接。
- 实时语音/视频通话。
- Agent-to-Agent 协议实现。
- 常驻看屏幕。
- 默认读取用户全部文件或办公系统。
- 自动上网无限学习。
- 自动发送外部消息。
- 插件市场。
- 完整 Browser/Computer Use。
- 完整 AppApi connector framework。
- 企业级 managed policy。
- 完整 eval 平台。
- 自主修改核心原则。
- 自动进化默认 LoopSpec。
- 自动进化默认 CoordinationSpec。
- 自动稳定化 AgentSelfModel 或 PartnershipModel。
- 用伙伴关系绕过用户控制、permission、approval 或 delegation 边界。
- 自动 L4/L5 行动。
- 复制或移植第三方项目的大模块。

## 已由后续需求文档固定的范围

本文早期版本中的架构前问题已经由 `03-foundation-scope-decisions.md` 固定，不再作为未决项保留。当前决策是：

- M0 采用 CLI + Local Gateway/App Server 双入口。
- M0 直接选择 SQLite/FTS 与 append-only event log 方向。
- M0 将 MCP、skills、plugins 都作为一等能力进入完整核心闭环。
- M0 实现最小主动式闭环，而不是只预留主动式对象。
- M0 实现最小 `CoordinationReasoner`。
- M0 允许生成低置信 `CognitiveMapUpdateProposal`，但不自动提升。
- M0 生成 `FailureEvidence` 分类和 failure digest。
- M0 实现 `DelegationGrant` 与 `AutonomyEnvelope` 的运行时 enforcement。
- M0 的 approval protocol 必须 Gateway-compatible。

后续 `04-verification-and-acceptance-strategy.md` 定义如何证明这些决策没有停留在概念层。

## 当前结论

forme 的能力需求可以收敛为一句话：

> M0 先做一个 Harness-first、事件化、受权限治理、可配置、可验证、可扩展的完整核心 Agent 闭环，并把主动式、认知进化、Agent 自我理解、伙伴式成长、协调内核、认知地图、信任放权、失败证据、能力基质、执行触点、模型配置和原创合规边界都纳入统一运行链路；M1 在这个闭环上增强真实使用体验；M2 扩展跨系统主动协作、受控学习、资源图、场景认知地图和伙伴关系阶段；M3 再探索策略、loop、协调模式、放权策略、AgentSelfModel 和 PartnershipModel 的受控自进化。

因此，后续架构方案不应从“有哪些工具”开始，而应从 `AgentHarness -> Run/Session/Event -> Policy/Approval/Delegation -> Cognitive Evolution/UserModel/AgentSelfModel/PartnershipModel -> Coordination/AutonomyEnvelope -> Capability/Execution -> Context/Memory -> Proactivity -> FailureEvidence/Eval` 的需求链路开始。
