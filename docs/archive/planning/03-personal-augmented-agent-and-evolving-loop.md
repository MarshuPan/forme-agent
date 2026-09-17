# 增强型个人主动智能体与自进化 Loop 设计

本文档是 forme V3 的概念校准文档。它修正前文中“第二分身”的表述。forme 的长期目标不是复制用户，也不是让 Agent 越来越像用户，而是构建一个足够理解用户、能够补强用户、具备主动协作能力、并能持续优化自身的增强型个人主动智能体。

本文不是技术实现方案，也不是 PRD。它用于定义长期产品方向、认知模型和 loop 自进化边界，后续需求文档和技术方案应以这里的概念为准。

状态说明：本文保留 V3 阶段对“增强型个人主动智能体”的概念校准。后续 `requirements/03-foundation-scope-decisions.md` 已经把 M0 从“只预留边界”修正为完整核心闭环，因此本文中的 M0 切分只代表当时的探索，不代表当前最终 M0 范围。

## 核心定位

forme 的长期形态应定义为：

> 增强型个人主动智能体：深度理解用户，但不复制用户；尊重用户目标，但不受用户当前能力上限限制；能够主动学习、主动沟通、主动补强，并在安全边界内持续进化自己的认知和工作方式。

这个定位有三个关键点：

- 不是用户复制体：Agent 不以“像用户”为目标。
- 不是普通助手：Agent 不只是被动响应命令。
- 不是无边界自治系统：Agent 的主动性和进化能力必须受内核、policy、approval、event store 和验证机制约束。

## 为什么不能叫第二分身

“第二分身”容易造成一个错误目标：让 Agent 学得越来越像用户。这个方向有明显风险：

- 如果用户在某些领域能力不足，Agent 复制用户会放大不足。
- 如果用户习惯存在盲区，Agent 复制习惯会固化盲区。
- 如果用户判断有偏差，Agent 复制判断会让系统变成更稳定的偏差执行器。
- 如果 Agent 只追随用户表达，而不理解用户长期利益，就无法形成真正的主动补强。

因此，forme 应把“理解用户”和“复制用户”明确分开。

`UserModel` 的目标是理解用户，不是克隆用户。Agent 需要知道用户的目标、偏好、风格、边界、短板和风险，但它的行为目标应是增强用户的长期能力与执行质量。

## 目标体验的正确理解

forme 期望的主动智能体体验是：

- 足够了解用户。
- 能主动提醒、判断、建议和执行。
- 能处理复杂工具和外部系统。
- 能补足用户没有显式说出的上下文。
- 能在关键时刻提出反证、风险或更优方案。

这种体验不应被理解为“用户的第二个自己”，而应被理解为“围绕用户目标工作的高能力主动智能体”。

## 长期对象与主动补强策略

V3 阶段曾把 `AugmentationModel` 设想为与 `UserModel`、`AgentSelfModel` 并列的长期对象。当前 canonical 口径已修正：长期对象不再包含独立 `AugmentationModel`，主动补强应作为 `ActionProposal`、`CommunicationProposal` 或 `LearningTask` 上的 `augmentation_strategy` 决策输出。长期关系状态由 `PartnershipModel` 承担。

| 模型 | 解决的问题 | 不应该做什么 |
|---|---|---|
| `UserModel` | 用户是谁、想要什么、偏好什么、风险边界是什么、能力短板在哪里。 | 不把用户当前能力当成 Agent 能力上限。 |
| `AgentSelfModel` | Agent 自己会什么、不会什么、置信度如何、工具边界在哪里、当前短板是什么。 | 不虚构能力，不把一次成功当成长期能力。 |
| `PartnershipModel` | 用户与 Agent 如何协作、互补、纠偏、放权、成长。 | 不把伙伴关系变成越权理由，不把用户短板固化成标签。 |

`augmentation_strategy` 是关键决策输出。它决定 forme 和普通 memory agent 的区别：Agent 不只是记住用户，而是围绕用户目标在具体情境中做增强。

## 主动补强策略的职责

`augmentation_strategy` 应至少负责：

1. 识别用户长期目标和当前任务之间的关系。
2. 识别用户在信息、判断、执行、验证、记忆、时间管理上的短板。
3. 判断这次交互应该顺着用户推进，还是应该追问、提醒、反证或建议替代方案。
4. 判断 Agent 自己是否需要先学习或调用工具，而不是直接回答。
5. 判断哪些经验应沉淀为用户偏好，哪些应沉淀为 Agent 能力，哪些应沉淀为长期原则。
6. 控制主动程度：不打扰、轻提醒、正式建议、请求确认、低风险自动执行。
7. 防止“过拟合用户”：当用户短期表达和长期目标冲突时，优先暴露冲突，而不是机械服从。

## 主动协作能力

增强型个人主动智能体至少需要以下主动协作机制：

| 能力 | 含义 |
|---|---|
| 主动观察 | 从对话、任务、文件、项目、时间、反馈和外部来源中发现值得关注的信号。 |
| 主动追问 | 在目标、约束、风险、成功标准不清楚时主动补齐关键上下文。 |
| 主动提醒 | 当用户遗漏风险、约束、时机或长期目标时给出提醒。 |
| 主动反证 | 当用户判断可能偏离事实或目标时提出反证和替代路径。 |
| 主动学习 | 根据长期目标和当前短板形成学习议题，先进入候选知识，不直接污染长期认知。 |
| 主动沉淀 | 从任务结果和反馈中提炼经验、能力更新、原则候选和 loop 改进候选。 |
| 主动执行 | 只在 policy 允许的低风险范围内执行，高影响动作必须确认。 |

主动不是更频繁说话。主动的核心是时机、价值和边界：该说时说，该问时问，该沉默时沉默，该执行时执行。

## Agent 能力成长

Agent 的成长不能只依赖用户教它。它应有自己的能力成长链路：

```text
CapabilityGap
  -> LearningAgenda
  -> SourcePolicy
  -> CandidateKnowledge
  -> PracticeTask
  -> Verification
  -> Reflection
  -> CapabilityUpdateProposal
  -> Skill/Principle/LoopSpec 更新候选
```

关键边界：

- 学习结果先进入候选区。
- 能力提升必须有来源、证据和验证。
- 新能力不能自动获得更高权限。
- 学习到的内容不能直接覆盖用户偏好或长期原则。
- 任何会影响长期行为的能力更新，都必须可审计、可回滚、可降级。

## Loop 的固定内核与可进化部分

九阶段 CognitiveLoop 只能作为 seed loop，不应成为永久固定的思考方式。真正可持续的设计应区分三层：

| 层级 | 稳定性 | 内容 |
|---|---|---|
| 固定内核 | 高 | 安全、权限、审计、真实性、隐私、用户确认、长期写入规则、不能绕过 Harness。 |
| 稳定认知骨架 | 中 | 观察、定位、决策、行动、验证、反思、沉淀等基本职责。 |
| 可进化策略 | 低到中 | 不同任务的 loop 模板、阶段顺序、角色分工、触发条件、主动沟通策略、学习策略。 |

这解决了一个取舍问题：如果全部固定，Agent 无法进化；如果全部可变，Agent 会漂移。forme 应固定内核，版本化骨架，持续优化策略。

## LoopSpec 机制

后续应把认知 loop 从“一个写死流程”升级为 `LoopSpec`：

| 对象 | 含义 |
|---|---|
| `LoopSpec` | 某一种 loop 模板，定义阶段、输入、输出、退出条件和适用场景。 |
| `LoopRegistry` | 保存可用 loop 模板、版本、适用范围和状态。 |
| `LoopSelectionPolicy` | 根据任务类型、风险、复杂度、用户状态选择 loop。 |
| `LoopFitness` | 衡量 loop 的效果，如准确性、成本、打扰程度、验证通过率、沉淀质量。 |
| `LoopEvolutionProposal` | 对 loop 的新增、调整、禁用或替换建议。 |
| `LoopPromotionPolicy` | 决定候选 loop 何时能进入默认或高信任状态。 |

M1 可以只有一个 seed loop。M2 只记录 `LoopFitness` 和候选问题，M3 再加入 `LoopRegistry`、`LoopSelectionPolicy`、`LoopEvolutionProposal` 和 `LoopPromotionPolicy`。

## Loop 进化流程

Loop 进化不应由模型一次性决定，而应走候选和提升流程：

```text
RunTrace / ReflectionRecord / FailureMode / UserFeedback
  -> LoopFitness 评估
  -> LoopEvolutionProposal
  -> Simulation / Replay / A-B 对比
  -> Review / Approval
  -> LoopRegistry 更新
  -> 灰度使用
  -> 回滚或提升
```

这意味着 Agent 可以学习更好的思考方式，但不能每天随意改变底层行为。改变必须有证据、有版本、有范围、有回滚。

## 防偏离机制

自进化系统最大的风险是偏离。forme 至少需要以下防偏离机制：

- 核心原则不可被普通学习任务修改。
- 高影响 loop 变更必须人工确认或经过严格验证。
- loop 变更必须记录来源、原因、预期收益、风险和回滚方式。
- 长期用户画像不能被一次对话直接覆盖。
- AgentSelfModel 不能因为一次成功就提升能力等级。
- 主动补强策略不能以“补强用户”为理由绕过用户控制权。
- 主动学习不能绕过 source policy 和 network policy。
- 主动执行不能绕过 approval policy。

## 与 AgentHarness 的关系

增强型个人主动智能体仍然必须是 Harness-first：

- `AgentHarness` 负责 run/session/turn、policy、event、store、approval、tool runtime、resume 和 verification。
- `Cognitive Plane` 负责观察、学习、反思、候选认知更新和 loop 进化建议。
- 主动补强策略负责决定本次如何围绕用户目标做补强。
- `LoopSpec` 决定某类认知任务如何运行。
- 所有真实行动都必须回到 `AgentHarness`。
- 所有长期写入都必须经过候选、验证、提升和审计。

这保证 Agent 可以变强，但不会绕过底座。

## M0/M1/M2/M3 切分

M0 固定最小运行边界：

- 保留 Cognitive Plane / Cognitive Evolution Kernel 事件类型。
- 定义 `UserModel`、`AgentSelfModel`、`PartnershipModel`、`LoopSpec` 的数据边界。
- 运行 post-turn / idle 触发的最小 CognitiveLoop，生成低置信候选更新和审计事件。
- seed loop 可采用默认九阶段模板，但不做自动进化。
- 候选更新不能自动提升为稳定用户画像、稳定 Agent 自我属性、稳定伙伴关系或默认策略。

M1 做最小增强型主动能力：

- `UserModel` 基础结构。
- `AgentSelfModel` 基础结构。
- `PartnershipModel` 基础结构。
- 主动补强策略的人工或半自动规则。
- 非对称交互策略。
- 反思和候选认知更新。
- seed loop 的按事件裁剪。

M2 做主动学习和能力成长：

- `LearningAgenda`。
- `CapabilityGap`。
- `CandidateKnowledge`。
- `CapabilityUpdateProposal`。
- source policy、verification、knowledge promotion。
- 低风险学习自动化，高影响沉淀仍需确认。

M3 做 loop 自进化：

- `LoopRegistry`。
- `LoopFitness`。
- `LoopEvolutionProposal`。
- replay / simulation / A-B 对比。
- loop promotion / rollback。
- 更完整的长期主动协作体验。

## 对既有文档的修正关系

本文档不推翻 `01-initial-product-and-foundation-design.md` 和 `02-cognitive-loop-design.md` 的 harness、runtime、Cognitive Plane 基线，但修正其中“第二分身”的概念方向：

- “第二分身”应降级为历史表述或体验类比，不再作为正式目标。
- 正式目标应改为“增强型个人主动智能体”。
- `UserModel` 不能单独承担长期人格方向，必须和 `AgentSelfModel`、`PartnershipModel` 以及 proposal 级主动补强策略配套。
- 九阶段 loop 是 seed loop，不是永久固定流程。
- 自进化能力应走 `LoopSpec / LoopRegistry / LoopFitness / LoopEvolutionProposal / LoopPromotionPolicy`，而不是直接让模型修改自己的思考方式。

## 当前结论

forme 的长期目标应从“像用户”转向“增强用户”。它需要足够了解用户，但不复制用户；需要能主动协作，但不越过用户控制；需要能自我学习和自我进化，但不能绕过固定内核、审计、验证和提升流程。

因此，V3 的核心主线应是：

```text
AgentHarness
  -> Cognitive Plane
  -> UserModel + AgentSelfModel + PartnershipModel
  -> ActionProposal / CommunicationProposal / LearningTask.augmentation_strategy
  -> Seed CognitiveLoop
  -> LoopSpec / LoopRegistry / LoopFitness
  -> 受控的自我学习与自我进化
```

这个方向可以把主动智能体体验、通用 Agent 底座、主动学习、深度反思和可控自进化放到同一个架构里，而不是把它们做成互相脱节的功能点。
