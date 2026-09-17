# Agent Harness 与 Agent Loop 主导架构

本文档把 `Agent Harness` 与 `Agent Loop` 理念落到 forme 底座。当前仍是规划文档，不是 Rust 实现方案，也不是 PRD。

## 目的

forme 要做的是通用 Agent 工具底座，不是一个薄 LLM wrapper，也不是只在 CLI 里跑一段工具循环。这个底座未来会支持多入口、长任务、工具执行、权限、安全、上下文压缩、memory、skills、MCP、plugins、subagents、自动化和可观测运行。

Agent 工程的核心判断是：模型本身不是 Agent；模型必须被一个运行容器包住，才能拥有状态、上下文、工具、执行环境、权限、持久化、恢复、评估和事件流。这个运行容器就是 `AgentHarness`。模型与工具的多轮交替是 `AgentLoop`，但 loop 只是 harness 内部的核心机制，不是系统最高边界。

## 理论基线

本轮架构判断基于以下主线：

- reason/action/observation 交替是模型通过环境反馈迭代解决问题的基础 loop。
- 模型之外的运行要素都属于 harness：prompt、tools、skills、MCP、filesystem、sandbox、orchestration、hooks/middleware。
- 运行器管理 turns、tools、guardrails、handoffs、sessions，loop 持续到 final output、handoff、tool call 或限制/异常。
- workflow 与 agent 需要区分：workflow 是预定义代码路径，agent 是模型动态决定过程和工具使用。
- 生产级 agent orchestration 需要 durable execution、streaming、human-in-the-loop、persistence。
- 多层循环：loop 不止一个，至少包括 agent loop、verification loop、event-driven loop、trace-driven improvement loop。

因此，forme 的底座不应只问“模型如何调用工具”，而应问“什么运行时拥有并治理这个 Agent 的完整生命周期”。

## 核心判断

forme 应采用 Harness-first 架构。

`AgentHarness` 是 forme Agent 的运行时本体。它拥有 run/session/turn 生命周期，拥有 loop，拥有上下文、工具、权限、事件、状态、扩展、评估、恢复和子代理编排。

`AgentLoop` 是 harness 内部的执行状态机。它负责一次 run 或 turn 内模型、工具、handoff、observation、final output 的多轮交替，但不直接成为外部入口、不直接拥有 session、不绕过 policy、不直接持久化全局状态。

换句话说，模型不是 Agent，loop 也不是完整 Agent。Harness 才是把模型、工具、上下文、权限和执行环境组合成 Agent 的系统边界。

## 概念边界

| 概念 | 定义 | forme 中的位置 |
|---|---|---|
| Model | 推理和生成下一步意图的模型。 | 被 harness 调用，不拥有运行状态。 |
| Agent Loop | 模型输出、工具执行、工具结果回填、handoff、重复或结束的状态机。 | `LoopEngine`，由 harness 驱动。 |
| Agent Harness | Agent 运行容器，负责生命周期、上下文、工具、权限、状态、事件、扩展、评估、恢复、子代理。 | Runtime Core 的主对象。 |
| Workflow | 由代码预先定义路径的流程，可包含 LLM 和工具。 | 用于审批、验证、并行、确定性任务。 |
| Control Plane | 多入口、Gateway、API、session stream、插件生命周期。 | 调用和管理 harness，不复制 loop。 |
| Capability Layer | tools、skills、memory、MCP、plugins、cron 等能力集合。 | 以 harness-owned subsystem 形式接入。 |
| Cognitive Evolution Kernel | 长期观察、反思、学习、认知沉淀、认知地图、稳定性模型和进化建议。 | 只能提交候选更新，真实行动与长期写入必须回到 harness。 |
| Temporal Memory / UserModel | raw storage、memory substrate、时间化事件、用户属性候选、稳定用户属性、ImportedHistoricalEvidence 和多时间尺度用户理解。 | 支撑 Cognitive Evolution 与 Coordination，但不能绕过候选、验证和用户反馈直接塑造稳定画像。 |
| AgentSelfModel / PartnershipModel | Agent 自身能力、限制、可靠性、成长状态，以及用户与 Agent 的互补关系、协作阶段和放权边界。 | 只能通过任务、验证、失败、用户反馈和学习形成候选；不能由初始 profile 或单次事件直接稳定化。 |
| Coordination Kernel | 目标建模、资源选择、行动路线、done contract、autonomy envelope、decision trace 和协调策略候选。 | 在 Evolution 与 Proactivity 之间形成 ResourcePlan，但执行仍由 harness 治理。 |
| Proactivity Kernel | 授权观察、机会识别、主动介入判断和 proactive job 候选。 | 不能直接执行，必须把 Opportunity 或 ActionCandidate 交给协调和 harness。 |
| Communication Fabric | text/voice/image/video、software/hardware carrier、ChannelAdapter、CommunicationSession、DisclosurePolicy、TerminationPolicy、ExternalCommunicationGrant、Agent-to-Agent bounded session。 | 作为 Gateway 一等子域，统一外部沟通触手；不能直接拥有 loop 或绕过 harness。 |
| Trust / Delegation | trust profile、delegation grant、autonomy envelope 和渐进放权边界。 | permission 是硬边界，trust 只能影响路线和放权建议，最终由 harness enforce。 |
| Failure Evidence | 失败分类、影响、相关资源、验证结果、用户反馈和修正动作。 | 作为 eval、认知地图、协调策略、能力评分和信任校准的共同输入。 |
| LoopSpec | 认知 loop 的版本化定义。 | 由 `CognitiveLoop` 运行，由治理流程提升或回滚。 |

## 通用 Loop 状态机

forme 的 `LoopEngine` 不应只是 while 函数。它应是可观测、可中断、可恢复、可评估的状态机。

建议状态如下：

1. `RunAccepted`：请求被接收，生成 run id。
2. `SessionBound`：绑定 session、workspace、agent profile、policy profile。
3. `TurnStarted`：进入一次 turn。
4. `ContextBuildStarted` / `ContextBuildFinished`：构建模型输入、上下文预算和工具可见集。
5. `ModelCallStarted` / `ModelStreamDelta` / `ModelCallFinished`：模型产生文本、tool call、handoff 或 final output。
6. `OutputClassified`：判断模型输出是 final、tool call、handoff、clarification、invalid 还是 blocked。
7. `ToolCallProposed`：模型提出工具调用。
8. `ToolPolicyEvaluated`：执行层重新校验工具、权限、审批、沙箱、参数。
9. `ApprovalRequested` / `ApprovalResolved`：需要人类介入时暂停和恢复。
10. `ToolCallStarted` / `ToolOutputDelta` / `ToolCallFinished`：工具执行并产出 observation。
11. `ToolResultAppended`：工具结果写入 transcript/event log，并进入下一轮上下文。
12. `HandoffRequested` / `HandoffResolved`：切换到目标 agent 或 child run。
13. `VerificationStarted` / `VerificationFinished`：对中间产物或最终产物做确定性或模型评估。
14. `CompactionStarted` / `CompactionFinished`：上下文接近限制时压缩。
15. `TurnComplete` / `TurnAborted` / `TurnFailed`：turn 收尾。
16. `RunComplete` / `RunAborted` / `RunFailed` / `RunLimited` / `RunWaiting`：run 结束或等待外部输入。

Loop 的停止条件至少包括：

- 模型产生 final output。
- 达到 max turns。
- 达到 token、cost 或 wall-time 限制。
- 用户取消或外部中断。
- 权限审批拒绝且无法继续。
- provider 或 tool 错误达到重试上限。
- 上下文超限且压缩失败。
- handoff 无目标、循环移交或超过移交上限。
- verifier 判定失败且无法自动修复。
- 进入 human-in-the-loop 等待状态。

## Harness 的生产职责

forme 的 `AgentHarness` 应负责以下边界：

- 输入归一化：把 CLI、API、Gateway、自动化、subagent、内部调用统一成 `RunRequest`。
- 沟通归一化：通过 Gateway / Communication Fabric 接收 `CommunicationEvent`、`CommunicationSession`、`CommunicationProposal`、`ExternalCommunicationGrant` 和 bounded Agent-to-Agent session，而不是让 channel adapter 直接调用 loop。
- 生命周期：创建和推进 `Run`、`Session`、`Turn`，提供 wait、drain、cancel、resume、steer、follow-up。
- 会话绑定：绑定 workspace、agent profile、model、policy profile、toolset、memory scope。
- 上下文装配：加载 rules、session history、memory、skills、MCP/tool schema、插件上下文、压缩摘要。
- 工具治理：决定模型可见工具，执行前再次检查工具范围、权限、审批、沙箱和并发策略。
- 模型调用：选择 provider/model，管理 request options、stream、retry、budget、stop reason。
- 状态持久化：写入 event log、transcript、session state、run result、approval state、compaction summary。
- 事件协议：对外发出稳定事件，支持 UI、CLI、Gateway、日志、测试、评估和恢复。
- 扩展生命周期：触发 hooks，加载 plugin contribution，管理 skills/MCP 的延迟暴露。
- 子代理：创建 child run，隔离 独立上下文 和 toolset，回传 summary/result。
- 压缩与恢复：在合适边界执行 compaction，保留可恢复的 session lineage。
- 验证与评估：支持 deterministic verifier、LLM judge、tool eval、trace eval、环境结果验证。
- 外部事件：为 cron、webhook、平台消息、文件变更等 event-driven loop 预留入口。
- 认知与协调治理：接收 `ActionProposal`、`CommunicationProposal`、`LearningTask`、`UserAttributeCandidate`、`AgentSelfUpdateCandidate`、`CapabilityGap`、`PartnershipStateCandidate`、`CognitiveUpdateProposal`、`CognitiveMapUpdateProposal`、`LoopEvolutionProposal`、`ResourcePlan`、`DoneContract`、`AutonomyEnvelope`、`ExecutionRoute`、`TrustUpdateProposal`、`FailureToLearningProposal` 和 `CoordinationUpdateProposal`，并通过 Gateway、policy、approval、event store 和提升/回滚流程治理。
- 放权治理：检查 `DelegationGrant`、`TrustProfile` 和 `AutonomyEnvelope` 是否匹配当前动作，禁止 trust 突破 permission。
- 失败捕获：把 tool error、verification failure、approval denial、user revoke、proactive failure、handoff failure 和 policy failure 记录为结构化 `FailureEvidence`。

## Tool Call 的硬边界

工具调用在 forme 中必须被视为模型 proposal，而不是模型命令。标准生命周期为：

1. 模型提出 tool call。
2. 解析并校验 schema。
3. 检查工具是否在当前 visible toolset 中。
4. `PolicyEngine` 判定 allow / ask / deny。
5. 如果 ask，产生 `ApprovalRequest` 并暂停或等待。
6. 执行前再次检查参数、路径、网络、命令、sandbox、side effect。
7. 选择执行器：local、sandbox、remote、MCP、browser、app API。
8. 流式记录工具输出。
9. 按输出契约截断、摘要、artifact 化或结构化。
10. 写入 event log 和 transcript。
11. 把 observation 交回下一轮 loop。

这个边界必须在执行层实现，不能只写在 prompt 里。

## Context 与 Compaction

Context 不是字符串拼接，而是每轮 loop 的运行资源分配。forme 应把上下文分层：

1. System base：产品固定规则。
2. Agent profile：agent persona、模型、默认工具、长期配置。
3. Rules：项目、用户、组织确定性规则。
4. Skills metadata：短描述常驻，正文按需加载。
5. Memory summary：短索引常驻，topic 按需加载。
6. UserModel slice：只加载当前任务相关的用户属性、稳定性、置信度和时间尺度。
7. AgentSelfModel slice：只加载当前任务相关的 Agent 能力、限制、可靠性、能力缺口、学习议题和当前自我状态。
8. PartnershipModel slice：只加载当前用户与 Agent 在该 scope 下的协作阶段、放权边界、互补关系和纠偏机制。
9. Session history：当前 session 的近期消息和关键摘要。
10. Retrieved context：文件、搜索结果、MCP resources、历史检索；ImportedHistoricalEvidence 必须标记为低权重历史证据。
11. Volatile context：时间、环境、当前任务状态、approval status。
12. Tool schemas：只暴露当前 toolset，必要时 tool search / schema 延迟加载。

Compaction 属于 session/harness 生命周期，不是普通工具。它应该有事件、hook、摘要 lineage 和恢复边界。

## Verification 与改进闭环

forme 的底座应为验证闭环预留位置。M0 不需要实现完整 eval 平台，但必须让 loop 的产物可被检查。

M0 可以先支持：

- deterministic verifier：命令、测试、schema、文件存在、链接检查、diff 范围检查。
- final output validator：结构化输出、必填字段、禁止项。
- tool result validator：工具输出 schema、错误分类、敏感信息过滤。
- event trace export：为后续 eval 和 debug 留完整轨迹。
- failure evidence：记录失败类型、scope、影响、相关资源、验证结果、用户反馈、可回滚性和后续修正动作。

后续可继续扩展：

- LLM judge。
- rubric retry。
- trace cluster。
- failure digest 和 regression guard。
- tool description 改进。
- prompt/policy/harness config 的 hill-climbing loop。

## Harness-first 分层调整

当前总体分层保持三层一辅，但主语调整为 harness：

- Runtime Core：以 `AgentHarness` 为主对象，采用清晰的 Rust runtime 分层。
- Control Plane：调用、选择、管理 harness。
- Capability Layer：以 harness-owned subsystem 接入工具、memory、skills、MCP、automation。
- Coordination Kernel：在 harness-owned subsystem 中形成目标、资源、路线和 done contract，不能绕过 policy 和 execution re-check。
- Trust / Delegation：在 harness-owned governance 中执行 delegation grant、autonomy envelope 和 approval，不允许由模型或协调内核自我放权。
- Failure Evidence：由 harness/event/eval 捕获，再反馈给 cognition、coordination、capability 和 trust。
- Experience Contract：约束 harness 对用户暴露的权限、压缩、subagent、hook、result 行为。

建议模块方向：

| 模块 | 责任 |
|---|---|
| `harness` | `AgentHarness`、`HarnessConfig`、run/session/turn 生命周期、队列、状态机编排。 |
| `loop` | `LoopEngine`、model/tool/handoff 交替、turn 状态机、停止条件。 |
| `workflow` | 确定性流程、verification loop、approval flow、event-driven run flow。 |
| `protocol` | `RunRequest`、`Event`、`RunResult`、`ToolCall`、`ApprovalRequest`、session events。 |
| `context` | prompt/context assembly、rules、history、memory、skills、compression input。 |
| `memory` | raw storage、memory substrate、session history、memory summary、candidate/stable memory、ImportedHistoricalEvidence、UserAttributeCandidate、UserModelAttribute、AgentSelfAttribute、PartnershipState、CognitiveMap、时间尺度和 provenance；它是这些长期对象的事实源。 |
| `tools` | registry、toolset、tool schema、tool result contract、execution re-check。 |
| `policy` | 权限模式、approval、delegation grant、autonomy envelope、sandbox、path/network/command/tool policy。 |
| `store` | event log、transcript、session state、lineage、FTS、resume state。 |
| `extensions` | hooks、plugins、MCP、skills contribution 的统一入口。 |
| `agents` | agent profiles、handoff、child run、subagent orchestration。 |
| `cognition` | Cognitive Evolution Kernel、CognitiveLoop、LoopSpec、AgentSelfUpdateCandidate、PartnershipStateCandidate、CognitiveMapUpdateProposal、JudgmentFrame、BlindSpotModel、Reflection、LearningAgenda、proposal 级 `augmentation_strategy`；它通过 ref 读取 UserModel、AgentSelfModel、PartnershipModel 和 CognitiveMap，不重复持有事实源。 |
| `coordination` | GoalFrame、ResourceInventory、UserModelAttributeRef、AgentSelfAttributeRef、CapabilityEvidenceRef、PartnershipStateRef、TemporalMemoryRef、ImportedHistoricalEvidenceRef、CognitiveMapRef、TrustProfileRef、FailureEvidenceRef、ResourcePlan、DoneContract、AutonomyEnvelope、ExecutionRoute、DecisionTrace、CoordinationPolicy。 |
| `eval` | verifier、trace export、run scoring、tool evaluation、FailureEvidence、FailureTaxonomy、failure digest。 |
| `communication` | CommunicationEvent、CommunicationSession、ParticipantProfile、DisclosurePolicy、TerminationPolicy、ExternalCommunicationGrant、LocalDeviceChannelAdapter、Agent-to-Agent bounded session。 |
| `gateway` / `cli` | 外部入口，只提交 run 和消费 event，不拥有 loop。 |

## M0 融入方式

M0 不需要做完整产品平台，但必须形成完整核心体系的第一条可运行闭环。它不能只把 harness/loop 边界做对，还必须把 policy、persistence、capability、coordination、proactivity、cognition、failure evidence、delegation 和 Gateway-compatible approval 接到同一条受治理链路上。

M0 默认应包含：

- `AgentHarness` 第一版：接收 `RunRequest`，绑定 session，启动 turn，发事件，返回 `RunResult`。
- `LoopEngine` 第一版：模型调用、工具调用、工具结果回填、继续/结束。
- `Run/Session/Turn/Event` 协议：所有 UI/CLI/Gateway 都只看协议，不读内部对象。
- CLI + Local Gateway/App Server 第一版：两者提交同一种 run，消费同一种 event stream，共享 approval protocol。
- SQLite/FTS + `EventStore` 第一版：append-only event log、session transcript、resume state 和可检索摘要。
- `ContextEngine` 第一版：system prompt、history、rules、skills metadata、memory summary、tool schema。
- `TemporalMemory / UserModel` 第一版边界：raw storage、candidate memory、stable memory、UserAttributeCandidate、UserModelAttribute、ImportedHistoricalEvidence、过程证据优先级、稳定性和置信度元数据。
- `AgentSelfModel / PartnershipModel` 第一版边界：AgentProfile、AgentIdentityBaseline、AgentSelfAttribute、AgentSelfUpdateCandidate、CapabilityEvidence、CapabilityGap、LearningAgendaItem、PartnershipState、AgentSelfAttributeRef、PartnershipStateRef。
- `ToolRuntime` 第一版：tool registry、toolset、执行层 re-check、结构化 tool result。
- `PolicyEngine` 第一版：allow/ask/deny、approval request、tool/path/command 基础策略。
- `ResumeState` 第一版：approval 后恢复、tool interruption 后恢复。
- `Verifier` 第一版：能挂确定性检查，能把验证结果写事件。
- `CompactionManager` 第一版接口和事件：自动压缩策略可后续增强，但压缩必须属于 session/harness 生命周期。
- `Hook` 基础点：BeforeExecution、AfterExecution、SessionEnded、BeforeCompaction/AfterCompaction 先定义契约。
- MCP stdio、SkillRegistry 和 local plugin manifest 第一版：进入 CapabilityRegistry，接受 trust、scope、policy 和 event 治理。
- Communication Fabric 第一版边界：text modality、CLI/Local Gateway 标准化事件、CommunicationSession、DisclosurePolicy、TerminationPolicy、ExternalCommunicationGrant、CommunicationProposal 和 bounded A2A session 的数据模型。
- `Subagent` 简化模型：child run、独立上下文、限制 toolset、summary 回传。
- `Cognition` 第一版候选链路：定义认知事件、UserModel、AgentSelfModel、PartnershipModel、`CognitiveMap`、`JudgmentFrame`、`BlindSpotModel`、proposal 级 `augmentation_strategy` 和 `LoopSpec`，并允许生成低置信 `CognitiveMapUpdateProposal`、`AgentSelfUpdateCandidate` 和 `PartnershipStateCandidate`，但不自动提升稳定认知、稳定自我属性、伙伴关系状态或自动自治。
- `CoordinationReasoner` 第一版：基于目标框架、资源计划、完成合同、决策轨迹、用户属性引用、时间化记忆引用、信任引用和失败证据引用生成 `ResourcePlan`、`DoneContract`、`AutonomyEnvelope` 和 `DecisionTrace`，但不自动进化默认协调策略。
- `Proactive` 第一版：授权 observation -> opportunity -> value gate -> proposal -> feedback -> evidence 的最小闭环。
- `Delegation` 第一版 enforcement：`DelegationGrant` 和 `AutonomyEnvelope` 必须被 harness 检查，不自动提升 trust 或扩大权限。
- `FailureEvidence` 第一版：在事件中表达失败分类、影响、相关资源、验证结果、用户反馈和 failure digest。

## 对需求和 PRD 的影响

后续需求文档应优先描述 harness 能力，而不是直接列模型和工具：

- 用户能从哪些入口提交 run。
- run 如何绑定 session、workspace、agent profile。
- 用户如何观察运行、取消、恢复、等待。
- Agent 可以看到哪些上下文，哪些必须延迟加载。
- 工具如何被允许、审批、拒绝和审计。
- 压缩、memory、skills、MCP、plugins 如何影响一个 run。
- UserModel、UserAttributeCandidate、ImportedHistoricalEvidence 和时间化记忆如何影响 context、coordination、proactive 和 cognition。
- AgentSelfModel、AgentSelfUpdateCandidate、CapabilityEvidence、CapabilityGap 和 PartnershipModel 如何影响 context、coordination、proactive、trust 和 cognition。
- verifier 如何判断中间产物和最终产物。
- subagent 如何被创建、限制和汇总。
- 外部事件如何触发 run。
- 沟通事件如何通过 Gateway 标准化，如何区分文字/语音/图片/视频、软件承载和硬件承载，如何形成 CommunicationSession。
- Agent 如何在授权范围内对外沟通、代表用户表达、创建受限外部链接或与其他 Agent 进行 bounded session。
- Cognitive Evolution Kernel 如何提交认知更新候选和 loop 改进候选。
- UserModel 如何形成全维度、可演化、有置信度和时间尺度的用户理解，而不是静态 profile。
- AgentSelfModel 如何形成全维度、可演化、有置信度和时间尺度的自我理解，而不是静态 Agent profile 或 prompt persona。
- PartnershipModel 如何表达用户与 Agent 的互补关系、协作阶段、放权边界和纠偏机制。
- CognitiveMap 如何表达场景判断框架、质量模型和盲区模型。
- Coordination Kernel 如何提交资源计划、完成合同、自治边界、执行路线和协调策略候选。
- Trust / Delegation 如何区分 permission、approval、trust、delegation grant 和 autonomy envelope。
- FailureEvidence 如何进入 eval、认知地图、协调策略、能力评分和信任校准。
- Proactivity Kernel 如何提交行动候选、主动任务和用户介入请求。
- 固定内核如何限制主动性、长期写入、资源协调策略、放权提升和 loop promotion。

后续技术方案和实施型 PRD 应围绕 `AgentHarness -> Policy/Delegation -> Coordination -> LoopEngine -> Subsystems -> Evidence Protocol` 展开。这样实现时不会把工具、memory、permissions、plugins 做成散落功能，也不会让 CLI、Gateway、Proactivity Kernel 或 Coordination Kernel 偷偷拥有一套自己的执行 loop。

## 不做的事

- 不把 `AgentLoop` 作为外部入口的主对象。
- 不让 CLI/Gateway 直接调用模型循环。
- 不把权限只写进 prompt，执行层必须重新校验。
- 不把 compaction 当成普通工具，压缩属于 session/harness 生命周期。
- 不把 subagent 当成普通长文本工具结果，必须有 child run 和 summary contract。
- 不把 verifier/eval 当成事后人工流程，必须预留为 loop 外围闭环。
- 不让 Cognitive Evolution Kernel、Proactivity Kernel、主动补强策略或 LoopSpec 绕过 harness 直接执行工具或写长期状态。
- 不让 ImportedHistoricalEvidence、一次性历史导入或单次对话直接塑造稳定用户画像、稳定认知、trust 提升或高影响主动行为。
- 不让 AgentProfile、单次成功、单次失败或模型自我评价直接塑造稳定 AgentSelfModel、PartnershipModel、trust 提升或 delegation grant。
- 不让 Coordination Kernel 绕过 harness 直接执行工具、扩大权限、写长期状态或替换默认策略。
- 不让 Channel Adapter、LocalDeviceAdapter、外部沟通链接或 Agent-to-Agent 会话绕过 Gateway / Communication Fabric、policy、approval、termination policy 和 audit。
- 不让 trust 覆盖 permission，不让 delegation grant 无 scope、无期限、无审计或不可撤销。
- 不把失败只当普通 error log，关键失败必须进入 FailureEvidence。
- 不复制任何第三方项目的源码结构或单体组织。

## 当前阶段结论

forme 的底座规划应从 `AgentHarness` 开始。Harness 是主导架构，Loop 是内部引擎。这个判断来自通用 Agent 工程理论和 forme 自身的治理需求。

在这个基础上，后续写 `foundation architecture` 时，应先定义 harness 的 API、事件协议、状态机、子系统边界、恢复边界和验证边界，再进入具体 Rust crate、数据结构和实现步骤。
