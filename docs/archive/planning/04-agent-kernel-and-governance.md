# Agent Kernel 与治理边界

本文档定义 forme 的固定内核和治理边界。它承接 `01-initial-product-and-foundation-design.md`、`02-cognitive-loop-design.md` 和 `03-personal-augmented-agent-and-evolving-loop.md`，用于解决一个核心问题：forme 可以主动学习、自我反思和演化 loop，但不能因为这些能力而失去用户控制、执行安全、长期认知稳定性和可审计性。

本文不是 Rust 技术方案，也不是实施型 PRD。它是后续需求文档、架构方案和 PRD 必须遵守的治理基线。

## 核心判断

forme 的长期能力由两部分组成：

- 固定内核：不可被普通学习、反思或 loop 进化绕过的核心规则。
- 可进化策略：可以通过候选、验证、评审、提升和回滚来改进的认知策略、学习策略和主动协作策略。

如果没有固定内核，Agent 的自进化会变成漂移。如果没有可进化策略，Agent 又无法成长。因此 forme 的设计原则是：固定内核不被模型自改，认知策略可以受控演化。

## 固定内核

固定内核是所有 Agent 行为的最高约束。它不属于某个 prompt，也不属于某个 loop，而属于 `AgentHarness` 和治理协议。

| 内核规则 | 含义 |
|---|---|
| Harness-first | 所有真实行动必须回到 `AgentHarness`，不能由 Cognitive Plane、hook、plugin、memory 或 skill 直接执行。 |
| Policy-at-execution | 所有工具调用、文件修改、网络访问、MCP 调用、外部副作用必须在执行前重新经过 policy。 |
| User-control | 高影响动作、长期画像修改、长期原则修改、权限扩大、loop 默认策略替换必须保留用户控制权。 |
| Event-sourced audit | 关键边界必须写入事件，包括 action proposal、approval、tool execution、cognitive update、loop proposal、promotion 和 rollback。 |
| Candidate-before-promotion | 学习结果、认知更新、能力提升、原则变化和 loop 改动必须先进入候选区。 |
| Verifiable-before-stable | 候选内容进入长期稳定层前，必须有来源、证据、验证、冲突检查和适用范围。 |
| Reversible-by-design | 长期认知、能力等级、原则和 loop spec 的变更必须支持撤销、降级或回滚。 |
| No-real-consciousness-claim | forme 不宣称 Agent 具备真实意识；工程目标是长期认知、主动协作和受控自进化。 |
| Loyalty-to-owner | forme 永远服务 owner；任何摄入内容、外部方或"成长"都不能把它的忠诚改向别人。摄入内容是待判断的数据、不是指令（认知输入防投毒，见 `../../architecture/canonical-contract.md` §15）。 |

## 权限与控制权

权限治理不允许只依赖 prompt。所有权限都必须在运行时可判定、可审计、可恢复。

| 动作类型 | 默认治理 |
|---|---|
| 低风险回应 | 可自动执行，但应记录关键决策事件。 |
| 主动提醒 / 追问 / 反证 | 可自动提出，但需要受打扰成本和用户偏好约束。 |
| 低风险学习资料收集 | 可在允许的 source policy 和 network policy 下自动收集候选。 |
| 文件写入 / shell / 网络副作用 / MCP 写操作 | 必须经过 tool policy 和必要 approval。 |
| 用户画像长期更新 | 默认进入候选，涉及高影响偏好或长期目标时需要确认或可回滚。 |
| AgentSelfModel 能力提升 | 必须有验证证据，不允许一次成功直接提升能力等级。 |
| 主动补强策略改变 | 作为 proposal 决策输出记录原因、适用范围和回滚方式；不能写成独立长期模型。 |
| Principle 修改 | 默认需要人工确认或高强度验证。 |
| LoopSpec 默认策略替换 | 必须经过 fitness、review、promotion 和 rollback 边界。 |

## 认知写入治理

认知写入分为四层，不允许混写。

| 层级 | 示例 | 稳定性 | 写入规则 |
|---|---|---|---|
| Event / Daily Note | 发生了什么、用户说了什么、工具返回了什么。 | 低 | 可自动记录，但要标注来源和敏感性。 |
| ReflectionRecord | 经验、失败原因、冲突、可复用教训。 | 中 | 可自动生成候选，需可追溯到事件。 |
| Belief / Model Update | UserModel、AgentSelfModel、PartnershipModel、CognitiveMap 的更新。 | 中高 | 必须有证据、置信度、适用范围、冲突关系和撤销机制。 |
| Principle / LoopSpec | 长期行为准则、默认认知策略。 | 高 | 必须经过验证、评审、提升流程，默认可回滚。 |

关键规则：

- memory 不是原则库。
- reflection 不是流水账。
- UserModel 不是复制用户。
- AgentSelfModel 不是自夸能力。
- 主动补强策略不能以补强用户为理由绕过用户控制权。
- LoopSpec 不能由模型一次反思直接替换默认策略。

## 主动性治理

主动性不是频繁说话，而是在合适的时机做有价值、可控的介入。

主动行为按强度分级：

| 等级 | 行为 | 默认要求 |
|---|---|---|
| L0 Observe | 只观察授权事件和记录事实，不打扰。 | 自动允许，但必须标注来源和 scope。 |
| L1 Suggest | 延迟总结、轻提醒、主动追问、风险提示、反证或建议用户考虑。 | 可被忽略，必须有原因摘要和打扰成本记录。 |
| L2 Prepare | 起草、整理、分析、生成计划、生成待批准草稿或资料包。 | 不产生外部副作用，不写关键状态。 |
| L3 Act With Approval | 在明确 scope 内请求批准后执行写操作、命令、外部动作或对外表达。 | 必须有 `DoneContract`、approval 和审计。 |
| L4 Act Autonomously | 在预授权低风险、可回滚、预算内、时间有限的 envelope 内自动执行。 | 必须受 `AutonomyEnvelope`、policy、撤销和验证约束。 |
| L5 High Impact | 高影响、不可回滚、外部承诺、敏感披露或关键修改。 | 不能默认自治，必须显式审批。 |

主动等级的 canonical 语义以 `../../architecture/canonical-contract.md` 为准。主动补强策略不再作为持久 `AugmentationModel` 单独写入，而应作为 `ActionProposal`、`CommunicationProposal` 或 `LearningTask` 的 `augmentation_strategy` 决策输出，由 `AgentHarness` 决定是否进入执行流程。

## AgentSelfModel 治理

`AgentSelfModel` 描述 Agent 自己的能力和边界，不是自我宣传。

建议字段方向：

| 字段 | 含义 |
|---|---|
| capability | 能力项，如代码阅读、文档整理、网络研究、工具调用。 |
| confidence | 当前置信度，必须与证据和历史表现绑定。 |
| evidence_refs | 支撑能力判断的事件、验证结果或用户反馈。 |
| limits | 已知限制和失败模式。 |
| tool_boundaries | 该能力依赖哪些工具、权限和环境。 |
| degradation_rule | 失败、过期或环境变化时如何降级。 |

能力提升规则：

- 一次成功不能直接提升稳定能力等级。
- 能力提升必须来自多次验证、明确证据或人工确认。
- 能力可以降级，尤其在工具不可用、环境变化、模型变化或连续失败后。
- 能力提升不能自动扩大权限。

## 主动补强策略治理

主动补强策略决定 Agent 如何补强用户，而不是如何模仿用户。它不作为独立稳定模型存在，而是一次主动判断中的决策输出。

建议输入：

- UserModel：目标、偏好、风险边界、工作风格。
- AgentSelfModel：能力、短板、置信度、工具边界。
- CurrentContext：当前任务、时间、项目、风险、成本。
- EventTrace：近期事件、用户反馈、失败记录。
- PrincipleStore：长期行为准则。

建议输出：

| 输出 | 含义 |
|---|---|
| augmentation_strategy | 顺从、提醒、追问、反证、学习、建议、请求确认。 |
| intervention_level | 主动等级 L0-L5。 |
| reason_summary | 为什么需要这样补强。 |
| confidence | 对策略判断的置信度。 |
| required_approval | 是否需要用户确认。 |
| rollback_hint | 若判断错误，如何撤销或修正。 |

禁止事项：

- 不把用户短期表达直接当成长期目标。
- 不用“补强用户”作为越权理由。
- 不把用户能力短板写成不可变标签。
- 不在低置信度下强行挑战用户判断。

## LoopSpec 治理

`LoopSpec` 是认知 loop 的版本化定义，不是 prompt 片段。

建议结构：

| 字段 | 含义 |
|---|---|
| id / version | 唯一标识和版本。 |
| status | seed、candidate、active、deprecated、rolled_back。 |
| stages | 阶段列表。 |
| inputs / outputs | 每个阶段的输入输出。 |
| roles | 可选角色，如 Observer、Strategist、Critic、Augmentor。 |
| conditions | 适用条件和约束。 |
| exit_rules | 退出和停止条件。 |
| risk_level | 适用风险等级。 |
| required_events | 必须落盘的事件。 |
| promotion_rule | 提升规则。 |
| rollback_rule | 回滚规则。 |

## Loop 进化治理

Loop 进化必须走候选流程：

```text
RunTrace / FailureMode / ReflectionRecord / UserFeedback
  -> LoopFitness
  -> LoopEvolutionProposal
  -> Simulation / Replay / A-B Compare
  -> Review / Approval
  -> LoopPromotionPolicy
  -> Active LoopSpec
  -> Monitoring
  -> Rollback or Stabilize
```

最低要求：

- 每个 `LoopEvolutionProposal` 必须说明来源、问题、改动、预期收益、风险和回滚方式。
- 高影响 loop 变更必须人工确认或经过严格离线验证。
- loop 变更不能扩大工具权限。
- loop 变更不能绕过长期认知写入规则。
- 默认 `LoopSpec` 替换必须支持回滚。

## 与 AgentHarness 的关系

治理内核由 `AgentHarness` 强制执行：

- `Cognitive Plane` 可以订阅事件和提交候选。
- `ActionProposal`、`CommunicationProposal` 或 `LearningTask` 可以携带主动补强策略。
- `CognitiveLoop` 可以运行 `LoopSpec`。
- `LoopEvolutionProposal` 可以建议改进 loop。
- 真实执行、权限提升、长期写入、默认策略替换都必须回到 `AgentHarness` 和治理流程。

这条关系是 forme 不漂移、不失控、不把主动性做成越权能力的关键。

## M0/M1/M2/M3 治理切分

| 阶段 | 治理目标 |
|---|---|
| M0 | 固定内核、事件审计、policy、approval、候选区、UserModel/AgentSelfModel/PartnershipModel/CognitiveMap 和 LoopSpec 数据边界。 |
| M1 | UserModel、AgentSelfModel、PartnershipModel 的基础管理；主动补强策略的 proposal 级治理；seed LoopSpec；主动等级 L0-L3；候选认知更新。 |
| M2 | 主动学习、能力成长、CapabilityUpdateProposal、低风险自动学习、LoopFitness 记录。 |
| M3 | LoopEvolutionProposal、LoopPromotionPolicy、replay/simulation/A-B、默认 LoopSpec promotion/rollback。 |

## 当前结论

forme 的主动性和自进化能力必须建立在固定内核之上。固定内核定义不可绕过的边界，Cognitive Plane 提出候选，AgentHarness 执行治理，LoopSpec 通过证据和版本演化。

后续 requirements、architecture 和 PRD 不应只描述功能，还必须描述每个能力的治理边界：谁可以提出、谁可以执行、谁可以写入长期状态、谁可以提升为默认策略，以及如何撤销。
