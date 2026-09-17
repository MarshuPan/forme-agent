# 认知地图、信任放权与失败证据

本文档是 forme 的 V10 规划补强。它承接 `06-cognitive-evolution-kernel-assessment.md`、`09-open-source-positioning-and-kernel-boundaries.md` 和 `10-coordination-kernel-and-resource-orchestration.md`，用于把最近讨论中新增的三个关键关注点正式纳入三内核架构：

- `Cognitive Map`：模型负责通用认知能力，Agent 负责特定用户、特定项目、特定场景的认知地图。
- `Trust / Delegation`：主动式和协调式能力必须通过渐进放权建立信任，而不是直接追求自治上限。
- `Failure Evidence`：失败不是异常分支，而是认知进化、协调策略、能力评估和信任校准的一等证据。

本文不是需求文档、Rust 技术方案或 PRD。它的作用是修正和增强现有规划，避免后续架构只围绕“更主动、更会协调、更会学习”展开，却缺少场景认知、放权曲线和失败学习这三条硬骨架。

状态说明：本文中的 M0 数据边界表述已经被后续 `requirements/03-foundation-scope-decisions.md` 进一步强化：M0 需要生成低置信 `CognitiveMapUpdateProposal`、执行 `DelegationGrant / AutonomyEnvelope` enforcement，并形成 `FailureEvidence` 分类与 failure digest。本文继续作为三内核补强的规划依据。

## 核心判断

forme 不应新增第四个内核。当前三内核仍然成立：

```text
Cognitive Evolution Kernel
  -> makes the Agent stronger in a specific user/workspace context
Coordination Kernel
  -> uses resources, trust, failure evidence and cognitive maps to route work
Proactivity Kernel
  -> initiates useful action within an authorized autonomy envelope
AgentHarness
  -> governs execution, policy, approval, audit, rollback and promotion
```

本轮新增关注点的归属应是：

```text
Cognitive Map
  -> Cognitive Evolution Kernel 的核心认知对象
Trust / Delegation
  -> Coordination Kernel 的路线约束 + AgentHarness 的治理边界
Failure Evidence
  -> Event/Eval/Evolution/Coordination/Trust 的共同证据层
```

这意味着 forme 的内核主线需要从：

```text
Evolution -> Coordination -> Proactivity
```

进一步精炼为：

```text
Scenario cognition evolves.
Coordination uses that cognition under calibrated trust.
Proactivity acts only inside a governed delegation envelope.
Failures feed the next cognition, coordination and trust update.
```

## 外部理论映射

这些补强不是凭空加概念，而是来自几个成熟方向的共同指向。

| 方向 | 对 forme 的启发 |
|---|---|
| Cognitive map | Tolman 的 cognitive map 思路说明，智能行为不只是刺激-反应，而是内部地图对环境关系的组织。forme 的重点不是空间导航，而是为用户、项目、工具、风险、质量和任务模式建立场景认知地图。参考：https://psychclassics.yorku.ca/Tolman/Maps/maps.htm |
| Sensemaking | Pirolli/Card 的 sensemaking 过程把信息觅食、证据组织、schema、假设和表达产品放进循环。forme 的认知地图应来自持续 sensemaking，而不是单次总结。参考：https://andymatuschak.org/files/papers/Pirolli%2C%20Card%20-%202005%20-%20The%20sensemaking%20process%20and%20leverage%20points%20for%20analyst%20technology%20as.pdf |
| Trust in automation | Lee/See 指出自动化系统需要适当依赖，过信任会导致误用，不信任会导致弃用。forme 不能把 trusted mode 当成静态开关，必须做信任校准。参考：https://journals.sagepub.com/doi/10.1518/hfes.46.1.50_30392 |
| Levels of automation | Parasuraman/Sheridan/Wickens 把自动化拆成信息获取、信息分析、决策选择、行动实施四类，并且每类都有不同自动化等级。forme 的放权不能只按“能不能自动执行”划分，而要按任务功能拆开。参考：https://pubmed.ncbi.nlm.nih.gov/11760769/ |
| AI risk governance | NIST AI RMF 强调 Govern、Map、Measure、Manage。forme 的信任、风险、监测和治理应作为 lifecycle 能力，而不是 prompt 约束。参考：https://www.nist.gov/itl/ai-risk-management-framework |
| Blameless postmortem | Google SRE 的 postmortem 文化强调从事故贡献因素中学习，而不是简单归责。forme 的失败证据也应服务系统改进，而不是只标记“模型错了”。参考：https://sre.google/sre-book/postmortem-culture/ |

这些资料共同支持一个判断：随着模型逐渐吸收通用规划、推理和工具选择能力，Agent 产品的长期价值会更集中在特定场景认知、具身协调、边界治理、信任校准和失败学习上。

## 认知地图

### 定义

`CognitiveMap` 是 Agent 对某个用户、项目、领域、工具生态、工作流和风险环境形成的结构化认知地图。

它不是 memory 的同义词，也不是知识库。memory 记录“发生过什么、学到了什么”；认知地图描述“在这个场景里，哪些维度重要、它们如何关联、判断一件事时应该看什么、哪里容易误判”。

核心判断可以写成：

```text
Model owns general cognitive ability.
Agent owns scenario-specific cognitive maps.
```

对 forme 来说，模型会越来越擅长通用思考、规划、反思和工具调用，这部分不应成为产品护城河。forme 应沉淀的是模型无法从通用训练中获得的东西：

- 用户长期目标、偏好、能力短板和工作节奏。
- 项目的架构、约束、历史决策、质量标准和风险。
- 某类任务在当前用户场景中的判断维度。
- 用户反复否决、认可、修正过的隐性标准。
- 工具、数据源、skills、MCP、subagent 在当前环境里的真实表现。
- 哪些问题表面是执行问题，实际上是目标、选题、数据、流程或边界问题。

### 与普通 memory 的区别

| 对象 | 记录内容 | 作用 |
|---|---|---|
| EventMemory | 发生了什么。 | 事实追溯。 |
| EpisodicMemory | 一段任务经历。 | 场景回放。 |
| ReflectionMemory | 从经历提炼的经验。 | 行为改进候选。 |
| SemanticMemory | 稳定知识和事实。 | 检索和上下文补充。 |
| ProceduralMemory | 技能、流程和工具策略。 | 行动复用。 |
| CognitiveMap | 判断维度、关系、盲区、权重、场景规律。 | 形成更好的问题理解、资源协调和主动判断。 |

认知地图不应由单次任务直接生成。它应来自多次事件、失败、成功、用户反馈、外部学习和 replay/eval 的交叉证据。

### 核心对象

| 对象 | 责任 |
|---|---|
| `CognitiveMap` | 某个 scope 下的认知地图，如 user、project、domain、workflow、capability、risk。 |
| `MapNode` | 地图中的概念、对象、标准、约束、风险、资源或用户偏好。 |
| `MapEdge` | 节点之间的因果、依赖、冲突、支持、优先级、上下游关系。 |
| `JudgmentFrame` | 针对某类问题的判断框架，定义应观察的维度、问题、证据和取舍顺序。 |
| `BlindSpotModel` | Agent、模型、用户或某类 workflow 常见盲区和误判模式。 |
| `QualityModel` | 当前场景里什么叫质量好，哪些指标、标准、反例和红线重要。 |
| `CognitiveMapUpdateProposal` | 对认知地图的候选更新，必须带证据、冲突、适用范围和回滚方式。 |
| `MapConfidence` | 某个节点、边或判断框架的置信度、稳定性和证据等级。 |

### 更新链路

认知地图的更新链路建议如下：

```text
Event / Failure / UserFeedback / ExternalSource / Evaluation
  -> SensemakingRecord
  -> CandidateReflection
  -> CognitiveMapUpdateProposal
  -> ConflictCheck
  -> EvidenceScore
  -> Replay / Review / UserConfirmation
  -> Promote / Delay / Reject / Rollback
```

关键约束：

- 地图更新必须是候选先行。
- 事实和判断要分开。
- 用户反馈的权重高，但不能把一次反馈绝对化。
- 外部资料只能作为候选输入，不能直接覆盖本地场景认知。
- 认知地图可以有 scope，不应全局泛化。
- 低置信地图只能影响建议或追问，不能影响高风险行动。

### 如何影响协调内核

`Coordination Kernel` 应读取 `CognitiveMap`，但不能无条件信任它。

示例：

```text
GoalFrame
  -> SituationModel
  -> CognitiveMap lookup
  -> JudgmentFrame selection
  -> BlindSpot check
  -> ResourcePlan / DoneContract / ExecutionRoute
```

认知地图能帮助协调内核回答：

- 这类问题在当前用户场景里真正重要的维度是什么。
- 当前目标是不是表面目标，背后可能是什么。
- 哪些资源过去在这个项目上可靠。
- 哪些工具、资料源或 workflow 曾经失败。
- 用户通常希望 Agent 先问、先查、先做还是先沉默。
- 当前判断是否落入已知盲区。

## 信任与渐进放权

### 定义

`Trust / Delegation` 不是一个“trusted mode”。它是一套让用户逐步、可解释、可撤销地把某类任务、某类资源和某类行动交给 Agent 的机制。

信任的目标不是让用户无限相信 Agent，而是让用户形成适当依赖：

```text
trust should match demonstrated capability under a specific scope
```

过度信任会导致误用；信任不足会导致能力闲置。forme 的设计目标是信任校准，而不是最大化自动化等级。

### 信任不等于权限

| 概念 | 含义 | 归属 |
|---|---|---|
| `PermissionProfile` | 静态或配置化权限边界，定义什么允许、询问、禁止。 | Policy / Harness |
| `ApprovalPolicy` | 哪些动作需要用户审批。 | Policy / Approval |
| `TrustProfile` | Agent 在特定 scope、task、resource、action 上被证明可靠到什么程度。 | Coordination / Evolution |
| `DelegationPolicy` | 在某个信任状态下，Agent 能被委派到什么自动化等级。 | Coordination / Harness |
| `DelegationGrant` | 用户给出的具体放权授权，有 scope、期限、预算和撤销规则。 | Harness / Audit |

权限是硬边界。信任是经验判断。放权是用户授权。三者不能混用。

### 放权维度

参考 levels of automation，forme 的放权不应只问“是否自动执行”，而要拆成四个维度：

| 维度 | 低放权 | 中放权 | 高放权 |
|---|---|---|---|
| 信息获取 | 用户提供资料 | Agent 建议读取来源 | Agent 在授权范围内主动检索和整理 |
| 信息分析 | Agent 总结 | Agent 给出判断和不确定性 | Agent 持续监测并形成分析候选 |
| 决策选择 | 用户决策 | Agent 推荐方案 | Agent 在低风险范围内选择方案 |
| 行动实施 | 用户执行 | Agent 请求审批后执行 | Agent 在预授权 envelope 内执行 |

这能避免一个常见错误：因为用户允许 Agent 主动分析，就误以为它也允许 Agent 自动执行。

### 核心对象

| 对象 | 责任 |
|---|---|
| `TrustProfile` | 描述某个 scope 下 Agent、模型、tool、skill、MCP、subagent 或 workflow 的信任状态。 |
| `TrustEvidence` | 支撑信任变化的证据，包括成功、失败、验证、用户反馈、撤销、事故和长期表现。 |
| `TrustCalibrationRecord` | 记录信任是否过高、过低或匹配能力。 |
| `DelegationLevel` | 对某类任务或动作的放权等级。 |
| `DelegationPolicy` | 从 trust、risk、reversibility、impact、cost 推导可放权范围。 |
| `DelegationGrant` | 用户显式授权的放权范围、期限、预算、工具、动作类型和撤销条件。 |
| `AutonomyEnvelope` | 当前 run/job/proactive task 的自治边界，包含可做、不可做、必须问、必须验证、必须停止。 |
| `TrustUpdateProposal` | 对 TrustProfile 或 DelegationPolicy 的候选更新。 |

### 放权曲线

不再维护另一条 D0-D5 放权梯子。主动等级 L0-L5 只表达介入强度；真正的放权范围由 `AutonomyEnvelope` 表达，并与 L 等级正交。

`AutonomyEnvelope` 至少包含 scope、capability、action_type、risk_limit、approval_rule、budget、timebox 和 rollback。它由 permission、trust、risk、reversibility、用户显式授权和近期 FailureEvidence 共同决定。

高自治不能来自一次成功，也不能由模型自我判断开启。它必须由用户授权，并且受 scope、预算、时间、工具、动作类型和回滚能力限制。canonical 语义以 `../../architecture/canonical-contract.md` 为准。

### 与协调内核的关系

`Coordination Kernel` 在形成路线时必须做 trust-aware routing：

```text
GoalFrame
  -> Risk / Impact / Reversibility
  -> TrustProfile lookup
  -> DelegationPolicy
  -> AutonomyEnvelope
  -> ResourcePlan / ApprovalPlan
```

它需要判断：

- 当前是否只是建议，还是会产生副作用。
- 是否需要先形成 DoneContract。
- 是否需要先请求授权。
- 是否应降级为准备材料而不执行。
- 哪个工具、skill、subagent 在当前 scope 下更可信。
- 最近失败是否降低了某类自动化等级。

### 与 AgentHarness 的关系

`Coordination Kernel` 可以提出放权建议，但 `AgentHarness` 才能执行治理：

- enforce `AutonomyEnvelope`。
- 检查 `DelegationGrant` 是否存在、是否过期、是否匹配 action。
- 高风险动作强制 approval。
- 记录放权使用、成功、失败、撤销。
- 用户可以随时降级、暂停或撤回授权。
- trust update 只能进入候选，不自动提升默认权限。

## 失败作为一等证据

### 定义

`FailureEvidence` 是对失败、误判、偏航、无效打扰、工具错误、验证失败、审批拒绝、用户撤销、长任务失控等事件的结构化记录。

失败不是只用于报错。它应同时服务：

- AgentSelfModel：我在哪些任务上不可靠。
- CognitiveMap：我在哪些判断框架上误解了场景。
- CoordinationPolicy：我是否选错资源、路线或完成标准。
- CapabilityEvidenceStore：某个工具、skill、MCP、模型或 subagent 是否可靠。
- TrustProfile：某个 scope 下是否应降级放权。
- ProactiveEvaluation：主动建议是否造成噪声或风险。
- Regression/Eval：未来如何避免同类失败。

### 失败分类

建议初始 `FailureTaxonomy` 包含：

| 类型 | 表现 |
|---|---|
| `goal_framing_failure` | 目标理解错、成功标准不清、没有先协商完成。 |
| `context_failure` | 忽略关键文档、历史、用户偏好或当前状态。 |
| `cognitive_map_failure` | 使用了错误判断框架，抓错问题本质。 |
| `resource_selection_failure` | 选错工具、模型、skill、memory、subagent 或资料源。 |
| `execution_failure` | 工具、命令、文件、MCP、API、browser 等执行失败。 |
| `verification_failure` | 没有验证、验证不足、验证结果被误解。 |
| `trust_failure` | 放权过高、审批边界不清、自动化等级不匹配。 |
| `proactivity_failure` | 主动建议低价值、打扰、过早或越界。 |
| `learning_failure` | 把低质量来源或一次经验提升为稳定认知。 |
| `handoff_failure` | subagent、checkpoint、resume 或长任务交接不完整。 |
| `self_eval_trap` | 自己检查自己后过早宣称完成。 |
| `safety_policy_failure` | policy、approval、hook、plugin 或 sandbox 边界缺失或误判。 |

### 核心对象

| 对象 | 责任 |
|---|---|
| `FailureEvidence` | 失败事实记录，绑定 run、event、tool、resource、scope、impact 和用户反馈。 |
| `FailureIncident` | 多个 FailureEvidence 组成的一次任务级或系统级失败事件。 |
| `FailureTaxonomy` | 失败分类和判定规则。 |
| `FailureDigest` | 从失败事件提炼出的可读摘要、时间线、贡献因素和修正方向。 |
| `CorrectionAction` | 对能力、认知地图、协调策略、信任、测试或文档的修正动作。 |
| `RegressionGuard` | 防止同类失败复发的验证、检查、eval case 或 policy rule。 |
| `FailureToLearningProposal` | 将失败转化为候选认知、候选策略或候选测试的提案。 |

### 失败处理链路

```text
Failure observed
  -> FailureEvidence
  -> FailureClassification
  -> Impact / Reversibility / UserHarm assessment
  -> Immediate containment
  -> FailureDigest
  -> CorrectionAction
  -> Trust / Capability / CognitiveMap / Coordination candidate update
  -> RegressionGuard
  -> Promotion / Rollback / UserReview
```

关键点：

- 失败必须先被记录为事实，再被解释。
- 不能把失败简单归因给“模型不行”或“用户没说清”。
- 失败需要记录当时 Agent 有哪些信息、用了什么资源、为什么这样判断。
- 失败可能导致能力降级、信任降级、主动等级降级或协调策略回滚。
- 失败也可能证明用户的长期偏好、项目风险或工具边界，需要进入认知地图候选。

### 失败与成功的关系

成功同样需要记录，但失败的价值更高，因为它暴露边界。

```text
SuccessEvidence
  -> confirms current map/policy/capability under a scope
FailureEvidence
  -> reveals wrong map, wrong route, wrong trust or missing guard
```

因此，forme 的学习系统不能只统计成功率。它必须关注：

- 哪类失败反复出现。
- 哪类失败影响高。
- 哪类失败来自错误认知地图。
- 哪类失败来自过度信任。
- 哪类失败来自 under-tooling 或 over-tooling。
- 哪类失败需要用户确认新的边界。

## 三者如何组成闭环

三者不是并列功能，而是一个闭环：

```text
CognitiveMap
  -> tells the Agent what matters in this scenario
Trust / Delegation
  -> tells the Agent how much autonomy it has under this scope
Coordination Kernel
  -> chooses resources and route under map + trust + risk
AgentHarness
  -> executes, audits, verifies, approves and contains
FailureEvidence
  -> updates map, trust, capability and coordination candidates
```

用更工程化的链路表示：

```text
Goal / Opportunity
  -> SituationModel
  -> CognitiveMap + JudgmentFrame + BlindSpotModel
  -> ResourceInventory + TrustProfile + FailureEvidence
  -> ResourcePlan + DoneContract + AutonomyEnvelope
  -> AgentHarness execution
  -> Verification / UserFeedback / FailureEvidence
  -> CognitiveMapUpdateProposal
  -> TrustUpdateProposal
  -> CoordinationUpdateProposal
```

这个闭环让主动式不再只是“发现机会后做事”，而是：

- 先理解这个场景里什么重要。
- 再判断自己是否值得被信任。
- 再选择资源和路线。
- 再在 harness 约束下行动。
- 最后从成功与失败中校准认知和放权。

## 对三内核的补强

### Cognitive Evolution Kernel

新增重点：

- `CognitiveMap` 成为长期认知的核心对象之一。
- `JudgmentFrame` 让 Agent 不是只记事实，而是沉淀判断框架。
- `BlindSpotModel` 让 Agent 能知道自己在哪类问题上容易误判。
- `FailureEvidence` 成为认知更新的重要输入。
- `CognitiveMapUpdateProposal` 必须进入 candidate-before-promotion。

### Coordination Kernel

新增重点：

- 资源选择必须读取认知地图、信任状态和失败证据。
- `ResourcePlan` 应能解释为什么某些资源没有被使用。
- `DoneContract` 应绑定当前判断框架和失败风险。
- `ExecutionRoute` 应包含 `AutonomyEnvelope` 和 approval plan。
- `CoordinationOutcome` 应产出 failure-aware 和 trust-aware 的候选更新。

### Proactivity Kernel

新增重点：

- 主动建议必须经过认知地图检查，避免把低价值机会当成重要机会。
- 主动学习要围绕地图缺口、失败密集区和用户长期目标，而不是随机浏览。
- 主动行动必须受 delegation envelope 限制。
- 主动失败要进入 `ProactiveEvaluation`，影响后续打扰预算和主动等级。

### AgentHarness

新增重点：

- harness 必须是 delegation grant、autonomy envelope、approval 和 rollback 的最终执行边界。
- harness 必须捕获 failure evidence，而不是只抛错误。
- harness 必须能把失败、撤销、审批拒绝、验证失败等事件传给 eval/evolution/coordination。
- harness 不允许模型或内核自行提升 trust、扩大权限或稳定化认知地图。

## 固定与可进化边界

### 固定不变

- 模型不能自称拥有真实意识。
- 模型不能绕过 harness 执行真实动作。
- 权限永远高于信任。
- 放权必须可撤销、可过期、可审计。
- 高影响动作默认不能自治。
- 失败必须记录，不能被 final output 掩盖。
- 认知地图更新必须 candidate-before-promotion。
- 一次成功不能提升默认信任。
- 一次失败可以触发降级，但稳定降级也需要 evidence 和 review。

### 可进化

- 认知地图的节点、关系、权重和判断框架。
- 盲区模型和风险模型。
- 某类任务的资源选择策略。
- 某些工具、skill、MCP、模型和 subagent 的信任评分。
- 某些低风险任务的 delegation envelope。
- 主动建议的触发时机和打扰预算。
- failure taxonomy 的细分和回归验证方式。

## M0/M1/M2/M3 建议

### M0

M0 不做完整认知地图系统、自动放权和复杂事故学习，但必须固定数据边界：

- `CognitiveMap` / `JudgmentFrame` / `BlindSpotModel` 的对象边界。
- `TrustProfile` / `DelegationPolicy` / `DelegationGrant` / `AutonomyEnvelope` 的对象边界。
- `FailureEvidence` / `FailureTaxonomy` / `FailureIncident` 的对象边界。
- `DecisionTrace` 必须能引用认知地图、信任状态和失败证据。
- `RunResult` 和 event log 必须能表达失败类型、验证失败和审批拒绝。
- 默认不自动提升信任、不自动扩大权限、不自动稳定化地图更新。

### M1

M1 做最小体验：

- 用户可以查看某次 run 的 failure evidence。
- 用户可以标记“这次判断框架错了”“资源选错了”“打扰了”“可放权/不可放权”。
- `CoordinationReasoner` 可以在资源选择摘要中说明使用了哪些认知地图和信任依据。
- `TrustProfile` 可以记录用户显式授权、撤销和连续表现。
- `CognitiveMapUpdateProposal` 可由任务结束反思生成，但需要用户确认或足够证据。

### M2

M2 做跨任务学习：

- 多任务 failure clustering。
- 场景认知地图的图谱化管理。
- trust-aware resource routing。
- proactive learning based on map gaps and failure hotspots。
- low-risk delegation envelope for repeated verified tasks。
- regression guard / eval case generation。

### M3

M3 做受控进化：

- CognitiveMap promotion/replay/rollback。
- DelegationPolicy simulation and evaluation。
- Trust calibration dashboard。
- coordination strategy replay with failure evidence。
- proactive policy evolution based on trust and failure signals。
- model capability changes reflected in trust and coordination policies。

## forme 的差异化重点

- 将失败证据、认知地图和信任放权纳入长期内核，而不是只服务单次任务。
- 在 control plane 上方建立场景认知和渐进放权，不把多入口等同于主动式。
- 把 delegation 从功能能力提升为 trust-aware governance，不让 cron 伪装成主动性。
- 认知地图、失败证据和信任校准按 forme 自己体系设计。

## 对既有文档的修正关系

本文不推翻 `archive/planning/10`，而是补齐它的三个隐含前提：

- 协调内核要选资源，必须知道场景里什么重要，所以需要 `CognitiveMap`。
- 协调内核要决定行动路线，必须知道用户给了多少放权，所以需要 `Trust / Delegation`。
- 协调内核要进化，必须知道为什么失败，所以需要 `FailureEvidence`。

因此后续 `requirements` 和 `architecture` 不应只写：

```text
GoalFrame -> ResourcePlan -> ExecutionRoute
```

而应写成：

```text
GoalFrame
  -> SituationModel
  -> CognitiveMap / TrustProfile / FailureEvidence
  -> ResourcePlan / DoneContract / AutonomyEnvelope
  -> AgentHarness
  -> Outcome / FailureEvidence / UpdateProposal
```

## 当前结论

forme 的主动式 Agent 内核现在可以更准确地表达为：

> 一个以 `AgentHarness` 为最高治理边界，以 `Cognitive Evolution Kernel`、`Coordination Kernel`、`Proactive Kernel` 为三条内核主线，并通过场景认知地图、渐进放权和失败证据持续校准自身的个人/工作空间 Agent。

这让 forme 与常规 Agent 工具的差异更清晰：

- 不是比模型更聪明，而是沉淀模型没有的场景认知地图。
- 不是追求一次性全自动，而是设计用户敢逐步放权的信任曲线。
- 不是把失败当异常，而是把失败变成认知、协调、能力和信任进化的证据。
- 不是让主动式越强越好，而是让主动行为始终处在认知、信任、风险、权限和证据共同约束下。

后续进入正式架构方案时，`CognitiveMap`、`TrustProfile / DelegationPolicy / AutonomyEnvelope` 和 `FailureEvidence` 应作为 M0 数据边界进入 `docs/architecture/03-foundation-architecture.md`，但完整自动化能力应放到 M1-M3 渐进验证。
