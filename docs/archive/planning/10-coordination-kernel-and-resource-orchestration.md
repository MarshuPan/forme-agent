# 协调内核与资源编排

本文档是 forme 的 V9 规划补强。它承接 `06-cognitive-evolution-kernel-assessment.md`、`07-capability-substrate-mcp-skills-plugins.md`、`08-execution-touchpoints-model-gateway-config.md` 和 `09-open-source-positioning-and-kernel-boundaries.md`，用于补齐一个此前已经隐含但尚未正式命名的内核能力：

> Agent 面对目标时，如何判断应该使用哪些资源、如何组合这些资源、如何形成行动路线、如何判断完成、如何从结果中改进下一次协调策略。

本文不是需求文档，不是 Rust 技术方案，也不是实施型 PRD。它用于把“协调性”提升为 forme 的第三个核心内核，并说明它如何和前文的主动式、进化式学习、能力基质、执行触点和 harness-first 架构融合。后续 `11-cognitive-map-trust-and-failure-evidence.md` 对本文进行了 V10 补强：协调内核选择资源和路线时，还必须读取场景认知地图、信任放权状态和失败证据。

状态说明：本文中的 M0/M1 切分是 V9 阶段对协调内核的初步落位。后续 `requirements/03-foundation-scope-decisions.md` 已经把最小 `CoordinationReasoner` 纳入 M0，而不是只在 M0 预留数据边界。本文的价值是解释为什么需要协调内核；当前范围以后续需求文档为准。

## 为什么需要协调内核

此前文档已经定义了两个核心方向：

- `Cognitive Evolution Kernel`：让 Agent 长期变强，负责学习、反思、记忆、认知沉淀、策略候选、提升和回滚。
- `Proactive Kernel / Proactive Plane`：让 Agent 不只被动回答，而能在授权范围内观察机会、判断时机、提出建议或发起候选行动。

但这两者之间还缺一个中间能力：Agent 学到了很多知识、拥有很多工具、接入很多 MCP/skills/plugins、保留了很多记忆和上下文后，它到底如何把这些资源组合成最短、最稳、最有价值、最低风险的行动路线？

如果没有协调内核，系统会出现几个问题：

- 资源很多，但选择粗糙：工具、skills、记忆、上下文、子代理、模型、外部资料都存在，却不知道何时该用。
- 主动性变成噪声：能主动发现机会，但不知道这个机会是否值得打扰用户、是否需要先准备资料、是否应保持沉默。
- 进化无法转成行动质量：学习和反思沉淀了很多经验，但没有影响到下一次资源组合和行动路线。
- 长任务容易失控：没有明确的 done contract、checkpoint、handoff 和 trace learning，任务会拖长、偏航或反复自证成功。
- 模型能力变化后 harness 不会变：当模型更会规划、更会工具选择、更会自我管理上下文时，外部 harness 仍然用旧策略，会限制整体能力。

因此，forme 需要把协调能力正式定义为内核级能力，而不是把它散落在工具选择、workflow、subagent、model routing 或 prompt 中。

## 核心判断

forme 的内核主线应调整为：

```text
Cognitive Evolution Kernel
  -> makes the Agent stronger
Coordination Kernel
  -> uses available resources well
Proactive Kernel
  -> initiates useful action at the right time
AgentHarness
  -> governs execution, policy, approval, event, store, rollback
```

更准确的关系是：

```text
Evolution produces better cognition, skills, policies and resource knowledge.
Coordination selects and composes resources into action routes.
Proactivity detects opportunities and asks coordination before acting.
Harness enforces every real action, long-term write and default-policy change.
```

主动式的价值依赖进化能力，但进化能力必须通过协调能力转化为行动质量。也就是说，`Proactive Kernel` 不应直接决定“我要做什么”；它应产生 `Opportunity` 或 `ActionCandidate`，再交给 `Coordination Kernel` 判断资源和路径，最后交给 `AgentHarness` 治理执行。

## Coordination Kernel 的定义

`Coordination Kernel` 不是普通 orchestrator，也不是 tool router。它是 Agent 的元决策层，负责在目标、上下文、能力、风险、成本、用户注意力和完成标准之间做资源理性选择。

它的核心问题是：

```text
面对一个目标，我如何选择最短、最稳、最有价值、最低风险的路径？
```

它协调的对象包括：

- model resources：模型、模型能力、上下文窗口、成本、延迟、tool compatibility。
- context resources：当前任务上下文、历史摘要、retrieved context、rules、skills metadata。
- memory resources：事件记忆、反思记忆、语义记忆、程序记忆、原则记忆、策略记忆。
- capability resources：tools、MCP tools/resources、skills、plugins、hooks、toolsets。
- execution resources：shell、file、browser、computer use、API connector、scheduler、remote backend。
- cognitive resources：UserModel、AgentSelfModel、PartnershipModel、CognitiveMap、ProjectModel、RiskModel、ContextGraph，以及 proposal 级主动补强策略。
- agentic resources：subagents、generator/evaluator、workflow、LoopSpec、CoordinationSpec。
- user resources：用户注意力、审批意愿、可打扰窗口、反馈、业务判断。
- external resources：网页、论文、GitHub 项目、官方文档、社区实践、外部系统数据。

因此，协调内核不是“选择一个工具”，而是把目标、认知、能力、执行、用户控制和外部资源组合成一条可治理的行动路线。

## 外部理论和实践映射

协调内核不是凭空创造的新名词，它对应多个成熟方向的交集。

| 方向 | 对 Coordination Kernel 的启发 |
|---|---|
| Rational metareasoning / bounded optimality | 智能体不只要做事，还要判断哪些推理、检索、工具调用和子任务值得花成本做。参考：https://people.eecs.berkeley.edu/~russell/research-bo.html |
| Sensemaking loop | 复杂任务需要在信息觅食、证据组织、假设构建、行动判断之间循环，而不是一次性回答。参考：https://andymatuschak.org/files/papers/Pirolli%2C%20Card%20-%202005%20-%20The%20sensemaking%20process%20and%20leverage%20points%20for%20analyst%20technology%20as.pdf |
| ReAct | reasoning、action、observation 的交替证明了模型需要环境反馈来修正路径。参考：https://arxiv.org/abs/2210.03629 |
| Reflexion | 任务反馈和反思可以改进后续策略，但需要被记录为可用经验。参考：https://arxiv.org/abs/2303.11366 |
| Voyager | curriculum、skill library、self-verification 说明能力成长需要目标选择、技能沉淀和验证闭环。参考：https://arxiv.org/abs/2305.16291 |
| Workflow/agent patterns | routing、orchestrator-workers、evaluator-optimizer 都是资源协调模式，但需要按任务选择，不应固定一种。 |
| Long-running harness 实践 | done-contract、persistent artifacts、checkpoint、generator/evaluator、trace reading 和 harness 随模型能力变化，是长任务协调的关键经验。 |

这些资料共同指向一个结论：越强的 Agent 越不能只依赖“模型自由发挥”。它需要一个能随模型能力变化而演化的 harness，并且需要把资源选择、任务路线、完成标准、验证和经验回收变成可观察、可评估、可改进的系统能力。

## 三内核关系

### Evolution Kernel

`Cognitive Evolution Kernel` 回答：

```text
我如何变得更强？
```

它负责：

- 记录经验和反馈。
- 形成反思。
- 生成候选认知、候选知识、候选技能、候选策略。
- 管理稳定性等级、证据要求、冲突检查、提升和回滚。
- 让 UserModel、AgentSelfModel、PartnershipModel、CognitiveMap、SkillModel、StrategyMemory 等长期变好，并让主动补强策略在 proposal 层变得更准确。

它产出的内容会成为协调内核的输入，例如：哪些工具历史表现好、用户不喜欢什么打扰方式、某类任务应先做验证、某个领域需要先学习。

### Coordination Kernel

`Coordination Kernel` 回答：

```text
我如何用好已有资源？
```

它负责：

- 明确目标和 done contract。
- 建模当前局面。
- 读取可用资源和限制。
- 选择模型、上下文、记忆、工具、skills、MCP、subagent、workflow、外部资料。
- 形成行动路线。
- 决定何时执行、何时先问、何时先学习、何时保持沉默。
- 记录 decision trace。
- 从 outcome 中改进下一次协调策略。

它是 Evolution 与 Proactivity 之间的转换层。

### Proactive Kernel

`Proactive Kernel` 回答：

```text
我什么时候应该主动出现？
```

它负责：

- 观察授权信号。
- 识别机会、风险、缺口和待跟进事项。
- 形成 `Opportunity`、`LearningTask`、`ReflectionTask`、`ActionCandidate` 或 `ProactiveJob`。
- 通过 ValueGate 判断主动介入是否可能有价值。

但它不应直接执行行动。它必须把候选交给协调内核和 harness。

## 与 AgentHarness 的边界

`Coordination Kernel` 不能绕过 `AgentHarness`。它可以提出 `ResourcePlan`、`DoneContract`、`ExecutionRoute`、`ActionProposal` 和 `CoordinationUpdateProposal`，但真实行动必须回到 harness。

```text
Goal / Signal / Opportunity
  -> Coordination Kernel
  -> ResourcePlan / DoneContract / ExecutionRoute
  -> AgentHarness
  -> Policy / Approval / ToolRuntime / AgentLoop
  -> EventStore / Trace / Outcome
  -> Evolution Kernel / Coordination Kernel
```

边界规则：

- 协调内核可以选择资源，但不能直接越权调用资源。
- 协调内核可以建议用某工具，但执行前必须经过 toolset re-check、policy 和 approval。
- 协调内核可以建议更新策略，但默认策略变更必须进入 candidate、evaluation、promotion、rollback。
- 协调内核可以请求用户注意力，但必须服从 InterventionPolicy 和打扰预算。
- 协调内核可以选择 subagent 或 evaluator，但 child run 必须受 parent harness 管理。

## 核心对象模型

| 对象 | 责任 |
|---|---|
| `GoalFrame` | 描述目标、用户意图、约束、成功标准、风险等级、时间和预算。 |
| `SituationModel` | 描述当前上下文、已有状态、缺失信息、已知风险、可用证据和不确定性。 |
| `ResourceInventory` | 当前可用资源快照，包括 tools、skills、MCP、models、memory、subagents、external sources、user attention。 |
| `ResourceGraph` | 资源之间的依赖关系、互斥关系、风险关系、上下游关系和历史表现。 |
| `CoordinationPolicy` | 定义资源选择、成本预算、风险降级、用户打扰、外部检索、subagent、evaluator 的默认策略。 |
| `CoordinationSpec` | 某类任务的协调模板，定义节点、输入、输出、适用范围、评估指标和回滚方式。 |
| `StrategyCandidate` | 一条候选行动路线，例如直接回答、先检索、先问用户、生成/评估循环、subagent fanout。 |
| `ResourcePlan` | 选定资源、使用顺序、预算、权限要求、失败降级路径和验证要求。 |
| `DoneContract` | 动手前定义“什么叫完成”，包括验收标准、输出格式、验证方式和停止条件。 |
| `WorkPattern` | 可复用工作模式，例如 direct-answer、research-summarize、tool-loop、generator-evaluator、long-running-harness。 |
| `ExecutionRoute` | 具体执行路线，绑定 run、loop、workflow、tool calls、subagents、checkpoints 和 handoff。 |
| `DecisionTrace` | 记录目标如何理解、资源如何选择、为什么选择这条路线、哪些判断后来被证明错误。 |
| `CoordinationOutcome` | 记录执行结果、成本、验证结果、用户反馈、失败模式和策略表现。 |
| `CoordinationUpdateProposal` | 对 CoordinationPolicy、CoordinationSpec、WorkPattern 或资源评分的候选更新。 |

这些对象不是都必须在 M0 完整实现，但 M0 至少要把数据边界和事件边界定义出来，否则后续无法评估协调能力。

## Seed CoordinationSpec

初始版本可以有一个 seed `CoordinationSpec`，但不能把它当成永久固定流程。

```text
1. GoalFraming
2. SituationModeling
3. ResourceMapping
4. StrategyComposition
5. ValueRiskCostScoring
6. DoneContractFormation
7. ExecutionRouting
8. CheckpointAndHandoff
9. OutcomeEvaluation
10. CoordinationLearning
```

### 1. GoalFraming

把用户请求、主动机会或后台任务转成目标框架。重点不是复述需求，而是明确：

- 用户真正要的结果是什么。
- 成功标准是什么。
- 哪些约束不能突破。
- 这是回答、研究、执行、长期任务、学习任务还是主动建议。
- 是否需要先向用户协商完成标准。

### 2. SituationModeling

建模当前局面：

- 已知上下文。
- 相关记忆。
- 当前项目状态。
- 现有证据。
- 缺失信息。
- 风险和不确定性。
- 是否有足够信息开始行动。

### 3. ResourceMapping

列出可用资源和限制：

- 哪些 memory/context 值得读。
- 哪些 skills 可能适用。
- 哪些 tools/MCP 可用。
- 哪些模型适合规划、执行、评估。
- 是否需要 subagent。
- 是否需要外部资料。
- 用户注意力是否是必要资源。

### 4. StrategyComposition

生成有限数量的候选路线，不无限展开。例如：

- 直接回答。
- 先追问一个关键问题。
- 先读取本地文档再回答。
- 先搜索外部资料再汇总。
- 用工具执行并验证。
- 让 subagent 并行研究。
- generator/evaluator 循环。
- 长任务 harness：persistent artifacts + checkpoint + per-iteration verification。

### 5. ValueRiskCostScoring

对候选路线评分：

- 价值：是否更接近用户目标。
- 风险：是否会误导、越权、破坏状态、打扰用户。
- 成本：token、时间、工具调用、用户注意力、金钱。
- 置信度：模型和工具对该任务的历史表现。
- 可回滚性：失败后能否恢复。
- 新鲜度：资料是否需要更新。
- 可验证性：结果能否被检查。

### 6. DoneContractFormation

动手前形成完成合同。长任务尤其需要这一点。

`DoneContract` 至少应包含：

- 目标输出。
- 必须满足的验收标准。
- 不做什么。
- 需要哪些验证。
- 何时停止。
- 失败或不确定时如何升级给用户。

### 7. ExecutionRouting

把路线交给合适的执行机制：

- 简单回答进入普通 AgentLoop。
- 工具任务进入 tool loop。
- 可确定流程进入 workflow。
- 长任务进入 long-running harness pattern。
- 复杂检查进入 generator/evaluator。
- 并行研究进入 subagent child runs。
- 主动任务进入 ProactiveJob。

### 8. CheckpointAndHandoff

长任务必须留下可恢复状态：

- 当前目标。
- 已完成事项。
- 已尝试路径。
- evaluator 发现的问题。
- 已修复内容。
- 剩余风险。
- 下一步建议。

这些状态不只是记忆，也是协调资源。未来模型、用户或子代理需要靠它继续任务。

### 9. OutcomeEvaluation

评估路线效果：

- 结果是否满足 DoneContract。
- 是否用了过多或过少资源。
- 是否误判风险。
- 是否过早结束。
- 是否打扰用户。
- 是否遗漏更好的工具、skill、memory 或外部来源。

### 10. CoordinationLearning

把评估结果转成候选更新：

- 更新资源评分。
- 记录适用或不适用的 WorkPattern。
- 形成 `CoordinationUpdateProposal`。
- 标记需要人工确认或 replay/eval 的策略变更。

## 资源选择机制

协调内核的第一类核心能力是判断用什么资源。

建议的初始判断链路：

```text
GoalFrame
  -> SituationModel
  -> ResourceInventory
  -> CandidateStrategy
  -> Value / Risk / Cost / Confidence scoring
  -> ResourcePlan
```

资源选择不应只靠模型瞬时判断。它应结合：

- 任务类型。
- 用户授权范围。
- 当前 workspace/project/channel。
- 工具和 skill metadata。
- MCP/resource/source trust。
- 历史成功和失败。
- 上下文预算。
- 模型能力 profile。
- AgentSelfModel 的能力置信度。
- 用户偏好和打扰预算。
- 风险和可回滚性。

### 资源评分维度

| 维度 | 问题 |
|---|---|
| relevance | 这个资源是否直接服务当前目标。 |
| reliability | 这个资源过去是否可靠，是否经常失败或误导。 |
| risk | 使用它会不会修改状态、泄露数据、产生外部影响。 |
| cost | token、时间、金钱、用户注意力是否值得。 |
| freshness | 是否需要实时信息或最新资料。 |
| scope fit | 是否在当前用户、workspace、project、session 授权范围内。 |
| reversibility | 出错后能否撤销或降级。 |
| verification | 使用后能否验证结果。 |
| cognitive value | 是否能产生长期学习、反思或能力沉淀。 |

## 行动路线组合

协调内核的第二类核心能力是把资源组合成路线。

建议的初始链路：

```text
ResourcePlan
  -> WorkPattern
  -> DoneContract
  -> ExecutionRoute
  -> Checkpoint / Handoff
```

常见 `WorkPattern` 可以先定义为：

| WorkPattern | 适用场景 |
|---|---|
| `direct_answer` | 信息足够、低风险、无需工具。 |
| `ask_then_act` | 目标、约束或成功标准不清楚。 |
| `retrieve_then_answer` | 需要读本地文档、历史、memory 或资料。 |
| `research_then_synthesize` | 需要外部资料、论文、官方文档或最新信息。 |
| `tool_loop` | 需要执行工具并根据 observation 继续。 |
| `workflow` | 路径明确、审批或验证步骤固定。 |
| `generator_evaluator` | 输出质量难以自评，需要独立 critic 或真实环境验证。 |
| `subagent_fanout` | 子问题可并行、上下文隔离更有利。 |
| `long_running_harness` | 任务跨多轮、多小时、多阶段，需要持久 artifacts 和 checkpoint。 |
| `proactive_prepare` | 主动式低风险准备，不直接打扰或执行高影响动作。 |

这些模式不应都进入 M0 的完整实现，但可以作为 M0 的概念边界和 M1/M2 的增强方向。

## 协调策略进化

协调内核的第三类核心能力是从 traces、失败和反馈中改进协调方式。

建议链路：

```text
DecisionTrace
  -> OutcomeEvaluation
  -> FailureTaxonomy coordination view
  -> CoordinationUpdateProposal
  -> Replay / Simulation / Evidence
  -> Promotion / Rollback
```

### FailureTaxonomy 的协调域视图

`CoordinationError` 不再作为独立失败分类。以下条目只是 `FailureTaxonomy` 在协调域里的视图，正式分类以 `../../architecture/canonical-contract.md` 为准。

| 错误 | 表现 |
|---|---|
| over-tooling | 简单任务用了太多工具、检索或子代理；通常映射到 resource_selection_failure 或 proactivity_failure。 |
| under-tooling | 应该查证或执行验证，却直接回答；通常映射到 verification_failure 或 resource_selection_failure。 |
| wrong_resource | 选错工具、模型、skill、memory 或资料源；映射到 resource_selection_failure。 |
| premature_action | 没有先协商 done contract 就开始执行；映射到 goal_framing_failure 或 trust_failure。 |
| missing_context | 忽略关键项目文档、历史约束或用户偏好；映射到 context_failure。 |
| self_eval_trap | 自己检查自己，过早宣称完成。 |
| over_delegation | 不必要地拆给多个 subagent，增加协调成本；通常映射到 resource_selection_failure。 |
| poor_handoff | 长任务没有留下可恢复状态；映射到 handoff_failure。 |
| stale_strategy | 模型能力变化后仍使用旧 harness 策略；映射到 learning_failure 或 verification_failure。 |
| noisy_proactivity | 主动建议低价值或打扰成本过高；映射到 proactivity_failure。 |

### Harness 随模型能力变化

参考长任务 harness 实践，一个关键结论是：harness 不会因为模型变强而消失，它会随着模型能力迁移。

当模型弱时，harness 需要更多外部结构：

- 更细的分步计划。
- 独立上下文。
- 强制 persistent artifacts。
- 更频繁 evaluator。
- 更硬的 done contract。

当模型强时，harness 可以把一部分控制权交回模型：

- 允许更长单 session。
- 依赖更强的 context management。
- 减少不必要的分段。
- 让模型承担更多 planning/tool selection。

但这不是“取消 harness”，而是 `CoordinationPolicy` 的变化。forme 需要把这种变化记录成可评估、可回滚的策略，而不是隐含在 prompt 或临时经验里。

## 与 Capability Substrate 的关系

`Capability Substrate` 回答：

```text
Agent 有哪些能力、能力从哪里来、风险是什么、历史表现如何？
```

`Coordination Kernel` 回答：

```text
当前目标应该选哪些能力、如何组合、何时使用、做到什么程度？
```

二者关系：

```text
CapabilityRegistry / ToolRegistry / SkillRegistry / McpRegistry
  -> ResourceInventory
  -> Coordination Kernel
  -> ResourcePlan
  -> AgentHarness execution governance
  -> CapabilityEvidenceStore
  -> AgentSelfModel / CoordinationPolicy candidate update
```

因此，能力基质不是协调内核的替代品。它是协调内核的资源事实层。

## 与 Context / Memory 的关系

memory 不是越多越好，context 也不是越大越好。协调内核需要把 context 和 memory 当成资源来分配。

它需要判断：

- 当前 run 是否需要读取稳定记忆。
- 是否只需要 session history。
- 是否需要检索项目文档。
- 是否需要压缩上下文。
- 是否应把某段历史保留为 checkpoint。
- 某个反思是否只是候选经验，还是足以影响当前路线。

这可以避免两个极端：

- 什么都记，导致上下文污染。
- 什么都不读，导致长期认知无法发挥作用。

## 与 Proactive Kernel 的关系

主动式不应直接等于行动。合理链路是：

```text
Authorized Signal
  -> Observation
  -> Opportunity
  -> Proactive ValueGate
  -> Coordination Kernel
  -> ResourcePlan / DoneContract / InterventionPlan
  -> AgentHarness
```

协调内核在主动式里负责判断：

- 这件事是否值得现在打扰用户。
- 是否应该先后台准备资料。
- 是否应该只记录，不发声。
- 是否应该主动追问。
- 是否需要低风险执行候选。
- 是否需要用户审批。
- 是否需要学习后再建议。

主动式的质量最终取决于协调内核对“资源、时机、风险、用户注意力”的判断。

## 与 LoopSpec 的关系

`LoopSpec` 定义认知 loop 或任务 loop 的阶段。`CoordinationSpec` 定义资源协调和路线选择策略。

二者可以关联，但不应混成一个东西：

| 对象 | 关注点 |
|---|---|
| `LoopSpec` | 一类认知或执行 loop 如何运行。 |
| `CoordinationSpec` | 面对目标时如何选择资源、选择 loop、组合路线和评估协调效果。 |

示例：

```text
CoordinationSpec selects:
  - use research_then_synthesize WorkPattern
  - use external official sources
  - use verifier after synthesis
  - use seed research LoopSpec
  - do not use write-capable tools
```

这样设计后，loop 可以进化，协调策略也可以进化，但两者都有版本、适用范围、评估指标和回滚。

## 固定与可进化边界

协调内核必须区分固定不变的底线和可进化策略。

### 固定不变

- 所有真实行动必须经过 `AgentHarness`。
- 执行前必须 policy check。
- 高风险动作必须 approval。
- 长期写入必须 candidate-before-promotion。
- 默认策略变更必须可追溯、可评估、可回滚。
- 用户注意力是受治理资源，不能无限打扰。
- 不把一次成功当成稳定策略。

### 可进化

- 不同任务使用哪些 WorkPattern。
- 资源评分权重。
- 模型分工策略。
- subagent 使用策略。
- evaluator 触发条件。
- context / memory 读取策略。
- proactive intervention 策略。
- long-running harness 的 checkpoint 频率。
- done contract 模板。

这符合前文“固定内核 + 动态学习”的占比模型：底线稳定，策略随证据进化。

## M0/M1/M2/M3 建议

### M0

M0 不做完整自动协调，但必须定义协调内核的数据边界：

- `GoalFrame`。
- `SituationModel`。
- `ResourceInventory`。
- `ResourcePlan`。
- `DoneContract`。
- `DecisionTrace`。
- `CoordinationOutcome`。
- `CoordinationPolicy` 最小结构。
- `CoordinationSpec` seed 边界。
- `CoordinationUpdateProposal` 候选边界。
- coordination-related events。

M0 行为边界：

- 可以用简单规则或显式 prompt 形成 `ResourcePlan`。
- 可以要求长任务先形成 `DoneContract`。
- 可以记录为什么用了某工具、某 skill、某 memory 或某 subagent。
- 不自动进化默认 `CoordinationSpec`。
- 不自动改变模型路由、权限或主动等级。

### M1

M1 做最小协调体验：

- `CoordinationReasoner` MVP。
- 任务前的 resource selection summary。
- 长任务 done contract。
- 简单 WorkPattern 选择。
- basic generator/evaluator 或 verifier routing。
- 用户可反馈“资源用多了、用少了、选错了、打扰了”。
- coordination trace export。

### M2

M2 做资源图和跨系统协调：

- `ResourceGraph`。
- tool/skill/model/memory/source performance scoring。
- long-running harness artifacts。
- stronger subagent routing。
- proactive prepare route。
- context/memory budget optimizer。
- model capability aware harness policy。

### M3

M3 做受控协调策略进化：

- `CoordinationRegistry`。
- `CoordinationFitness`。
- `CoordinationUpdateProposal` replay/eval。
- WorkPattern promotion/rollback。
- model-capability-driven harness adaptation。
- multi-agent coordination policy evolution。
- coordination strategy A/B or simulation。

## forme 协调内核的定位

通用 agent runtime 通常分别解决 runtime、tool execution、approval、控制面、session、plugin、tools/toolsets、memory、delegation、context compression、skills、hooks 和 permissions 等问题，但这些能力往往是功能并列的。

forme 的协调内核把能力层、执行层、治理层重新组织为“目标 -> 资源 -> 路线 -> 验证 -> 经验回收”的原创内核设计，更强调长期主动式、认知进化、用户增强、资源理性选择和协调策略进化。

## 开源边界

协调内核尤其需要遵守开源边界，因为它容易受到第三方 prompt、工具描述、agent loop 代码和 long-running harness 示例的影响。

要求：

- 不复制第三方源码、文档表达、prompt、命令体系、错误信息或测试 fixture。
- `CoordinationSpec`、`WorkPattern`、`DoneContract`、`DecisionTrace` 等命名和结构应按 forme 自己的设计重新写。
- 来源不明或未获授权的材料不能作为实现来源。
- 后续实现前，若引入依赖、代码复用、协议兼容或具体公开实现细节，必须进入原创性与合规边界检查记录。
- 引用外部资料时只写短链接和概念来源，不搬运长篇原文。

## 对既有文档的修正关系

本文档不推翻前文，而是补齐前文缺口：

- `06-cognitive-evolution-kernel-assessment.md` 说明 Agent 如何变强。
- `07-capability-substrate-mcp-skills-plugins.md` 说明 Agent 有哪些能力和能力如何治理。
- `08-execution-touchpoints-model-gateway-config.md` 说明 Agent 如何触达真实世界。
- `09-open-source-positioning-and-kernel-boundaries.md` 说明初版内核和开源边界。
- 本文说明 Agent 如何把认知、能力、工具、记忆、模型、subagent、用户注意力和外部资源协调成行动路线。

因此，后续 requirements 和 architecture 不应只写：

```text
AgentHarness -> Capability/Execution -> Cognitive Evolution -> Proactive
```

而应写成：

```text
AgentHarness
  -> Cognitive Evolution Kernel
  -> Coordination Kernel
  -> Proactive Kernel
  -> Capability / Execution / Context / Memory / Policy
```

其中 `AgentHarness` 仍是最高治理边界，`Coordination Kernel` 是资源和行动路线的元决策层。

V10 进一步补充：`Coordination Kernel` 的输入不应只包含 `GoalFrame`、`ResourceInventory` 和 `SituationModel`，还应包含 `CognitiveMapRef`、`TrustProfileRef` 和 `FailureEvidenceRef`；输出不应只包含 `ResourcePlan` 和 `ExecutionRoute`，还应包含本次行动的 `AutonomyEnvelope`。

## 当前结论

forme 初版内核应从“两核心”升级为“三核心”：

```text
Evolution
  -> Coordination
  -> Proactivity
```

这不是功能数量增加，而是把主动式 Agent 必须具备的中间能力补上：

- 没有 Evolution，Agent 不会长期变强。
- 没有 Coordination，Agent 无法把知识、工具、记忆和模型变成高质量行动。
- 没有 Proactivity，Agent 仍然只是被动响应。
- 没有 Harness governance，三者都会失控。

后续进入需求和架构方案时，应把 `Coordination Kernel` 作为与 `Cognitive Evolution Kernel`、`Proactive Kernel` 同级的内核主线，并在 M0 至少固定 `GoalFrame`、`ResourcePlan`、`DoneContract`、`DecisionTrace` 和 `CoordinationPolicy` 的数据边界。V10 后还应把 `CognitiveMapRef`、`TrustProfileRef`、`FailureEvidenceRef` 和 `AutonomyEnvelope` 作为协调内核的 M0 数据边界。
