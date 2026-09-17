# 认知进化内核评估

本文档是 forme 的 V5 评估文档。它不是需求文档，不是技术方案，也不是实施型 PRD。它用于重新审视当前规划是否真的支撑“主动式 + 自我学习 + 自我进化”的最终 Agent 方向。

状态说明：本文确认了多时间尺度学习和可验证沉淀方向。后续 `13-temporal-memory-and-user-model.md` 已经进一步细化 memory、UserModel、用户属性候选、ImportedHistoricalEvidence 和过程证据优先级；涉及记忆和用户理解的正式规划口径以后续 V12 文档为准。

核心问题：

> forme 要做的不是一个会主动发通知的 Agent，也不是一个会积累 memory 的 Agent，而是一个具备稳定内核、动态认知、分层学习、可控进化能力的主动式智能体。

## 用户设想的核心含义

当前目标可以被理解为：

- Agent 需要充分理解用户，但不是复制用户。
- Agent 需要理解用户的项目、资料、长期目标、工作模式、短板和风险。
- Agent 需要主动学习外部知识、项目经验和实践反馈。
- Agent 需要形成自己的认知结构和思考链路。
- Agent 需要主动为用户做事，但主动性必须可控、可解释、可回滚。
- Agent 需要自我进化，但不能漂移、失控或随意改变核心原则。

因此，主动式只是表现结果。真正的内核是：

```text
稳定内核
  -> 分层认知
  -> 多时间尺度学习
  -> 可验证沉淀
  -> 可控策略进化
  -> 主动行动
```

如果只做主动提醒、后台任务或自动学习，都只是表层。真正决定 Agent 能不能长期变强的，是它如何管理“哪些东西稳定、哪些东西变化、以什么速度变化、变化后如何验证和回滚”。

## 外部研究映射

以下研究不是要照搬，而是用于支撑架构判断。

### Generative Agents

Generative Agents 提出了 observation、planning、reflection 的架构。重要点不是模拟人类小镇，而是它证明了长期行为需要 memory stream、反思提炼和计划生成共同作用。

对 forme 的启发：

- 日常事件不能直接等于长期认知。
- 需要从事件中周期性提炼更高层反思。
- 反思结果需要影响后续计划和行为。

来源：https://arxiv.org/abs/2304.03442

### Reflexion

Reflexion 通过语言反馈和 episodic memory 改进后续决策，不更新模型权重。关键点是：Agent 可以通过“反思文本 + 任务反馈”形成行为改进。

对 forme 的启发：

- 自我进化不应优先理解为微调模型参数。
- 反思记录可以成为策略改进的候选输入。
- 任务反馈、验证结果和用户反馈都应进入学习闭环。

来源：https://arxiv.org/abs/2303.11366

### Voyager

Voyager 的关键机制是 automatic curriculum、skill library 和 self-verification。它不是简单 memory，而是把探索目标、可复用技能和环境反馈结合起来。

对 forme 的启发：

- 主动学习需要 curriculum，不是随机浏览。
- 能力成长需要 skill/capability library。
- 技能沉淀必须可执行、可验证、可复用。

来源：https://arxiv.org/abs/2305.16291

### Complementary Learning Systems

CLS 理论区分快速学习的海马系统和慢速整合的新皮层系统。快速经验先被记录，之后通过重放、整合和渐进更新进入更稳定的结构。

对 forme 的启发：

- 需要 fast memory 和 slow memory。
- 新信息不能直接写入长期稳定认知。
- 离线反思、重放和整合是必要机制。

来源：https://stanford.edu/~jlmcc/papers/McCMcNaughtonOReilly95.pdf

### Continual Learning 与稳定-可塑性问题

持续学习的核心难点是 stability-plasticity dilemma：系统既要保持旧能力，又要学习新东西。EWC 和灾难性遗忘研究说明，不能让新经验随意覆盖旧结构。

对 forme 的启发：

- 需要保护核心原则和高置信稳定认知。
- 新学习必须进入候选层，再逐步提升。
- 需要回滚、降级、冲突检测和遗忘策略。

来源：

- https://www.pnas.org/doi/10.1073/pnas.1611835114
- https://arxiv.org/html/2403.05175v1

### LLM Agent Memory Survey

近期 Agent memory 研究强调 memory 不是简单存储，而是 write、manage、read 的系统能力，并且与 perception 和 action 紧密耦合。

对 forme 的启发：

- memory 必须参与行动决策，而不是只做检索增强。
- 写入、管理、读取都需要 policy。
- memory 需要按时间范围、表示形式、控制策略分层。

来源：

- https://arxiv.org/abs/2404.13501
- https://arxiv.org/html/2603.07670v1

## 当前架构的正确部分

现有规划中以下判断是稳的，不建议推翻：

### Harness-first

`AgentHarness` 作为最高运行边界是正确的。主动性、自学习、自进化都不能绕过它。否则 Agent 会变成多个外挂脚本、memory job、自动任务和工具调用的拼接体。

应继续保留：

- 所有真实行动回到 `AgentHarness`。
- 所有工具调用执行前重新 policy check。
- 所有长期写入进入候选和提升流程。
- 所有关键变化进入 event store。
- 所有 loop promotion 支持 rollback。

### Candidate-before-promotion

当前 `CognitiveUpdateProposal`、`LoopEvolutionProposal`、候选区、promotion/rollback 的方向是正确的。这是防止认知漂移的底线。

### 长期对象方向

早期 `UserModel / AgentSelfModel / AugmentationModel` 的方向指出了“理解用户、理解自身、补强用户”三件事，但当前 canonical 口径已收敛为：长期对象使用 `UserModel / AgentSelfModel / PartnershipModel / CognitiveMap` 等；主动补强不作为独立持久模型，而作为 proposal 级 `augmentation_strategy` 决策输出。它们都应被放进更大的认知基质中，而不是由单个对象承担全部长期认知。

### LoopSpec 版本化

把 loop 从固定九阶段升级为 `LoopSpec` 是正确方向。但 `LoopSpec` 不应只是阶段模板，它应描述触发条件、适用范围、角色分工、介入方式、学习策略、评估指标和回滚规则。

## 当前架构的不足

### 1. “Cognitive Plane”仍然偏功能层，不够像认知系统

目前文档里的 Cognitive/Proactive Plane 已经有主动机制，但还偏像一组模块：ObservationFabric、OpportunityDetector、ValueGate、IdleWorkEngine 等。

真正的内核需要进一步表达为：

```text
Cognitive Evolution Kernel
  = stable kernel
  + cognitive substrate
  + learning pipeline
  + evolution governor
  + proactive action policy
```

也就是说，主动式不是 ProactivePlane 单独负责，而是整个认知内核的输出。

### 2. 长期对象不够表达“人的属性变化周期”

用户提到的关键点是：一个人可能有很多属性，这些属性都会变，但变化速度不同、互相影响、周期不同。

早期三模型表达了用户、Agent 自身和补强策略，但缺少一个统一的“认知属性分层”。当前正式稳定性枚举以 `../../architecture/canonical-contract.md` 为准：

| Tier | 示例 |
|---|---|
| fixed | 安全、权限、审计、真实性、用户控制、不可宣称真实意识。 |
| constitutional | Agent 的基本行为准则、用户授权边界、长期价值取向。 |
| stable | 用户长期目标、稳定偏好、协作风格、Agent 的稳定能力画像。 |
| working | 对项目、领域、任务模式、用户短板、风险模式的工作认知。 |
| session | 当前任务、当前项目状态、短期计划、最近反馈。 |
| ephemeral | 当前 turn 的推理、工具结果、临时假设、即时注意力。 |

这些层级应该有不同的写入权限、验证要求、更新周期和回滚方式。

### 3. 当前 M0/M1/M2/M3 切分还偏产品阶段，不够表达认知进化阶段

M0/M1/M2/M3 作为产品路线可以保留，但认知进化还需要另一条维度：

| 阶段 | 认知能力目标 |
|---|---|
| C0 | 记录事件，保留认知边界，不自动解释。 |
| C1 | 形成反思和候选认知，但不自动稳定化。 |
| C2 | 形成受控学习议题、候选知识和能力更新。 |
| C3 | 形成跨任务 schema、项目模型和用户模型更新。 |
| C4 | 形成 loop/strategy 的候选进化。 |
| C5 | 通过 replay、evaluation、approval 提升默认策略。 |

这样可以避免产品阶段和认知成熟度混在一起。

### 4. “团队角色式 loop”只是手段，不是本体

多角色 loop 的价值是认知职能分工，例如 Observer、Strategist、Critic、Augmentor、Verifier。但它不能被理解成“多几个 agent 讨论一下”。

正确理解：

- 角色是认知功能，不一定是独立 Agent。
- 角色组合由 `LoopSpec` 决定。
- 角色权重可以随任务和历史表现变化。
- 角色输出必须被 `EvolutionGovernor` 评估。

多角色协作的最终目的不是模拟团队，而是提升认知质量、降低偏差、支持进化。

## 建议的新内核模型

建议把 V5 内核命名为：

> Cognitive Evolution Kernel

它由六个部分组成。

### 1. Fixed Kernel

不可自改的底线：

- Harness-first。
- Policy-at-execution。
- User-control。
- Event-sourced audit。
- Candidate-before-promotion。
- Verifiable-before-stable。
- Reversible-by-design。
- No-real-consciousness-claim。

这是 Agent 的“宪法层”。它不是 personality，也不是 prompt，而是运行时治理协议。

### 2. Cognitive Substrate

认知基质，管理所有可变认知对象：

- UserModel。
- AgentSelfModel。
- PartnershipModel。
- CognitiveMap。
- proposal 级主动补强策略。
- ProjectModel。
- WorldModel。
- TaskModel。
- SkillModel。
- PrincipleStore。
- PreferenceModel。
- RiskModel。
- ContextGraph。

每个对象都必须声明：

- stability level。
- update frequency。
- evidence requirement。
- conflict policy。
- owner。
- rollback rule。
- expiration / decay rule。

### 3. Multi-timescale Memory

多时间尺度记忆：

| 类型 | 作用 |
|---|---|
| EventMemory | 原始事件，保留事实。 |
| EpisodicMemory | 一段任务或经历。 |
| ReflectionMemory | 从经历中提炼的经验。 |
| SemanticMemory | 稳定知识、项目认知、用户认知。 |
| ProceduralMemory | skill、流程、工具使用策略。 |
| PrincipleMemory | 长期行为准则。 |
| StrategyMemory | loop、主动策略、学习策略。 |

（口径更新，见 `../../architecture/canonical-contract.md` §14：本表按"记忆 vs 认知"边界重新归位——EventMemory/EpisodicMemory/ReflectionMemory 属**记忆**（素材），SemanticMemory/ProceduralMemory/PrincipleMemory/StrategyMemory 属**认知**（已在指导判断）。分界点 = 是原始记录还是已指导判断。）

写入路径必须是：

```text
event
  -> episode
  -> reflection
  -> candidate semantic / procedural / principle / strategy
  -> verification
  -> promotion
  -> stable memory
```

### 4. Learning Pipeline

学习管线：

```text
Need / Failure / Goal / External Signal
  -> LearningAgenda
  -> CurriculumPolicy
  -> SourcePolicy
  -> CandidateKnowledge
  -> Practice / Tool Use / Experiment
  -> Verification
  -> Reflection
  -> CapabilityUpdateProposal
  -> Skill / Memory / LoopSpec candidate
```

这里需要引入 `CurriculumPolicy`。它决定 Agent 该学什么、先学什么、为什么学，而不是让 Agent 随机上网看资料。

### 5. Evolution Governor

进化治理器：

```text
Candidate change
  -> StabilityImpact
  -> EvidenceScore
  -> ConflictCheck
  -> Simulation / Replay
  -> RiskReview
  -> Promotion / Reject / Delay / Rollback
```

`EvolutionGovernor` 负责决定哪些变化可以进入稳定层。它比 `LoopPromotionPolicy` 更大，不只管 loop，也管认知模型、能力模型、原则和学习策略。

### 6. Proactive Action Policy

主动行动策略：

```text
Observation
  -> Opportunity
  -> ValueGate
  -> InterventionLevel
  -> ActionProposal
  -> Harness governance
  -> Execute / Ask / Wait / Record
  -> Feedback
  -> ProactiveEvaluation
```

主动性是认知系统的输出，不是通知系统。

## 固定与动态的“占比”模型

早期建议引入 `Stability Profile`。当前已由 `../../architecture/canonical-contract.md` 收敛为唯一稳定性枚举：fixed / constitutional / stable / working / session / ephemeral。本文不再维护第二套层级；下表只作为对象映射示例。

| 认知对象 | canonical stability | 更新策略 |
|---|---|---|
| FixedKernel | fixed | 只能人工改版本。 |
| GovernancePolicy | constitutional | 需要人工确认或严格验证。 |
| PrincipleStore | constitutional / stable | 候选、验证、确认、回滚。 |
| UserLongTermModel | stable | 多证据、多时间点、可撤销。 |
| AgentSelfModel | stable / working | 由验证结果和失败记录更新。 |
| PartnershipModel | stable / working | 由协作结果、授权、撤销和纠偏证据更新。 |
| ProjectModel | working | 随项目事件和文档更新。 |
| SkillModel | working / stable | 由练习、验证和复用更新。 |
| PreferenceModel | working / stable | 区分短期偏好和长期偏好。 |
| WorkingContext | session | 当前任务内快速变化。 |
| Hypothesis | ephemeral | 默认过期，不进入长期层。 |

这个模型比“固定内核 + 可进化策略”更细，也更接近用户描述的人类属性变化方式。

## 对现有文档的调整建议

### 保留

- Harness-first。
- Policy-at-execution。
- Event-sourced observability。
- Candidate-before-promotion。
- UserModel / AgentSelfModel / PartnershipModel / CognitiveMap。
- LoopSpec。
- L0-L5 主动等级。
- M0/M1/M2/M3 产品阶段。

### 升级

- `Cognitive Plane` 升级为 `Cognitive Evolution Kernel`。
- `ProactivePlane` 作为主动式输出层，而不是整个认知系统本身。
- `LoopPromotionPolicy` 升级为更大的 `EvolutionGovernor`。
- `LearningAgenda` 增加 `CurriculumPolicy`。
- `Memory` 从普通 topic memory 升级为 multi-timescale memory。
- `LoopSpec` 增加 stability impact、evaluation metric、role weights、decay/rollback。

### 可能推翻

- “九阶段 loop”不应作为中心设计，只能作为 seed template。
- “多角色协作”不应作为默认实现，只能作为某些 LoopSpec 的认知策略。
- “主动式专用内核”不应只理解为 ProactiveJob/ObservationFabric 等模块集合，而应理解为认知进化系统。

## 当前判断

用户描述的最终 Agent 更接近：

> 一个拥有稳定治理内核、多时间尺度记忆、可验证学习管线、认知属性稳定性模型、受控自我进化机制，并以主动行动为外在表现的个人增强智能体。

它不是完全类人，也不需要模拟意识；但它需要具备类似人类认知系统的几个工程特性：

- 快速经验记录。
- 慢速认知整合。
- 稳定人格/原则边界。
- 不同属性不同更新周期。
- 经验驱动能力成长。
- 反思驱动策略调整。
- 主动行为受价值和风险判断约束。

如果继续推进，后续文档应不只是写“主动式 Agent 内核”，而应写：

> Cognitive Evolution Kernel for Proactive Agent

这会成为 requirements 和 Rust 架构方案之间最关键的一层：它决定哪些东西是数据，哪些是状态，哪些是策略，哪些是内核，哪些可以进化，哪些永远不能自改。
