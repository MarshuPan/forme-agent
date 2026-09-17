# 主动式专用 Agent 内核规划补充

本文档是 forme 的 V4 规划补充。它承接前四份规划文档，并根据主动式 Agent 产品形态的发展趋势，以及 mixed-initiative / proactive agent 研究，对 forme 的内核定位做一次校准。

本文不是需求文档，不是 Rust 技术方案，也不是实施型 PRD。它的目的，是把“主动式”和“进化式学习”从普通功能上提为 Agent 内核特征。

## 定位校准

此前的表述是“通用 Agent 底座”。这个方向仍然成立，但不够精确。

更准确的定位应是：

> forme 是一个主动式、可进化、受治理的个人/工作空间 Agent 内核。它具备通用 Agent 底座能力，但不是为了泛泛适配所有场景而中性化；它的内核从一开始就面向主动协作、长期上下文理解、持续学习和用户增强。

这里的“专用”不是指绑定某个行业、办公套件、聊天工具或 coding 场景，而是指内核目标明确：

- 主动式是默认设计目标，不是后续插件。
- 进化式学习是默认设计目标，不是普通 memory。
- 用户增强是默认行为准则，不是模仿用户。
- 治理边界是默认内核，不是安全补丁。

## 两个核心特征

### 主动式

主动式 Agent 应具备以下能力：

- 观察授权范围内的上下文变化。
- 识别值得关注的机会和风险。
- 判断是否应该介入。
- 决定介入方式：沉默、记录、延迟总结、轻提醒、追问、反证、建议、请求确认、低风险执行。
- 在用户没有直接发起请求时，也能准备、跟进、学习或提出候选行动。
- 在高影响动作前保留用户控制权。

主动不是“它一直说话”，而是“它知道什么时候不该说、什么时候必须说、什么时候应该先准备”。

### 进化式学习

进化式学习不是把网页、聊天记录或任务结果直接写进长期记忆。它应是一条受控链路：

```text
Authorized Signals
  -> CandidateObservation
  -> ReflectionRecord
  -> LearningAgenda
  -> SourcePolicy
  -> CandidateKnowledge
  -> Verification / ConflictCheck
  -> CognitiveUpdateProposal
  -> Promotion / Rollback
  -> Stable Memory / Skill / Principle / LoopSpec
```

进化式学习的核心不是“自动学习很多东西”，而是“长期认知能够在证据、反馈、验证和治理中持续变好”。

## 主动式内核对象

现有对象模型需要扩展为主动式内核对象模型：

| 对象 | 责任 |
|---|---|
| `AgentHarness` | 最高运行边界，接收所有主动行动、学习、写入、工具调用和权限变更请求。 |
| `ProactivePlane` | 横切运行时的主动认知平面，订阅授权事件，维护观察、机会、计划和候选更新。 |
| `ObservationFabric` | 接入 chat、files、tools、workspace、calendar、code、browser、external feeds 等授权信号。 |
| `ContextGraph` | 组织用户、项目、任务、目标、事件、证据、工具、权限和长期认知关系。 |
| `OpportunityDetector` | 从上下文变化中识别值得介入的机会、风险、缺口和跟进事项。 |
| `ValueGate` | 判断主动介入是否比沉默更有价值，避免噪声和打扰。 |
| `InterventionPolicy` | 决定主动等级 L0-L5、表达方式、确认要求和 surface。 |
| `ActionProposalBroker` | 把主动意图转成 `ActionProposal`，交给 `AgentHarness` 治理。 |
| `IdleWorkEngine` | 利用空闲时间做候选学习、资料准备、反思和预计算。 |
| `ProactiveJob` | 后台主动任务、monitor、digest、follow-up、long-running project 的统一对象。 |
| `LearningAgenda` | 主动学习议题队列。 |
| `EvidencePipeline` | 管理来源、引用、freshness、冲突检查和验证。 |
| `MemoryPromotion` | 管理从事件到反思、候选认知、长期稳定层的提升。 |
| `ProactiveEvaluation` | 记录命中率、采纳率、误报、打扰成本、验证通过率、回滚率。 |

这里的主动式能力已收敛为 `Proactivity Kernel`。早期文档中的 `ProactivePlane / Cognitive Plane` 可理解为横切事件订阅面和历史表述，但正式内核口径应使用三内核：Cognitive Evolution、Coordination、Proactivity。

## 主动式数据流

主动式数据流不应从“用户提问”开始，而应从“授权信号变化”开始：

```text
Authorized Signal
  -> ObservationFabric
  -> CandidateObservation
  -> ContextGraph update
  -> OpportunityDetector
  -> ValueGate
  -> InterventionPolicy
  -> ActionProposal / LearningTask / ReflectionTask / ProactiveJob
  -> AgentHarness
  -> Policy / Approval / ToolRuntime / AgentLoop
  -> EventStore
  -> Verification
  -> Reflection / Learning / MemoryPromotion
```

这个链路要求 forme 的内核不是一个被动 chat loop，而是一个 event-driven proactive runtime。

## 主动等级

既有 L0-L5 治理分级应保留，但需要和主动式内核绑定：

| 等级 | 行为 | 示例 | 默认边界 |
|---|---|---|---|
| L0 Observe | 只观察授权事件和记录事实。 | 记录任务事件、工具结果、上下文变化。 | 不打扰，不产生外部副作用。 |
| L1 Suggest | 给建议、提醒、追问、风险提示或反证。 | 日终摘要、任务后复盘、需求缺口提示。 | 可被忽略，必须记录 reason summary 和打扰成本。 |
| L2 Prepare | 起草、整理、分析、生成计划或待批准草稿。 | 准备资料包、整理对比、生成待审批计划。 | 不产生外部副作用，不写关键状态。 |
| L3 Act With Approval | 审批后执行写操作、命令、外部动作或对外表达。 | 经批准创建任务、发送消息、修改文件。 | 必须有 `DoneContract`、approval 和审计。 |
| L4 Act Autonomously | 预授权低风险、可回滚、预算内自治。 | 在 envelope 内执行非破坏性例行任务。 | 必须受 `AutonomyEnvelope`、policy、撤销和验证约束。 |
| L5 High Impact | 高影响、不可回滚或外部承诺动作。 | 敏感披露、关键修改、重大外部承诺。 | 必须显式审批，不默认自治。 |

主动式 Agent 的质量不在于 L4/L5 越多越好，而在于 L0-L3 的判断足够准，且 L4/L5 的边界足够稳。

## 与长期对象的关系

V3 的三模型表述已经被 canonical 口径收敛：长期对象以 `UserModel / AgentSelfModel / PartnershipModel / CognitiveMap` 为主，主动补强作为 proposal 级策略输出。主动式内核需要读取这些对象，但不能把它们当成直接执行许可。

### UserModel

`UserModel` 不只是偏好记录，还要表达：

- 当前目标和长期目标。
- 工作节奏和可打扰窗口。
- 偏好的主动程度。
- 高影响动作边界。
- 用户的能力短板、风险偏好和反感行为。
- 不同 workspace / project / relationship 下的差异。

### AgentSelfModel

`AgentSelfModel` 不只是能力清单，还要表达：

- 主动识别机会的能力。
- 主动建议的历史命中率。
- 误报、打扰、低价值建议的历史。
- 不同工具、连接器、数据源下的置信度。
- 何时必须降级为“只建议、不执行”。

### 主动补强策略

主动补强策略是主动式内核的关键决策输出。它需要回答：

- 用户当前需要顺从、提醒、追问、反证、学习、建议还是执行？
- 此刻介入是否比沉默更有价值？
- 应该通过哪个 surface 介入：当前对话、延迟摘要、任务面板、通知、后台草稿？
- 需要什么证据和确认？
- 如果判断错了如何撤销或降低主动等级？

## 与 LoopSpec 的关系

`LoopSpec` 不能只定义思考阶段，还要定义主动式运行策略：

| 字段方向 | 含义 |
|---|---|
| trigger_conditions | 何种事件、时间、上下文变化会触发。 |
| observation_scope | 可观察哪些数据，不能观察哪些数据。 |
| opportunity_rules | 哪些机会值得进入候选。 |
| value_gate | 什么情况下沉默优先，什么情况下必须介入。 |
| intervention_modes | 可以使用哪些主动等级和表达 surface。 |
| idle_work_rules | 空闲时间允许做什么准备和学习。 |
| required_evidence | 需要哪些来源、引用、验证和冲突检查。 |
| promotion_rule | 何时从候选策略提升为稳定策略。 |
| rollback_rule | 何时撤销、降级或禁用。 |

默认九阶段 seed LoopSpec 仍可保留，但 V4 应把它从“认知思考模板”扩展为“主动式认知运行模板”。

## 内核分层调整

当前 Harness-first 分层不推翻，但需要校准主语：

- Runtime Core：仍以 `AgentHarness` 为最高运行边界。
- Control Plane：不只是多入口，还要承载 proactive task、monitor、digest、follow-up 的调度和管理。
- Capability Layer：不只是工具和 memory，还要承载 observation connectors、source policy、evidence pipeline。
- Proactivity Kernel：从长期认知平面的观察能力中拆出主动机会识别、价值/能力门控、介入策略和主动反馈。
- Experience Contract：必须定义用户如何理解、调整、暂停和回滚 Agent 的主动性。

## M0/M1/M2/M3 调整建议

### M0

M0 仍然不做完整主动自治，但必须把主动式边界做进内核：

- `ProactiveEvent` 类型预留。
- `ActionProposal`、`LearningTask`、`ReflectionTask`、`ProactiveJob` 数据边界。
- `ObservationSource` 和 `ObservationScope` 数据边界。
- `InterventionLevel` L0-L5。
- `ProactivePolicy` 最小结构。
- `UserModel / AgentSelfModel / PartnershipModel / LoopSpec` 存储边界，以及 proposal 级主动补强策略边界。
- event store 能记录主动触发、主动建议、用户采纳、拒绝、延后和撤销。

M0 不做：

- 常驻桌面观察。
- 自动访问全量办公系统。
- 无监督主动上网学习。
- 自动发送外部消息。
- 默认开启 L4/L5 行动。

### M1

M1 做最小主动式体验：

- 手动或半自动 `ProactiveJob`。
- L0-L3 主动等级。
- 非对称交互：等待、追问、延迟总结、轻提醒。
- seed LoopSpec 的主动式字段。
- `LearningAgenda` 手动/半自动模式。
- 用户可查看、暂停、删除 proactive tasks。
- 主动建议的采纳/拒绝反馈。

### M2

M2 做受控主动学习和能力成长：

- `ObservationFabric` 的多来源接入。
- `SourcePolicy / EvidencePipeline / CandidateKnowledge / KnowledgePromotion`。
- 低风险自动资料收集。
- `IdleWorkEngine`。
- `CapabilityGap / CapabilityUpdateProposal`。
- `ProactiveEvaluation`：采纳率、误报率、打扰成本、验证通过率。
- 长期目标跟踪、主动建议和定期汇报的基础体验。

### M3

M3 做自进化和高阶治理：

- `LoopRegistry / LoopSelectionPolicy / LoopFitness`。
- `LoopEvolutionProposal / LoopPromotionPolicy / rollback`。
- replay / simulation / A-B 对比。
- 多角色认知协作网络。
- 认知冲突图谱。
- 长期主动协作策略治理。

## 需要避免的误区

1. 不把主动式等同于通知系统。
2. 不把主动式等同于常驻看屏幕。
3. 不把主动学习等同于自动爬网页。
4. 不把用户画像当成用户复制品。
5. 不把主动补强策略当成越权理由。
6. 不把 `LoopSpec` 进化交给模型一次反思直接决定。
7. 不把 Slack、办公套件、coding、桌面自动化任一场景当成内核本身。

## 当前结论

forme 的底座应从“通用 Agent 底座”进一步校准为“主动式专用 Agent 内核”：

- 通用能力仍然重要，因为它保证工具、入口、session、权限、扩展、执行和状态统一。
- 专用方向更重要，因为它决定内核从第一天就围绕主动协作、长期上下文、进化式学习和用户增强设计。

后续需求文档不应只问“forme 要支持哪些工具”，还必须问：

- Agent 能观察什么？
- 什么时候应该主动？
- 什么情况下必须保持沉默？
- 什么学习可以自动做，什么必须确认？
- 主动建议如何评估？
- 用户如何暂停、调整、撤销和降级主动性？
- 哪些策略可以进化，哪些内核规则永远不能被模型自改？
