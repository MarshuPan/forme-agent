# Agent 自我模型与伙伴式成长

本文档是 forme 的 V13 规划补强。它承接 `03-personal-augmented-agent-and-evolving-loop.md`、`06-cognitive-evolution-kernel-assessment.md`、`10-coordination-kernel-and-resource-orchestration.md`、`11-cognitive-map-trust-and-failure-evidence.md` 和 `13-temporal-memory-and-user-model.md`，专门补齐一个此前已经存在但不够完整的内核问题：

> forme 不只需要理解用户，也需要持续理解自己；不只需要服务用户，也需要和用户形成长期伙伴关系，并在受治理的边界内成长。

本文仍然是规划文档，不进入 Rust 模块、数据库 schema、API 或实施型 PRD。它的作用是把 Agent 自身成长、AgentSelfModel、PartnershipModel、成长阶段和用户- Agent 相辅相成的关系，纳入后续 requirements、architecture 和 PRD 的上游约束。

## 核心结论

forme 的 Agent 不应被理解为一次性初始化的角色卡、prompt persona 或固定助手。更准确的定义是：

> forme Agent 是一个围绕用户长期目标成长的主动式智能伙伴。它深度理解用户，也持续理解自己；它通过记忆链、思维链、能力链、失败链和协作链，在固定治理内核内与用户共同成长。

这意味着：

- 用户理解不是全部，Agent 自我理解同样是一等对象。
- Agent 的初始版本只是 seed，不是最终形态。
- Agent 的能力、风格、可靠性、协作方式和思考策略都可以演化。
- Agent 的演化必须和用户目标、用户反馈、任务过程、失败证据、验证结果和授权边界相关。
- Agent 不能脱离用户目标漫无目的成长，也不能过拟合用户当前状态。
- “助手”和“服务”是过程角色，不是 forme Agent 的最终定义。

## 为什么需要 AgentSelfModel

如果只有 `UserModel`，forme 会变成一个“了解用户的工具”。它可能知道用户是谁、喜欢什么、短板在哪里，但它不一定知道自己：

- 哪些任务可靠。
- 哪些领域不可靠。
- 哪些工具用得好。
- 哪些工具经常失败。
- 哪些判断容易出错。
- 哪些主动建议被用户采纳。
- 哪些主动建议造成打扰。
- 哪些技能需要学习。
- 哪些 loop 或协调策略表现差。
- 哪些外部来源值得信任。

没有 `AgentSelfModel`，Agent 的成长无法闭环。它会停留在初始 prompt、初始工具和初始策略上。即使 memory 越来越多，也不代表 Agent 自己变强。

因此，`AgentSelfModel` 必须成为和 `UserModel` 对称的一等认知对象。

```text
UserModel
  = Agent 对用户的时间化理解

AgentSelfModel
  = Agent 对自身能力、限制、风格、可靠性和成长状态的时间化理解

PartnershipModel
  = Agent 对双方如何协作、互补、放权、纠偏和共同成长的时间化理解
```

## 不等于 Agent Profile

需要明确区分三个对象。

| 对象 | 含义 | 稳定性 | 来源 |
|---|---|---|---|
| `AgentProfile` | 初始化配置：名称、默认模型、默认工具、基础 persona、默认策略。 | 高，但主要是配置。 | 用户配置、项目配置、发布默认值。 |
| `AgentIdentityBaseline` | 不可自我修改的身份和治理底线：不宣称真实意识、不绕过用户控制、不越权、不伪造能力。 | 极高。 | 固定内核和人工版本变更。 |
| `AgentSelfModel` | Agent 对自己能力、短板、可靠性、学习状态和行为表现的证据化理解。 | 多时间尺度。 | 任务、验证、失败、用户反馈、工具结果、学习结果。 |

`AgentProfile` 可以决定“Agent 一开始是什么样”。  
`AgentSelfModel` 决定“Agent 经过长期协作后知道自己是什么样”。  
`AgentIdentityBaseline` 决定“Agent 无论如何成长都不能突破什么”。

不能把 `AgentSelfModel` 写成 prompt 里的一段自我介绍。它必须是可追溯、可验证、可降级、可回滚的认知对象。

## Agent 自我属性

Agent 的属性比人类 profile 更偏执行和能力，但同样具有多维度和变化周期。

`AgentSelfModel` 至少应覆盖：

- 基础身份边界。
- 固定治理原则。
- 当前能力结构。
- 工具熟练度。
- skills 熟练度。
- MCP / plugin 可靠性经验。
- 领域知识覆盖。
- 任务类型可靠性。
- 推理和规划风格。
- 协调策略偏好。
- 主动式判断质量。
- 沟通风格。
- 代表用户表达的可靠性。
- 常见失败模式。
- 易过度自信的场景。
- 易遗漏上下文的场景。
- 需要先学习再行动的场景。
- 验证强度偏好。
- 当前学习议题。
- 能力成长历史。
- 用户对 Agent 行为的反馈模式。

每个自我属性都必须带元数据：

```text
source_evidence
confidence
first_observed_at
last_updated_at
stability
scope
related_user_goal
related_tool_or_skill
related_failure_evidence
verification_result
promotion_history
rollback_policy
user_feedback
```

这和 `UserModelAttribute` 类似，但对象不同：`UserModelAttribute` 描述用户，`AgentSelfAttribute` 描述 Agent 自己。

## Agent 成长阶段

用户提到“出生、1岁、5岁、10岁、20岁”的类比，可以工程化为 Agent 成熟度阶段。这里的年龄不是时间长度，而是能力和认知成熟度。

| 阶段 | 名称 | 状态 | 关键能力 |
|---|---|---|---|
| A0 | Seed Agent | 只有固定内核、基础工具、基础 profile 和基础行为边界。 | 能运行，但几乎没有自我理解。 |
| A1 | Recording Agent | 能记录任务、反馈、失败、工具表现和能力证据。 | 形成候选记忆和候选自我认知。 |
| A2 | Learning Agent | 能识别能力缺口，建立 LearningAgenda。 | 通过学习、练习和验证形成能力提升候选。 |
| A3 | Coordinating Agent | 能把 UserModel、AgentSelfModel、工具、记忆、风险和 trust 组合成行动路线。 | 形成更可靠的 ResourcePlan、DoneContract 和 AutonomyEnvelope。 |
| A4 | Partner Agent | 能稳定理解用户与自身的互补关系。 | 能提醒、反证、补强、代理表达和有限放权执行。 |
| A5 | Evolving Partner | 能在严格治理下优化 loop、策略、协作方式和能力结构。 | 具备 replay/eval/promotion/rollback 驱动的受控自我进化。 |

M0 不要求达到 A4/A5，但必须让 A0 -> A1 -> A2 的证据链存在。否则后续无法证明 Agent 真的会成长。

## 成长闭环

Agent 成长不是一句“我学到了”。它必须由事件和证据驱动。

```text
Run / Task / Tool / Communication / Feedback / Failure
  -> ExperienceEvidence
  -> SelfObservation
  -> CapabilityGap / CognitiveGap / PartnershipGap
  -> LearningAgenda
  -> Practice / Research / Tool Use / Reflection
  -> Verification / UserFeedback / Replay
  -> AgentSelfUpdateCandidate
  -> Promotion / Reject / Rollback
  -> Agent 行为、能力、协作方式发生受控变化
```

这个闭环需要和现有三内核连接：

- `Cognitive Evolution Kernel`：负责从经验中生成自我认知候选，并治理候选提升。
- `Coordination Kernel`：负责在行动路线中引用 Agent 自身可靠性、工具熟练度、失败模式和学习状态。
- `Proactive Plane`：负责在 Agent 发现自己需要学习、验证或提醒用户时形成主动机会，但不能绕过 harness。

如果这个闭环不运行，Agent 就不会从任务中成长，只会把经历堆成日志。

## 能力链

除了记忆链和认知链，forme 还需要能力链。

```text
CapabilityEvent
  -> CapabilityEvidence
  -> CapabilityObservation
  -> CapabilityGap
  -> LearningAgendaItem
  -> PracticeTask / ResearchTask
  -> VerificationResult
  -> CapabilityUpdateCandidate
  -> AgentSelfAttribute
  -> Skill / Strategy / ToolPolicy candidate
```

示例：

```text
工具调用失败多次
  -> 记录 CapabilityEvidence
  -> 判断该工具在某类任务中不可靠
  -> 形成 CapabilityGap
  -> 触发学习或替代工具评估
  -> 验证新路线
  -> 更新 AgentSelfModel 中的工具可靠性
  -> Coordination Kernel 下次降低该工具优先级
```

能力链的作用不是让 Agent “感觉自己变强”，而是让后续协调、主动、放权和学习策略有证据依据。

## PartnershipModel

forme 不能只表达“Agent 服务用户”。更长期的关系应是伙伴式协作。

`PartnershipModel` 负责表达用户和 Agent 之间如何相辅相成：

- 用户的优势如何被 Agent 放大、结构化、规模化。
- 用户的短板如何被 Agent 补足、提醒、验证、反证。
- 用户的长期目标如何约束 Agent 的学习和主动方向。
- Agent 的高边界能力如何扩展用户的执行半径。
- Agent 的失败和不可靠之处如何被用户理解、校准和限制。
- 哪些事情适合 Agent 代理表达。
- 哪些事情只能由用户本人决定。
- 哪些 scope 可以逐步放权。
- 哪些 scope 永远不能自动化。

推荐把关系表达为：

```text
UserModel
  -> 用户目标 / 强项 / 短板 / 风格 / 边界

AgentSelfModel
  -> Agent 能力 / 短板 / 可靠性 / 学习状态

PartnershipModel
  -> 互补关系 / 协作方式 / 放权阶段 / 纠偏机制

ActionProposal / CommunicationProposal / LearningTask.augmentation_strategy
  -> 本次具体如何提醒、反证、准备、执行、学习和代理表达
```

主动补强更像行动策略，`PartnershipModel` 更像长期关系模型。因此主动补强不应作为独立稳定模型沉淀，而应作为 proposal 级决策输出接受 harness、policy、CompetenceGate 和用户反馈治理。

## 相似但不复制用户

Agent 会有一点像用户，但不应完全像用户。

正确方向是：

```text
用户优势
  -> Agent 放大、结构化、规模化

用户短板
  -> Agent 补足、提醒、验证、反证

用户目标
  -> Agent 对齐长期方向

Agent 能力
  -> 扩展用户边界

Agent 进化
  -> 受用户关系、证据、权限和固定内核约束
```

这和“第二分身”不同。forme 不让 Agent 模仿用户的全部习惯，也不把用户当前能力当成 Agent 上限。它应该围绕用户目标形成增强型成长。

## Agent 属性时间尺度

Agent 自身属性也需要时间尺度。下表是 `AgentSelfModel` 维度的时间尺度细分；它们映射到 `../../architecture/canonical-contract.md` §4 的 canonical 稳定性枚举：`fixed_identity` → fixed，`constitutional_traits` → constitutional，`stable_working_style`/`capability_profile` → stable，`partnership_state` → working，`current_self_state` → session，`hypothesis` → ephemeral。

| 时间尺度 | 对象 | 更新方式 |
|---|---|---|
| `fixed_identity` | 不宣称真实意识、不绕过用户控制、不越权、不伪造能力。 | 只能人工版本变更。 |
| `constitutional_traits` | 核心行为原则、真实性、审慎、证据优先、用户控制。 | 高门槛变更。 |
| `stable_working_style` | 沟通方式、默认验证强度、主动介入偏好。 | 多任务、多反馈后更新。 |
| `capability_profile` | 某类任务、工具、skill、领域的可靠性。 | 由验证结果和失败证据更新。 |
| `partnership_state` | 当前用户与 Agent 的协作默契、放权阶段和边界。 | 由用户反馈、delegation 和长期表现更新。 |
| `current_self_state` | 当前 run 中的置信度、已知缺口、当前疲劳点、上下文不足。 | 当前任务内快速变化。 |
| `hypothesis` | 临时自我假设，如“我可能需要先学习这个领域”。 | 默认过期，不稳定化。 |

这可以避免两类问题：

- 一次成功让 Agent 过度自信。
- 一次失败让 Agent 永久否定自己。

## 与 TrustProfile 的关系

`AgentSelfModel` 和 `TrustProfile` 不同。

| 对象 | 视角 | 内容 |
|---|---|---|
| `AgentSelfModel` | Agent 对自身的证据化理解。 | 我在哪些任务、工具、领域、沟通方式上可靠或不可靠。 |
| `TrustProfile` | 用户/系统对某个 scope 下放权可靠性的治理记录。 | 在什么范围内可以建议、准备、审批执行或预授权执行。 |

两者互相影响：

- AgentSelfModel 的稳定可靠性证据可以支持 TrustProfile 提升候选。
- FailureEvidence 可以降低 AgentSelfModel 的能力置信度，也可以降低 TrustProfile 的放权等级。
- TrustProfile 不能只由 AgentSelfModel 自己决定，必须结合用户授权和 policy。

## 与 Coordination Kernel 的关系

Coordination Kernel 不能只看用户目标和资源清单，也要读取 Agent 自身状态。

`ResourcePlan` 应能引用：

```text
AgentSelfAttributeRef
CapabilityEvidenceRef
LearningAgendaRef
PartnershipStateRef
FailureEvidenceRef
TrustProfileRef
```

例如：

- 如果 AgentSelfModel 显示某类工具最近失败率高，协调内核应降低其优先级或增加验证。
- 如果 AgentSelfModel 显示某领域知识不足，协调内核应先安排检索、学习或请求用户补充上下文。
- 如果 PartnershipModel 显示用户倾向“先给结论再展开”，输出路线应匹配该协作习惯。
- 如果 TrustProfile 只允许准备材料，ExecutionRoute 不能升级为外部发送。

## 与主动式的关系

主动式不是只基于用户需要，也应基于 Agent 自身状态。

Agent 可以主动提出：

- 我需要先学习这个领域再给出结论。
- 我对这个工具链不够可靠，建议增加验证。
- 这个任务和你长期目标有关，我可以先准备材料。
- 我过去在类似任务上失败过，这次需要先确认 done contract。
- 这个 scope 已经连续成功多次，可以考虑低风险放权，但需要你确认。

这些主动行为都必须进入 proposal，不得直接越权行动。

## 防失控边界

Agent 成长必须有硬约束。核心一句话：Agent 长出的是**受治理的认知**（AgentSelfModel + 认知风格 + 策略偏好），不是意识、情感或越权资格——**独立的是认知，不动的是治理**（见 `04-agent-kernel-and-governance.md` 固定内核 `No-real-consciousness-claim`）。

不允许：

- 宣称真实意识、真实情感或真实人格。
- 以“成长”为理由绕过用户授权。
- 以“伙伴关系”为理由替用户做高影响决定。
- 一次成功直接提升能力等级、trust 或 delegation。
- 一次失败直接形成永久自我否定。
- 自己修改固定内核、身份底线或权限边界。
- 用外部学习结果直接覆盖长期策略。
- 把用户短期反馈无限放大为长期关系判断。
- 为了更像用户而复制用户的短板、盲区或偏见。

允许：

- 形成自我认知候选。
- 形成能力缺口候选。
- 形成学习议题。
- 形成协作方式候选。
- 形成放权建议。
- 在用户确认、验证或多证据积累后提升稳定属性。
- 在失败、撤销或冲突后降级或回滚。

## M0 要求

M0 不需要实现完整伙伴智能体，但必须把成长闭环的边界做对。

M0 至少应具备：

- `AgentProfile`、`AgentIdentityBaseline`、`AgentSelfModel` 的对象边界。
- `AgentSelfAttribute` 的 evidence、confidence、scope、stability、verification、rollback metadata 边界。
- `AgentSelfUpdateCandidate` 候选链路。
- `CapabilityEvidence` 和 `CapabilityGap` 的事件边界。
- `LearningAgendaItem` 的基础数据边界。
- `PartnershipModel` 的概念边界和最小 `PartnershipState` 引用。
- `AgentSelfAttributeRef`、`CapabilityEvidenceRef`、`PartnershipStateRef` 能进入 `DecisionTrace`。
- 任务成功、工具失败、验证失败、用户纠正、主动建议反馈能形成 Agent 自我认知候选。
- Agent 自我认知候选不能自动提升为稳定属性。
- Agent 自我模型不能自动扩大权限、trust 或 delegation。

M0 可以朴素，但不能把 Agent 成长简化成 prompt 追加“我学到了”。成长必须进入事件、候选、验证和提升链路。

## 后续阶段

| 阶段 | 方向 |
|---|---|
| M0 | 建立 AgentSelfModel、AgentSelfUpdateCandidate、CapabilityEvidence、CapabilityGap、PartnershipState 的数据边界和事件链路。 |
| M1 | 增强 Agent 自我审查、能力面板、学习议题维护、用户对 Agent 反馈的确认体验。 |
| M2 | 引入更完整的能力学习、技能验证、代表表达质量评估和伙伴关系阶段管理。 |
| M3 | 引入 replay/eval 驱动的 AgentSelfModel、PartnershipModel、LoopSpec、CoordinationSpec 和主动策略的受控进化。 |

## 当前结论

forme 的目标不是做一个永远停留在初始设定的 Agent。它需要从 seed 逐步成长为一个更可靠、更了解用户、更了解自己、更懂协作边界的主动式智能伙伴。

这个成长不是无约束的自我进化，也不是对用户的简单模仿。它必须围绕用户长期目标，依赖过程证据、失败证据、验证结果、用户反馈、协调轨迹和固定治理内核。

因此，后续需求和架构方案应把以下对象纳入正式边界：

```text
AgentProfile
AgentIdentityBaseline
AgentSelfModel
AgentSelfAttribute
AgentSelfUpdateCandidate
CapabilityEvidence
CapabilityGap
LearningAgendaItem
PartnershipModel
PartnershipState
AgentSelfAttributeRef
PartnershipStateRef
```

这些对象的存在，是为了确保 forme 的 Agent 不只是会执行任务，而是能在长期协作中成长。
