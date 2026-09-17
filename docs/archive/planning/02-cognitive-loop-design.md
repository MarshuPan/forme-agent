# 认知 Loop 设计

本文档定义 forme 的认知 Loop 与 Cognitive Plane。它承接 `01-initial-product-and-foundation-design.md`，用于把长期主动智能体方向落到可设计的架构概念中。V3 已在 `03-personal-augmented-agent-and-evolving-loop.md` 中把“第二分身”校准为“增强型个人主动智能体”，后续正式概念以 V3 文档为准。本文的 `Cognitive Plane / CognitiveLoop` 为历史术语，canonical §1 已收敛为三内核；同名对象口径以 `../../architecture/canonical-contract.md` 为准。

本文不是技术实现方案，也不是 PRD。它的作用是确认：认知结构在 forme 中不是静态角色设定、不是简单角色 prompt，而是一种横切的认知平面和可裁剪的认知 loop 架构。

状态说明：本文中的 M0/M1 切分是早期认知 Loop 专题规划。后续 `requirements/03-foundation-scope-decisions.md` 已经把 M0 修正为完整核心闭环，因此认知相关能力在 M0 不再只是预留事件，而是需要至少形成候选生成、证据记录和受治理的更新提议链路。

## 核心定位

认知结构应被定义为 `Cognitive Plane + CognitiveLoop + LoopSpec`：`Cognitive Plane` 是横切运行时的长期认知平面，`CognitiveLoop` 是认知 loop runner，`LoopSpec` 是可版本化的认知循环定义，用来驱动 Agent 的主动交互、自发学习、深度反思、补强策略和行为风格沉淀。

它解决的问题不是“这次任务如何执行”，而是：

- Agent 如何决定什么时候回应、什么时候等待、什么时候追问。
- Agent 如何从用户输入、环境事件、任务结果中提炼长期认知。
- Agent 如何围绕用户长期目标主动学习。
- Agent 如何通过多角色、多条件循环审议提升判断质量。
- Agent 如何把经验沉淀成 UserModel、AgentSelfModel、PartnershipModel、CognitiveMap、Memory、Skill 和 Principle，并在具体 proposal 中输出主动补强策略。

## 与现有 Harness/Loop 的关系

forme 应形成一套 harness 约束下的两类 loop：

```text
AgentHarness
  ├─ AgentLoop
  │   └─ 任务执行、工具调用、结果回填、final output
  └─ CognitiveLoop
      └─ 运行选定 LoopSpec，产出行动、学习和认知更新候选
```

`AgentHarness` 是运行容器，负责权限、状态、事件、存储、调度和生命周期。

`AgentLoop` 是执行层 loop，负责一次 run 内模型与工具的交替。

`CognitiveLoop` 是认知平面中的 loop runner，负责跨 run、跨 session 的认知演化。

`LoopSpec` 是 `CognitiveLoop` 运行的策略定义，描述阶段、角色、条件、输入、输出、退出规则、适用场景和风险边界。默认九阶段只是 seed `LoopSpec`，后续可以通过受控的版本化机制演进。

二者不能混在一起。执行层 loop 追求可控、可审计、可恢复；认知平面追求长期学习、反思、主动性和判断质量。认知平面可以提出行动，但不能绕过 harness 的 policy、approval 和 event store。

## Cognitive Plane 接口边界

`Cognitive Plane` 是横切平面，不是独立 runtime。它的接口应保持收敛：

| 接口 | 含义 | 边界 |
|---|---|---|
| Event subscription | 订阅 `EventStore`、session state、tool result、用户反馈和外部观察信号。 | 只能读取授权范围内的事件和上下文。 |
| `ActionProposal` | 提出回应、追问、等待、学习、执行、记录等行动建议。 | 不能直接调用工具，必须交给 `AgentHarness` 生成受 policy 约束的 `RunRequest`。 |
| `CognitiveUpdateProposal` | 提出 memory、skill、principle、UserModel、AgentSelfModel、PartnershipModel 或 CognitiveMap 的更新候选。 | 不能直接写长期认知，必须经过验证、冲突检查、审批或提升流程。 |
| `LearningTask` | 提出主动学习议题、来源策略和验证要求。 | 学习执行仍然走 `AgentLoop`、tool runtime、network policy 和 source policy。 |

这个边界保证 CognitiveLoop 是主导认知演化的机制，但不是绕过执行治理的后门。

## 为什么需要认知 Loop

现有大多数 Agent 是一问一答或任务驱动：

- 用户提问。
- Agent 构建上下文。
- Agent 调工具。
- Agent 回答。

这种结构不足以形成增强型个人主动智能体体验，因为它缺少长期认知循环：

- 不会自然等待更多上下文。
- 不会根据用户长期目标主动学习。
- 不会把事件升级成原则和判断模型。
- 不会持续校正自己对用户的理解。
- 不会形成稳定、可审计、可撤销的认知沉淀。

认知 Loop 的作用，就是把这些能力从“偶然行为”变成“系统机制”。

## seed LoopSpec：默认九阶段

建议采用九阶段作为 seed `LoopSpec`。九阶段不是固定流程，也不是为了形式，而是为了覆盖从观察到沉淀的完整闭环；具体运行时应由 `CognitiveLoop` 按事件类型、风险等级、复杂度和是否影响长期认知选择或裁剪 `LoopSpec`。

| 阶段 | 输入 | 输出 | 作用 |
|---|---|---|---|
| 观察 | 用户输入、环境事件、任务结果、外部资料 | Observation | 收集值得进入认知系统的事实和信号。 |
| 定位 | Observation、UserModel、长期目标、当前项目 | Orientation | 判断这件事和用户、项目、风险、目标的关系。 |
| 分解 | Orientation、角色集、条件集 | Decomposition | 从多角色、多条件视角拆问题。 |
| 推演 | Decomposition、历史经验、外部知识 | HypothesisSet | 形成假设、路径、收益、风险、反证和依赖。 |
| 决策 | HypothesisSet、policy、用户状态 | CognitiveDecision | 选择回应、等待、追问、学习、行动、记录或沉淀。 |
| 行动 | CognitiveDecision | ActionProposal / LearningTask / ReflectionTask | 只产出行动、学习或反思提议；真实执行必须交给 AgentHarness。 |
| 验证 | 行动结果、证据、工具输出 | VerificationResult | 判断结果是否可靠、是否需要重试、是否可沉淀。 |
| 反思 | VerificationResult、event trace、用户反馈 | ReflectionRecord | 提炼经验、失败原因、认知变化、原则候选。 |
| 沉淀 | ReflectionRecord、候选知识 | CognitiveUpdateProposal | 提交 Memory/Skill/Principle/UserModel/AgentSelfModel/PartnershipModel/CognitiveMap 的候选更新。 |

默认路径可以按以下方式裁剪：

| 事件类型 | 建议路径 | 目的 |
|---|---|---|
| 低影响消息 | 观察 -> 定位 -> 决策 | 快速判断是否回应、等待、追问或只记录。 |
| 普通用户反馈 | 观察 -> 定位 -> 反思 -> 候选沉淀 | 把反馈转成可审计的经验候选，而不是立刻改长期画像。 |
| 主动学习任务 | 观察 -> 定位 -> 分解 -> 推演 -> 行动提议 -> 验证 -> 反思 -> 候选沉淀 | 保证学习有来源、证据、验证和冲突检查。 |
| 原则、用户画像或能力模型更新 | 观察 -> 定位 -> 分解 -> 推演 -> 决策 -> 行动提议/确认 -> 验证 -> 反思 -> 候选沉淀 | 高影响更新默认走完整路径，并需要确认或可回滚审批。 |

这条规则解决两个问题：低风险事件不被重流程拖慢，高影响事件也不会被一次模型判断直接写入长期认知。

## 多角色不是多 Agent 堆叠

认知 Loop 中的角色不是简单开多个 Agent 聊天。它更接近认知职能分工。

建议 M1 角色集：

| 角色 | 责任 |
|---|---|
| Observer | 过滤事件，识别值得注意的信号。 |
| Strategist | 判断目标、路径、优先级和长期影响。 |
| Researcher | 判断是否需要学习外部资料，设计学习任务。 |
| Critic | 寻找反证、风险、过度自信和幻觉。 |
| Executor | 把认知决策转换为可执行 RunRequest。 |
| Verifier | 检查证据、结果和工具输出可靠性。 |
| Reflector | 从结果中提炼经验和认知变化。 |
| Archivist | 决定沉淀到 memory、skill、principle 还是丢弃。 |
| PersonaGuard | 维护 AgentSelfModel、行为边界和用户信任。 |
| Augmentor | 判断如何补强用户：顺从、提醒、追问、反证、建议、学习或请求确认。 |

这些角色可以由同一个模型在不同 prompt/mode 下执行，也可以由子代理执行。M1 更建议先做“角色模式”，不要直接做复杂多 Agent 网络。

## 多条件迭代

认知 Loop 不只是多角色，还需要多条件约束。否则角色讨论会变成发散。

建议条件集：

- 用户长期目标。
- 当前项目上下文。
- 时间敏感性。
- 风险等级。
- 证据可靠性。
- 是否需要用户确认。
- 是否值得长期沉淀。
- 是否可能污染 memory。
- 是否可以低成本验证。
- 是否涉及隐私、凭据、文件修改、网络或外部副作用。

每轮 CognitiveLoop 都应把这些条件作为约束，而不是让模型自由发挥。

## 主动式交互

主动式交互不是“更频繁地说话”，而是更像真实协作中的非对称交流。

CognitiveLoop 应支持以下决策：

- `respond_now`：立即回应。
- `wait_for_more`：暂不回应，等待更多上下文。
- `ask_clarification`：主动追问关键缺口。
- `summarize_later`：积累到一定程度后再总结。
- L0 Observe：只记录授权事件，不打扰用户。
- L1 Suggest：主动追问、提醒、反证或提出建议，用户可忽略。
- L2 Prepare：主动准备草稿、资料包、分析或计划，但不产生外部副作用。
- L3 Act With Approval：主动提出可执行行动，并等待用户确认。
- `run_low_risk_action`：在 policy 允许范围内执行低风险动作。

这需要 `ProactiveInteractionManager`，它读取 Observation、AttentionScore、UserModel、当前任务状态和打扰成本，决定是否发起交互。

## 自发性学习

自发学习必须受治理，否则会变成噪声源。

建议学习链路：

```text
LearningAgenda
  -> SourcePolicy
  -> Search/Fetch/Read
  -> CandidateKnowledge
  -> Verification
  -> Reflection
  -> KnowledgePromotion
  -> Memory/Skill/Principle
```

关键对象：

| 对象 | 作用 |
|---|---|
| `LearningAgenda` | 维护主动学习议题，来自用户目标、项目需求、技术趋势、失败反思。 |
| `SourcePolicy` | 定义可信来源优先级，如官方文档、论文、GitHub、权威博客。 |
| `CandidateKnowledge` | 候选知识，不能直接进入长期记忆。 |
| `KnowledgeVerifier` | 检查证据、版本、适用范围、冲突和过期风险。 |
| `KnowledgePromotion` | 把候选知识提升为 memory、skill、principle 或丢弃。 |

M1 不做完整主动学习闭环，只允许形成学习议题、来源建议和低风险候选资料收集计划，默认需要用户确认后执行。M2 再引入 `LearningAgenda -> SourcePolicy -> CandidateKnowledge -> Verification -> KnowledgePromotion` 的主动学习链路；任何沉淀到长期认知的学习结果都必须可审计。

## 深度思考与沉淀

需要区分四类记录：

| 类型 | 作用 |
|---|---|
| Daily/Event Notes | 低层事实记录，说明发生了什么。 |
| ReflectionRecord | 从事件中提炼经验、矛盾、失败原因、判断变化。 |
| BeliefUpdate | 对用户、项目、技术、世界状态的认知更新。 |
| Principle | 长期稳定的行为准则和判断原则。 |

这四类不能混成一个 memory。流水账只能说明发生过什么，不能构成 Agent 的认知主体。真正影响未来行为的是 Reflection、BeliefUpdate 和 Principle。

## 认知写入规则

长期认知写入必须有规则：

- 必须记录来源。
- 必须记录更新时间。
- 必须记录适用范围。
- 必须记录置信度。
- 必须支持撤销或过期。
- 与旧认知冲突时必须保留冲突关系，而不是直接覆盖。
- 涉及用户画像和长期原则时，默认需要用户确认或可回滚。

这条边界非常重要。否则 Agent 会越学越偏，最后形成不可解释的行为。

## 数据对象建议

| 对象 | 关键字段方向 |
|---|---|
| `Observation` | id、source、content、timestamp、scope、sensitivity、attention_score。 |
| `Orientation` | observation_id、related_goals、related_projects、urgency、risk、context_refs。 |
| `CognitiveDecision` | decision_type、reason_summary、required_approval、next_action、expires_at。 |
| `LearningAgendaItem` | topic、reason、priority、source_policy、status、reviewed_at。 |
| `CandidateKnowledge` | source、summary、evidence、confidence、conflicts、promotion_target。 |
| `ReflectionRecord` | source_events、lesson、failure_mode、principle_candidate、confidence。 |
| `BeliefUpdate` | target、old_belief、new_belief、reason、confidence、valid_until。 |
| `Principle` | statement、source_ref、scope、priority、created_at、updated_at、revoked_at。 |
| `UserModel` | preferences、goals、risk_profile、communication_style、working_style。 |
| `AgentSelfModel` | capabilities、limits、confidence、tool_boundaries、capability_gaps、behavior_constraints。 |
| `augmentation_strategy` | proposal 级字段：user_gap、strategy、intervention_level、challenge_reason、confirmation_required。 |
| `LoopSpec` | id、version、stages、roles、conditions、inputs、outputs、exit_rules、risk_level、status。 |
| `LoopFitness` | loop_spec_id、success_rate、verification_rate、cost、interruption_rate、user_feedback、promotion_signal。 |

## 事件类型建议

为了后续可审计和可恢复，M0 就应预留认知事件类型：

- `ObservationRecorded`
- `AttentionScored`
- `CognitiveLoopStarted`
- `CognitiveNodeCompleted`
- `CognitiveDecisionMade`
- `LearningAgendaCreated`
- `CandidateKnowledgeRecorded`
- `ReflectionRecorded`
- `BeliefUpdateProposed`
- `PrincipleProposed`
- `AgentSelfModelUpdateProposed`
- `PartnershipStateUpdateProposed`
- `LoopSpecProposed`
- `LoopFitnessRecorded`
- `LoopPromotionProposed`
- `LoopRollbackRequested`
- `CognitiveUpdateApproved`
- `CognitiveUpdateRejected`

M0 不能只定义事件。当前正式口径是：M0 至少要有一个最小但在运行的 CognitiveLoop 链路，用于在 post-turn 或 idle 边界生成低置信候选；它可以很朴素，但必须经过 harness、policy、event store 和 candidate-before-promotion 治理，且不能自动提升稳定认知。

## M0/M1/M2/M3 切分

M0 做最小运行中的 CognitiveLoop：

- 预留认知事件类型。
- 定义 UserModel、AgentSelfModel、PartnershipModel、LoopSpec、PrincipleStore、ReflectionRecord 的数据边界。
- memory/skills/profile 不和认知平面混淆。
- policy 支持认知写入需要审批。
- post-turn / idle 触发一次最小认知候选生成。
- 生成 `UserAttributeCandidate`、`AgentSelfUpdateCandidate`、`PartnershipStateCandidate`、`CognitiveMapUpdateProposal` 或 reflection candidate 时必须保持低置信候选状态。
- 候选必须带 evidence、scope、confidence、stability、来源和冲突信息。
- M0 不自动提升稳定认知，不自动修改默认 LoopSpec，不自动扩大权限。

M1 做 CognitiveLoop 体验增强：

- 默认九阶段模板，可按事件类型裁剪路径。
- seed LoopSpec 的版本化定义。
- 角色模式，不做复杂多 Agent 网络。
- 非对称交互策略。
- 周期性 reflection。
- candidate memory/principle/skill/UserModel/AgentSelfModel/PartnershipModel 提议。
- 用户确认后沉淀。

M2 做主动学习增强：

- LearningAgenda。
- SourcePolicy。
- CandidateKnowledge。
- KnowledgePromotion。
- CapabilityGap。
- CapabilityUpdateProposal。
- GitHub/论文/官方文档等来源的可信学习链路。
- 低风险自动收集，高影响沉淀需要确认。

M3 再考虑 loop 自进化和复杂认知协作：

- 多角色认知协作网络。
- 认知冲突图谱。
- 个性化行为策略。
- LoopRegistry。
- LoopFitness。
- LoopEvolutionProposal。
- LoopPromotionPolicy。
- replay / simulation / A-B 对比。

## 安全边界

认知 Loop 不能绕过 AgentHarness。

必须遵守：

- 主动学习不能绕过网络 policy。
- 主动行动不能绕过 approval。
- 认知写入不能绕过审计。
- 用户画像不能无来源自动更新。
- 长期原则不能被一次失败或一次网页内容直接覆盖。
- 自发学习结果不能直接成为事实，只能先成为候选知识。
- LoopSpec 不能由一次反思直接替换默认策略，必须经过 fitness、review、promotion 和 rollback 边界。

## 当前结论

认知 Loop 应作为 forme 认知平面中的 loop runner，而不是静态角色设定。它与 `AgentLoop` 相辅相成：`AgentLoop` 让 Agent 能执行任务，`CognitiveLoop` 通过 `LoopSpec` 让 Agent 能形成长期认知、主动性和增强型个人主动智能体能力。

当前最稳妥的路线是：M0 做好通用 AgentHarness/AgentLoop 底座并预留认知事件和存储边界；M1 引入最小 CognitiveLoop 和 seed LoopSpec；M2 发展主动学习、能力成长和知识提升机制；M3 再做 loop 自进化和高阶治理。
