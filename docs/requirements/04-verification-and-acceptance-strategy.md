# 验证与验收策略

本文档是 forme 的第四份需求文档。它承接 `03-foundation-scope-decisions.md`，回答一个问题：如何证明 M0 不是概念堆叠，而是一条完整、受治理、可观察、可恢复、可验证的核心闭环。

本文仍然不是测试代码、Rust 技术方案或实施 PRD。它定义后续架构和 PRD 必须满足的验收思路、场景矩阵和证据要求。

## 验证原则

### 1. 用事件和状态证明，不用最终回答证明

Agent 的最终回答不能单独证明系统正确。M0 的验收必须能从 run/session/event/trace 中证明：

- 入口如何提交 run。
- harness 如何绑定 session、policy、model、toolset。
- coordination 如何形成资源路线。
- tool call 如何被重新校验。
- approval 如何等待、拒绝、恢复或降级。
- execution backend 如何产生结构化结果。
- verifier 如何给出结果。
- failure evidence 如何写入。
- candidate update 如何保持候选状态。

### 2. 每个核心能力都必须有正向和反向场景

只证明 happy path 不够。M0 至少要证明：

- 允许路径能执行。
- 拒绝路径不能执行。
- 等待路径能恢复。
- 失败路径能收尾。
- 越权路径被拒绝。
- 候选更新不会直接污染稳定层。
- trust 不会覆盖 permission。

### 3. 验证对象必须覆盖三类结果

M0 验收要同时覆盖：

- `Runtime correctness`：run 能否正确推进、暂停、恢复、结束。
- `Governance correctness`：权限、审批、放权、审计是否生效。
- `Learning correctness`：记忆、认知地图、失败证据、协调反馈是否进入候选和证据链，而不是直接改稳定内核。

### 4. 验收标准必须能落到 PRD

本文先定义验收策略。后续 PRD 需要把每条验收策略转成具体测试任务、fixture、命令、事件断言和失败断言。

## M0 验收矩阵

| 领域 | M0 必须证明什么 | 典型反向验证 |
|---|---|---|
| AgentHarness 生命周期 | 同一 run 能从 CLI 或 Local Gateway/App Server 提交，绑定 session、policy、model、toolset，并产出一致事件。 | CLI 不能绕过 harness 直接调用 loop；Gateway 不能绕过 policy。 |
| AgentLoop | 模型、tool proposal、tool result、继续或结束能形成可恢复状态机。 | max turns、cancel、tool error、approval deny 后不能丢状态。 |
| 持久化 | SQLite/FTS 与 append-only event log 能记录 run、turn、tool、approval、verification、failure、candidate update。 | 重放或查询时不能只看到最终文本，看不到关键决策。 |
| Tool re-check | 模型可见工具不等于执行许可；执行前必须重新检查 toolset、policy、scope、参数和风险。 | 模型构造不可见工具名或越权参数时必须被拒绝。 |
| Approval | allow / ask / deny、wait / resume / reject 都有结构化事件。 | 拒绝审批后不能继续执行原高风险动作。 |
| Delegation | DelegationGrant 和 AutonomyEnvelope 在运行时 enforcement，受限预授权有 scope、期限、预算和撤销。 | trust 高不能越过 deny；subagent 不能使用未授权工具。 |
| MCP | stdio MCP tools/resources 能 discovery、allowlist、调用、超时、禁用并记录事件。 | 未授权 MCP server/tool/resource 不可见或不可执行。 |
| Skills | skills metadata 可见，正文按需加载，scope、version、trust boundary 可记录。 | 未选中的 skill 正文不能被默认塞入上下文。 |
| Plugins | local plugin manifest 可声明贡献，enable/disable/trust/policy 生效。 | 禁用插件贡献的 tool/skill/MCP/hook 不能进入当前 run。 |
| CapabilityRegistry | tools、skills、MCP、plugins、hooks 能被统一索引，并能生成当前 run 的 toolset 和能力证据。 | 能力来源不明或风险未知时不能默默暴露给模型。 |
| Communication Fabric | text/voice/image/video、software/hardware carrier、ChannelAdapter、CommunicationSession、DisclosurePolicy、TerminationPolicy、ExternalCommunicationGrant 能进入统一 Gateway 边界。 | 沟通触手不能直接绕过 Gateway 调用 harness；外部会话不能无期限、无限预算或默认披露。 |
| Context / Memory | 上下文来源、session history、summary、candidate/stable memory 有边界。 | candidate memory 不能自动变成 stable memory。 |
| Temporal Memory / UserModel | UserAttributeCandidate、UserModelAttribute、ImportedHistoricalEvidence、confidence、stability、scope、时间戳和证据链可记录、查询和解释。 | 单次对话、一次失败或历史导入资料不能直接生成稳定用户画像、trust 提升或高影响主动行为。 |
| CoordinationReasoner | 能把 goal、situation、resources、cognitive map、trust、failure evidence 转成 ResourcePlan、DoneContract、DecisionTrace。 | 没有完成标准或资源不可信时，不能假装已经具备行动路线。 |
| Proactive minimum loop | 授权 observation 能进入 opportunity、value gate、proposal、feedback 和 evidence。 | 主动建议被拒绝后不能继续执行或反复打扰。 |
| CognitiveMap candidate | 能生成低置信 CognitiveMapUpdateProposal，并保持候选状态。 | 一次成功或失败不能直接改稳定认知地图。 |
| FailureEvidence | 工具失败、验证失败、审批拒绝、主动式失败、资源选错等能分类并生成 digest。 | final answer 成功不能掩盖中间失败。 |
| Verification | deterministic verifier、tool result validator、final output validator 的结果进入 trace。 | 验证失败不能被标记成成功完成。 |
| ConfigDoctor | provider、credential、model capability、MCP server、plugin manifest、filesystem/shell permission 能被检查和解释。 | 配置错误不能只表现为模型运行时猜测。 |
| 原创与合规 | 本地参考材料不进入 build/import/include 路径；文档和实现不复制第三方表达。 | 不能出现第三方源码、来源不明材料、大段文案或一比一目录结构进入主工程。 |

## 必备验收场景

后续 PRD 至少要覆盖以下场景。这里描述的是验收意图，不是最终测试实现。

### S1. CLI 与 Gateway 同协议提交

目标：证明 CLI 和 Local Gateway/App Server 都只是入口。

验收证据：

- 两个入口产生同一种 `RunRequest`。
- 两个入口都能订阅同一种 event stream。
- 两个入口都能看到 approval waiting、tool result、verification、run result。
- loop 不被 CLI 或 Gateway 私有化。

### S2. 高风险工具调用审批拒绝

目标：证明工具调用是 proposal，不是模型命令。

验收证据：

- 模型提出高风险 shell/file 动作。
- PolicyEngine 返回 ask 或 deny。
- ApprovalRequest 记录理由、风险、scope、可回滚性。
- 用户拒绝后动作不执行。
- run 正确暂停、降级或结束。
- FailureEvidence 或 DecisionTrace 记录拒绝原因和后续路线。

### S3. MCP tool/resource 一等接入

目标：证明 MCP 不是普通工具列表拼接。

验收证据：

- MCP server discovery 产生 tools/resources 元数据。
- allowlist 决定可见范围。
- tool/resource 调用产生独立 MCP event。
- timeout、server error、schema mismatch 有分类。
- 禁用 server 后当前 run 不再暴露其能力。

### S4. Skill 按需加载

目标：证明 skill 是过程记忆，且上下文不被无差别污染。

验收证据：

- Context 中默认只有 skill metadata。
- 只有被选择或被 CoordinationReasoner 引用的 skill 才加载正文。
- 加载行为有 event。
- skill scope/version/trust boundary 可追溯。

### S5. Plugin 贡献边界

目标：证明插件可以扩展能力，但不能绕过治理。

验收证据：

- local plugin manifest 声明 tools、skills、MCP servers、hooks 或 config contribution。
- disabled 插件贡献不可见。
- enabled 插件贡献必须进入 CapabilityRegistry，并接受 policy、approval、event。
- 插件 hook 不能绕过 tool execution re-check。

### S6. CoordinationReasoner 生成行动路线

目标：证明协调性不是 prompt 中的一句话。

验收证据：

- 用户目标被转换为 GoalFrame。
- 当前上下文、缺失信息、可用资源形成 SituationModel 和 ResourceInventory。
- Reasoner 引用 CognitiveMapRef、TrustProfileRef、FailureEvidenceRef。
- 输出 ResourcePlan、DoneContract、AutonomyEnvelope、DecisionTrace。
- 用户或测试可以解释为什么用某些资源，不用另一些资源。

### S7. 主动式最小闭环

目标：证明主动式从 M0 起进入内核，但仍受治理。

验收证据：

- 授权 observation 被记录。
- OpportunityDetector 识别一个低风险机会。
- ValueGate 判断是否值得打扰。
- 系统生成 ActionProposal 或 LearningTask。
- 用户采纳、拒绝或延后。
- feedback 影响后续 proactive evidence。
- 被拒绝的 proposal 不自动执行。

### S8. UserModel 候选更新与历史证据降权

目标：证明用户理解是时间化、过程化、候选化的，不是一次性 profile 总结。

验收证据：

- 用户明确表达一个偏好或工作方式，系统生成 `UserAttributeCandidate`。
- candidate 记录 source evidence、confidence、first_observed_at、last_updated_at、stability、scope 和 user feedback。
- 多次过程证据或用户确认可以提高置信度。
- 一份历史导入资料只能标记为 `ImportedHistoricalEvidence`，并作为低权重候选线索。
- ImportedHistoricalEvidence 不能直接生成稳定 UserModelAttribute、trust 提升、delegation grant 或高影响主动行为。
- 用户可以拒绝、降级或确认关键用户属性候选。

### S9. CognitiveMap 候选更新

目标：证明长期学习不是流水账 memory。

验收证据：

- 任务结束后根据 trace、用户反馈和 failure evidence 生成低置信 CognitiveMapUpdateProposal。
- proposal 表达判断框架、质量标准、盲区或资源关系。
- proposal 进入 candidate 层。
- stable CognitiveMap 不自动变化。
- 用户可以拒绝或降级候选。

### S10. FailureEvidence 与 failure digest

目标：证明失败能进入学习和治理。

验收证据：

- 工具失败、验证失败、资源选错或主动式误报被分类。
- failure digest 记录影响、scope、相关资源、验证结果、用户反馈和建议修正。
- 失败与 DecisionTrace、CapabilityEvidence、TrustProfileRef 或 CognitiveMapUpdateProposal 关联。
- 最终回答不能覆盖或隐藏失败。

### S11. Subagent 受限委派

目标：证明 delegation 是 harness 管理的 child run。

验收证据：

- parent run 创建 child run。
- child run 独立上下文。
- child run toolset、model、permission、budget 被限制。
- child run 结果按 summary/result contract 返回。
- child run 越权调用被拒绝。
- 成功和失败进入 parent trace。

### S12. CommunicationEvent 标准化入口

目标：证明沟通触手先进入 Gateway / Communication Fabric，而不是直接进入 harness。

验收证据：

- CLI 或 Local Gateway 输入被转成标准 `CommunicationEvent`。
- 事件记录 modality、carrier、channel adapter、participant、scope 和 session routing。
- Gateway 将事件转成 `RunRequest` 或 `CommunicationSession`。
- harness 只接收标准化后的请求和事件。
- 非标准 adapter 不能直接调用 loop 或写 memory。

### S13. 外部沟通会话范围与终止

目标：在 M0 不接入真实公网链接和真实外部平台的前提下，证明对外沟通的数据模型和策略 enforcement 点已经具备目的、范围、期限和终止条件。

验收证据：

- 用模拟外部参与者事件创建 `CommunicationSession`，不要求真实公网链接。
- `ExternalCommunicationGrant` 声明 purpose、participant、disclosure、TTL、budget 和 transcript policy。
- 模拟外部参与者只能在 `CommunicationSession` 范围内对话。
- 超过 TTL、轮次、预算或目的偏离时，session 被策略终止或要求用户确认。
- transcript 进入 event/evidence/candidate，不自动写入 stable memory。

### S14. DisclosurePolicy 阻断敏感披露

目标：用模拟外部请求证明 Agent 足够了解用户，不等于可以对外说用户的一切。

验收证据：

- 模拟外部参与者请求超出 disclosure policy 的私人信息、客户信息、内部计划或未确认判断。
- Gateway/Harness 生成拒答、模糊化回答、转用户审批或结束会话。
- 被阻断的问题、策略来源和处理结果进入 audit event。
- Agent 不把低置信候选认知对外表达为确定事实。

### S15. 思想代理与代表表达身份边界

目标：证明 Agent 可以在授权范围内代表用户表达，但不能伪装成用户本人。

验收证据：

- `CommunicationPurpose` 表达代表讲述、课程、介绍或对接目标。
- `DisclosurePolicy` 的 representation 字段指定可代表表达、需标注不确定、必须回到用户确认的内容。
- 输出中保持 Agent 代表身份边界。
- 高影响表达或未确认观点触发 approval。
- 表达结果、用户反馈和误差进入 FailureEvidence 或 candidate update。

### S16. Local Device 授权边界

目标：在 M0 不接入真实麦克风、喇叭、摄像头和屏幕采集的前提下，证明本地设备触手的数据模型和授权 enforcement 点不能默认启用。

验收证据：

- 用模拟设备事件验证 `LocalDeviceChannelAdapter` 需要显式 `DeviceGrant`、ObservationScope 和 RetentionPolicy。
- 未授权模拟设备输入不会进入 event stream。
- 授权事件记录 device、modality、purpose、scope、expires_at。
- 撤销授权后设备事件被拒绝或忽略。

### S17. Agent-to-Agent bounded session

目标：在 M0 不实现真实 A2A 协议的前提下，证明 Agent-to-Agent bounded session 的数据模型和策略 enforcement 点是短会话、强目的、强终止，不会无限循环。

验收证据：

- 用模拟 A2A 事件创建 bounded session，不要求真实协议互通。
- A2A session 声明任务目的、双方身份、允许披露范围、最大轮次、TTL、token budget 和终止条件。
- 达成目的、预算耗尽、超时、风险升高或策略拒绝时会话结束。
- 双方 transcript digest 只进入 candidate/evidence。
- 任一方请求越界信息时，另一方拒绝、转审批或终止。

### S18. CompetenceGate 主动降级

目标：证明主动式不是“只要有价值就发出”，还必须满足能力、认知、信任和失败证据门槛。

验收证据：

- 构造一个 ValueGate 通过、但 MapConfidence 或 AgentSelfModel 置信度不足的主动机会。
- `CompetenceGate` 读取 MapConfidence、AgentSelfModel、CapabilityEvidence、TrustProfile 和 FailureEvidence。
- 原本可执行或对外表达的 proposal 被降级为 L2 Prepare、L1 Suggest 或 L0 Observe。
- 降级原因、相关失败证据和所需补充信息进入 event trace。
- 降级后的 proposal 不能绕过 policy、approval 或 AutonomyEnvelope。

### S19. 并发与一致性不变量

目标：证明前台 session、后台主动任务、reflection、subagent 和认知候选不会互相覆盖状态。

验收证据：

- 同一 session 的两个 run 进入 per-session queue，按顺序推进。
- event log 只有一个写入事实源，projection 或索引从 event 派生。
- 后台 ProactiveJob / IdleWork / reflection 读取快照，只提交 candidate/proposal，不直接修改前台 session 或 stable memory。
- subagent child run 不能直接写父 session 稳定状态，只能按 summary/result contract 回传。
- policy deny、approval revoke、budget exhausted 能打断后续执行并进入 event。

### S20. 撤销与派生失效

目标：证明稳定认知不是不可逆结论，撤销后派生对象会进入再评估，而不是继续被静默使用。

验收证据：

- 一个 stable UserModelAttribute 或 AgentSelfAttribute 记录 evidence -> object 血缘。
- CognitiveMap、TrustProfile、CoordinationPolicy、LoopSpec 或 PartnershipState 记录 object -> derived 血缘。
- 用户撤销或纠正稳定属性时追加 RetractionEvent / RevocationEvent。
- 所有关联派生对象生成 invalidation candidate、downgrade candidate 或 re-evaluation task。
- 系统不自动删除历史事实，但停止把被撤销属性当作有效稳定认知使用。

### S21. Schema Version 与 Replay 边界

目标：证明 append-only event log 能支撑未来 replay/eval/promotion，而不是被投影状态锁死。

验收证据：

- event、candidate、stable object 都带 schema_version。
- 旧 schema 事件可以通过读时 upcast 或 projection migration 被当前 reader 解释。
- replay trace 记录 schema version、policy version、LoopSpec version、model profile 和 tool schema snapshot。
- breaking schema change 必须有 migration note，不能静默改变历史事件语义。
- replay 结果能指出与当前 projection 的差异，而不是覆盖历史 event。

### S22. 开源原创 CI / Doctor 边界

目标：证明开源原创性不是发布前人工检查，而是从 M0 开始就有可执行边界。

验收证据：

- `research/upstreams/**` 本地参考材料或第三方源码不进入 build/import/include 路径。
- doctor 或 CI 检查能发现主工程对本地参考快照、来源不明材料、GPL/未知许可证材料的直接引用。
- 文档和工程命名不复制第三方目录结构、prompt、错误信息、测试 fixture 或大段产品表达。
- 需要公开协议兼容、依赖或复用时，必须记录 source、license、copy_risk、implementation_rule 和 notice action。
- 检查失败时不能进入 release-ready 状态。

## 阶段验收门

### 进入正式架构前

在编写 `architecture/03-foundation-architecture.md` 前，需要完成：

- `requirements/03` 的范围决策已被接受。
- 本文的验收矩阵没有明显缺项。
- M0 的能力不是按“后面再补”理解，而是按“第一条核心闭环”理解。

### 进入 PRD 前

在编写 `prd/01-foundation-implementation-prd.md` 前，需要完成：

- 架构文档能把本文每个验收领域映射到模块、事件、存储和测试位置。
- 每个 M0 决策都有对应的接口边界和失败边界。
- 没有任何入口、插件、MCP、skill、subagent、proactive job 可以绕过 harness。
- 没有任何 channel adapter、local device adapter、external communication link 或 Agent-to-Agent session 可以绕过 Gateway / Communication Fabric、policy、approval 和 audit。

### 进入实现前

在开始 Rust 实现前，PRD 需要把本文转成：

- 具体测试场景。
- fixtures。
- 命令或 API 调用。
- 预期 event 序列。
- 预期 store 状态。
- 预期错误分类。
- 预期用户可见结果。

### 开源发布前

开源发布前还需要额外验证：

- release tree 不包含第三方源码快照、来源不明材料或不应发布的本地参考目录。
- 主工程不 import/include/build `research/upstreams/**`。
- 文档没有复制第三方段落、prompt、错误信息或命令输出。
- 依赖、license、NOTICE、安全扫描完成。

## 对后续文档的约束

后续架构和 PRD 不能只写“支持某能力”，必须写清楚“如何验收某能力”。

每个 M0 模块都应能回答：

- 它产生哪些事件。
- 它的状态保存在哪里。
- 它的失败如何分类。
- 它如何被 policy 或 approval 约束。
- 它如何被 trace 回放和解释。
- 它如何证明没有绕过 harness。

## 当前结论

forme M0 的验收标准不是“能回答问题”或“能调用工具”，而是能证明完整核心体系已经闭合：入口统一、运行受管、能力可控、行动可审计、主动式可反馈、认知更新先候选、失败可学习、放权可撤销、开源边界可检查。
