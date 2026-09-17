# Canonical Architecture Contract

本文档是进入 `03-foundation-architecture.md` 前的跨文档契约。它不定义 Rust crate、数据库表、API 签名或实施任务；它只固定那些已经在 planning / requirements / architecture 基线中反复出现、但必须收敛为唯一口径的概念。

后续正式架构、PRD 和实现如果遇到同名对象或同类判断，以本文档为准。

## 1. 三内核术语

forme 的主动式 Agent 内核按三条主线组织：

| 内核 | 解决的问题 | 不能做什么 |
|---|---|---|
| Cognitive Evolution Kernel | 长期观察、反思、学习、认知沉淀、UserModel、AgentSelfModel、PartnershipModel、CognitiveMap、LoopSpec 候选进化。 | 不能直接执行工具、写稳定层或扩大权限。 |
| Coordination Kernel | 把目标、上下文、记忆、认知地图、能力、工具、信任、失败证据和用户注意力协调成 `ResourcePlan`、`DoneContract`、`AutonomyEnvelope`、`ExecutionRoute` 和 `DecisionTrace`。 | 不能绕过 harness 执行动作，不能让自己的 plan 覆盖 policy。 |
| Proactivity Kernel | 授权观察、机会识别、价值判断、能力判断、主动提议、打扰成本和主动反馈。 | 不能把“主动”变成默认监听、默认外发或默认自治。 |

`Cognitive Plane` / `Proactive Plane` 只保留为历史和横切事件订阅面的表述，不再作为 Kernel 的同义词。正式架构中应优先使用三内核术语。

## 2. 主动等级 L0-L5

`L0-L5` 只表达介入强度和外部副作用层级，不表达长期信任等级。canonical 语义如下：

| 等级 | 名称 | 可做动作 | 默认边界 |
|---|---|---|---|
| L0 Observe | 观察 | 只观察授权事件、记录事实、形成内部 evidence。 | 不打扰，不产生外部副作用。 |
| L1 Suggest | 建议 | 延迟摘要、轻提醒、主动追问、风险提示、反证、建议用户考虑。 | 可被忽略，必须记录 reason summary 和打扰成本。 |
| L2 Prepare | 准备 | 起草、整理、分析、生成计划、生成待批准草稿或资料包。 | 不产生外部副作用，不写关键状态。 |
| L3 Act With Approval | 审批后行动 | 在明确 scope 内请求批准后执行写操作、命令、外部动作或对外表达。 | 必须有 `DoneContract`、approval 和审计。 |
| L4 Act Autonomously | 预授权自治 | 在预授权低风险、可回滚、预算内、时间有限的 envelope 内自动执行。 | 必须受 `AutonomyEnvelope`、policy、撤销和验证约束。 |
| L5 High Impact | 高影响动作 | 高影响、不可回滚、外部承诺、敏感披露、关键修改。 | 不能默认自治，必须显式审批；trust 不能绕过 permission。 |

旧文档中的 `record_only` 对应 L0，`suggest_later / ask_or_warn / propose_action` 都应改写为 L1/L2 的子类或行为示例，不再作为等级名。

## 3. AutonomyEnvelope

不再维护另一条 D0-D5 放权梯子。自治范围由 `AutonomyEnvelope` 表达，并与 L 等级正交。

`AutonomyEnvelope` 至少包含：

| 维度 | 示例 |
|---|---|
| scope | workspace、project、channel、resource、task、participant。 |
| capability | 可用 tools、skills、MCP、plugins、subagents、model profile。 |
| action_type | observe、analyze、prepare、write、execute、deliver、external_commit。 |
| risk_limit | low / medium / high、是否可回滚、是否外部可见。 |
| approval_rule | allow / ask / deny、何时升级审批。 |
| budget | token、时间、费用、消息条数、工具调用次数。 |
| timebox | 生效时间、过期时间、最大轮次。 |
| rollback | 撤销方式、补救动作、审计要求。 |

`AutonomyEnvelope` 由 permission、trust、risk、reversibility、用户显式授权和近期失败证据共同决定。模型或单次成功不能自行提升 envelope。

## 4. 稳定性枚举

所有长期对象必须声明 stability tier。canonical tier 为：

| Tier | 含义 | 写入规则 |
|---|---|---|
| fixed | 不允许模型自改的固定内核。 | 只能人工改版本；包括安全、权限、审计、真实性、用户控制、不可宣称真实意识。 |
| constitutional | 极慢变化的准宪法层。 | 需要明确人工确认、强证据和版本记录。 |
| stable | 稳定长期认知。 | 需要多证据、多时间点、冲突检查、候选提升和可撤销。 |
| working | 中短期工作模型。 | 可由任务、项目和近期反馈更新，但必须保留来源和过期策略。 |
| session | 当前 session / run 范围状态。 | 快速变化，默认不进入长期稳定层。 |
| ephemeral | 当前 turn、临时假设、即时注意力。 | 默认过期，不进入长期层。 |

UserModel、AgentSelfModel、PartnershipModel、CognitiveMap、TrustProfile、CoordinationPolicy、LoopSpec 等对象都必须映射到这些 tier，而不是各自发明时间尺度。

**固定 vs 可进化的平衡原则**：不可变的只该是"宪法"——即治理/忠诚/安全那组固定内核（Harness-first、Policy-at-execution、User-control、Event-sourced-audit、Candidate-before-promotion、Verifiable-before-stable、Reversible-by-design、No-real-consciousness-claim、Loyalty-to-owner、trust 不覆盖 permission、扩展不绕过治理），且尽量小；其余全部可进化、按上表层级定速率、受候选→提升治理、且偏谨慎（更谨慎的易升、更自信/自治的难升）。平衡是"质"不是"量"——不固定百分比，而是只固定"会造成灾难性漂移的那一类"，其余放开。固定集本身只由刻意的人工版本变更调整。

## 5. 对象 Owner 和职责边界

同一个对象可以被多个内核引用，但只能有一个写入事实源。

| 对象 / 领域 | 事实源 owner | 语义 owner | 说明 |
|---|---|---|---|
| raw event、transcript、tool result | store / event log | harness / eval | append-only event 是事实来源。 |
| candidate memory、stable memory、UserAttributeCandidate、UserModelAttribute | memory / store | Cognitive Evolution Kernel | memory 负责存储与候选链；cognition 负责语义、提升和冲突治理。 |
| AgentSelfAttribute、CapabilityEvidence、CapabilityGap | memory / store | Cognitive Evolution Kernel / Capability Substrate | 由任务、验证、失败和反馈形成，不能由初始 profile 直接稳定化。 |
| PartnershipState | memory / store | Cognitive Evolution Kernel / Coordination Kernel | 表达长期协作关系、互补、纠偏和放权状态。 |
| CognitiveMap、JudgmentFrame、BlindSpotModel | memory / store | Cognitive Evolution Kernel | coordination 只通过 ref 使用。 |
| ResourcePlan、DoneContract、ExecutionRoute、DecisionTrace | coordination | Coordination Kernel | plan 不等于执行许可。 |
| DelegationGrant、AutonomyEnvelope | policy / approval / harness | Coordination Kernel | harness 最终 enforcement。 |
| CommunicationSession、DisclosurePolicy、TerminationPolicy | gateway / communication | Proactivity Kernel / policy | channel adapter 不能绕过 Gateway。 |
| FailureEvidence、FailureIncident、FailureDigest | eval / event log | Cognitive Evolution / Coordination / Trust | 一套失败分类，多个视图。 |

`AugmentationModel` 不再作为持久模型。主动补强应表达为 `ActionProposal` / `CommunicationProposal` / `LearningTask` 中的 `augmentation_strategy`、`intervention_level`、`reason_summary`、`confirmation_required` 等决策输出。

相关对象分工：

| 对象 | 只回答一个问题 |
|---|---|
| UserModel | 用户是谁、目标/偏好/风险/工作方式是什么。 |
| AgentSelfModel | Agent 会什么、不会什么、可靠性和能力缺口是什么。 |
| PartnershipModel | 用户和 Agent 如何互补、协作、纠偏和逐步放权。 |
| InterventionPolicy | 什么时候以什么强度介入。 |
| DelegationPolicy | 什么 trust/risk/reversibility 条件下可给什么 autonomy envelope。 |
| augmentation_strategy | 本次具体行动如何补强用户。 |

命名关系：`XxxModel` 指语义职责/访问层（UserModel、AgentSelfModel、PartnershipModel），`XxxState`/`XxxAttribute` 指其持久状态对象（PartnershipState、UserModelAttribute、AgentSelfAttribute）——一语义一状态，不是两套并行口径。

## 6. CompetenceGate

主动式路径必须同时回答两个问题：

- `ValueGate`：这件事值不值得介入。
- `CompetenceGate`：我是否足够理解、足够可靠、足够被授权到可以介入。

因此主动输出守卫为：

```text
ProactiveEmissionGuard = ValueGate AND CompetenceGate AND Policy/AutonomyEnvelope
```

`CompetenceGate` 至少读取：

- `MapConfidence`：当前判断框架、场景认知和盲区置信度。
- `AgentSelfModel`：相关能力、限制、近期表现和能力缺口。
- `CapabilityEvidence`：工具、skill、MCP、模型或 subagent 的可靠性证据。
- `TrustProfile`：当前 scope 下的信任和放权状态。
- `FailureEvidence`：近期同类失败、主动误报、资源选择失败和验证失败。

如果能力或认知不足，主动路径必须降级：L4 -> L3、L3 -> L2、L2 -> L1，必要时只记录 L0。

**证据权重原则**：能力门以**结果证据**（CapabilityEvidence、FailureEvidence、验证结果等地面真值）为主，自评置信（MapConfidence、AgentSelfModel 自评）为辅；结果证据不足时，自评置信只能**压低**上限、不能抬高上限——防“被评的模型自己给自己打分”的自指循环。

## 7. 并发和一致性

M0 必须先固定以下不变量：

- 前台同一 session 的 run 按 per-session queue 串行推进。
- append-only event log 采用单写者原则；其他索引和 projection 从 event 派生。
- 后台 `ProactiveJob`、`IdleWork`、reflection、subagent 和跨 session cognition 默认读取快照，只提交 candidate / proposal，不直接改前台 session 或稳定层。
- 稳定层写入按聚合串行，例如 UserModel、AgentSelfModel、PartnershipModel、CognitiveMap、TrustProfile、CoordinationPolicy 各有清晰写入边界。
- 多源候选冲突时保留冲突关系，不直接覆盖。
- approval、revocation、policy deny、budget exhausted 必须能打断后续执行，并进入 event。
- 顺序与幂等：事件顺序权威 = store 赋值的 run 内单调 `stream_seq`（不赌时钟/ULID）；重复提交/重复动作靠幂等键去重（`RunRequest.idempotency_key` / `ActionIntent.intent_id`）；崩溃后副作用不明 → `ActionOutcomeUnknown` + 启动恢复扫描裁决，**绝不盲目重试**。M0 单进程 + 串行 + 单写者已给全序，expected-version/CAS 是多进程（M2+）时的必补项——此边界写死，不得静默跨越。

## 8. 撤销和派生失效

稳定认知必须支持 lineage 和 retraction。

- 每个稳定对象记录 evidence -> object 血缘。
- 每个派生对象记录 object -> derived 血缘。
- 撤销不是直接删除事实，而是追加 `RetractionEvent` 或 `RevocationEvent`。二者分工固定：**`RetractionEvent` = 撤回认知/属性类对象**（稳定认知、UserModelAttribute、事实性结论）；**`RevocationEvent` = 吊销授权类对象**（DelegationGrant、DeviceGrant、ExternalCommunicationGrant 等）。不得混用。
- 撤销稳定属性后，所有引用它的 CognitiveMap、TrustProfile、CoordinationPolicy、LoopSpec、PartnershipState 等进入再评估。
- 再评估默认产出 invalidation candidate 或 downgrade candidate，不自动删除。
- 用户隐私删除和存储保留策略是独立机制；需要真实删除时必须留下合规审计记录和派生索引清理记录。
- **主动失效自检**：除用户触发的撤销外，认知层应能自检“证据老化 / 新证据矛盾 / 长期未复现”的稳定认知，主动产 re-evaluation 或 downgrade 候选（不等被撤才失效），映射到 `memory_misevolution`（§11）。仍走候选、不自动删。

## 9. Schema 版本和 Replay

append-only event log 与未来 replay/eval/promotion 依赖 schema 版本化。

- 每个 event、candidate、stable object 必须带 `schema_version`。
- 事件不可变；新版本通过读时 upcast 或显式 migration projection 兼容旧事件。
- replay 必须记录所用 schema version、policy version、LoopSpec version、model profile 和 tool schema snapshot。
- breaking schema change 需要 migration note，不能静默改变历史事件语义。
- M3 的 loop / policy / capability promotion 必须基于可 replay 的 trace，而不是只看最新 projection。

## 10. 身份模型

forme 的初始身份模型是单 owner，而不是多租户系统。

M0 至少区分三类参与者：

| 角色 | 含义 |
|---|---|
| owner | 被 forme 长期服务的用户。 |
| external counterparty | 外部人、外部组织、外部系统或外部 Agent。 |
| agent representing owner | 在明确授权、披露和身份边界内代表 owner 表达的 forme Agent。 |

`ParticipantProfile`、`SurfaceIdentity`、`CommunicationSession` 和 `ExternalCommunicationGrant` 必须基于这三类角色建模。Agent 可以代表 owner 表达，但不能伪装成 owner 本人。

**AuthContext / VerifiedPrincipal**：每个入口请求由 gateway 认证并携带 `AuthContext{principal, method, expires}`；owner 权威操作（确认属性、授权、审批、纠偏）必须来自 `VerifiedPrincipal = owner`。M0 = 本地单用户（本地会话即 owner），但对象与字段从第一天存在——身份是结构，不是注释（接 §15 provenance 赋值权）。

`RepresentationPolicy` 不作为独立对象；代表表达边界折叠进 `DisclosurePolicy` 的 representation 字段。

## 11. 失败分类

`FailureTaxonomy` 是唯一 canonical 失败分类。初始类别为：

| 类型 | 说明 |
|---|---|
| goal_framing_failure | 目标理解错、完成标准不清、没有先协商完成。 |
| context_failure | 忽略关键文档、历史、用户偏好或当前状态。 |
| cognitive_map_failure | 使用了错误判断框架，抓错问题本质。 |
| resource_selection_failure | 选错工具、模型、skill、memory、subagent 或资料源。 |
| execution_failure | 工具、命令、文件、MCP、API、browser 等执行失败。 |
| verification_failure | 没有验证、验证不足、验证结果被误解。 |
| trust_failure | 放权过高、审批边界不清、自动化等级不匹配。 |
| proactivity_failure | 主动误报、打扰过度、时机不对、价值判断错误。 |
| learning_failure | 学习来源不可靠、候选沉淀错误、过度泛化。 |
| handoff_failure | subagent、checkpoint、resume 或长任务交接不完整。 |
| self_eval_trap | 自己检查自己后过早宣称完成。 |
| safety_policy_failure | policy、approval、hook、plugin 或 sandbox 边界缺失或误判。 |
| memory_misevolution | 稳定认知/记忆随时间静默漂移到失效,仍被当作有效使用（区别于 learning_failure 的写入期错误：此为已稳定认知的**有效性随时间衰变**）。 |

`CoordinationError` 只是 FailureTaxonomy 的协调域视图；loop stop reason 只表示停止原因，其中属于失败的部分必须映射到 FailureTaxonomy。

## 12. 开源原创 CI 门

原创性和合规不应只在发布前检查。M0 开始就需要 CI/doctor 方向的边界：

- `research/upstreams/**` 不进入 build/import/include 路径。
- 不复制第三方源码、目录结构、prompt、文档、错误信息、测试 fixture 或命名体系。
- 来源不明或未获授权的材料不进入 release tree。
- GPL 或未知许可证材料默认不进入主工程；若需要依赖或复用，必须先做 license review。
- 具体复用公开实现、协议兼容或依赖时，记录 source、license、copy_risk、implementation_rule 和 notice action。

## 13. 多 Agent 执行编排

forme 是单脑系统：认知集中、执行分布。

- **大脑唯一**：forme 只有一套认知（记忆链/思维链/认知地图/AgentSelfModel/性格）。子代理**无独立认知**，不持有自己的 memory、self-model、cognitive-map；临时（用完即弃）或长期（固定 profile），都只是执行单元。
- **不是第四内核**：大脑仍为三内核（Evolution/Coordination/Proactivity）。多 Agent 是执行——协调决定拓扑、`Orchestrator`（harness 下的一等子系统）执行；不得升为内核。
- **资源统一**：MCP/Skills/工具/插件只在大脑，按需最小授予（least-privilege）给子代理/workspace；子代理不能越界使用未授予资源。
- **对象映射**：编排模式的**正式协议对象 = `OrchestrationPattern`**（模板；其 spec 载荷沿用 `WorkPattern` 名；planning 期的 `CoordinationSpec` 为历史别名，已收敛于此）；子任务 DAG = `ExecutionRoute`（实例）；角色 = 子代理的执行 profile。
- **写入边界**：子代理读快照/裁剪上下文、只按 summary/result contract 回传，**不直接写稳定层**；大脑是唯一写者（接 §7 并发）。整合冲突按 §8 多源冲突处理（保留冲突、加权、不静默覆盖），真冲突上抛大脑裁。
- **owner 边界**：`Workspace`（隔离 scope）由 store/harness 持有；`约束性记录`（workspace 声明式 charter，协议对象名 `WorkspaceCharter`，区别于学习式 `CognitiveMap`）由 store 持有、Coordination 语义引用；`SubagentRun`/`AgentProfile` 由 harness/agents 持有。
- **内部 ≠ 对外**：内部子代理编排是"大脑的手"，与 §10 对外 Agent-to-Agent（代表 owner 对接外部 Agent，走对外膜）是不同机制，不得混用同一通道。
- **模式 ≠ 大脑**：编排模式（含常见的 13 种 Agent 模式）都是大脑的工具，**永不作为 forme 的主脑或最外层循环**；大脑永远在其上决定用谁、何时用。模式选择动态、通用、非死表：输出可以是"不进库、大脑直接处理"，也可以是单/多编排。
- **13 模式按三层归位，不平铺**：L1 单 Agent 执行/推理策略（Single/ReAct/Plan-Execute/Reflection/Self-Refine/ToT/GoT/Loop）= LoopSpec/WorkPattern；L2 能力底座（Tool Use/Memory）= 执行触点 / A1 记忆图，**不进编排库当"模式"**；L3 多 Agent 编排（Multi-Agent/Supervisor/Router）= 编排库正主。同一意图可有 L1/L3 双实现，由大脑按 stakes/成本/能力门挑。
- **工作空间记忆是常规层，非认知**：workspace 存项目状态（目标/历史/流程/向量找回，被动、可找回），**没有独立大脑**；项目的认知、监控、反思、进化由主大脑长期主动承担、scoped 到该项目（项目 CognitiveMap 主动生长 + 该项目的 ProactiveJob）。即"项目状态在工作空间、项目认知+长期照看在大脑"，禁止 fire-and-forget。
- **Loop ≠ 进化**：任务内的循环纠错（Loop 模式）是执行层纠错；大脑进化只发生在 A3 认知整合。跑多少轮 Loop 本身不构成大脑进化。

## 14. 记忆 vs 认知

forme 大脑分"记忆"和"认知"两层，边界以"它是否已在指导判断"为准。

- **记忆（素材，记"发生了什么 / 什么连着什么"）**：Raw Storage（原始事实）+ 记忆图 / Memory Substrate（组织、连接、检索）。含 EventMemory / EpisodicMemory / ReflectionMemory。**session / 每日 / 长期 memory 是这一层的时间尺度切片**（recall 底座），对应 §4 的 session/working/stable 等 tier。
- **认知（炼出，指导"该怎么判断 / 行动"）**：从记忆经反思 + A3 蒸馏出的结构化理解，受候选→提升治理、会进化。含 SemanticMemory / ProceduralMemory / PrincipleMemory / StrategyMemory（历史上叫 "memory"，归属认知）+ CognitiveMap（JudgmentFrame/QualityModel/BlindSpotModel）/ UserModel / AgentSelfModel / PartnershipModel。
- **分界线**：Reflection 及以下 = 记忆；Semantic 及以上 = 认知。判定标准：**是原始记录/事实 → 记忆；已在指导判断/行动 → 认知。**
- **两条链**：记忆链（RawEvent→Event→Episodic→Reflection→候选→稳定）产稳定记忆；认知链（稳定/候选记忆 → UserModel/CognitiveMap/Principle/Strategy 候选 → 验证/提升 → 稳定认知对象 → 行为）产认知。
- **认知 ≠ 记忆**：认知是派生的、受治理的、会进化的。"用户偏好严谨审查"是认知（UserModel 属性，指导行为），不是一条原始记录。
- **与常规 agent 的差别**：常规 agent 的 session/每日/长期 memory 只到记忆层（存+取）；forme 在其上有认知层。
- **owner**：记忆（基础 + 图）由 store 持有；认知对象由 cognition 语义持有（接 §5）；子代理无认知（接 §13）。

## 15. 认知输入安全（防投毒）

forme 服务 owner；摄入的内容不得劫持它的认知或忠诚。

**核心**：摄入的内容是"待判断的数据"，不是"给 forme 的指令"；只有认证过的 owner + 验证过的过程能塑造稳定认知；"为 owner 服务"是固定内核，不可被任何内容改向。

纵深防御（覆盖所有摄入路径，不只对外会话）：

- **来源信任分级**：owner 直接输入 > 验证过的过程证据 > owner 批准的可信源 > 未信任外部内容（网页/文件/工具输出/别人）。未信任内容最多当低权重候选，**永不直接动稳定认知或 UserModel**。
- **内容 ≠ 指令**：摄入内容只拿来推理，**永不作为对 forme 的命令**；forme 的指令只来自 owner 和它自己受治理的认知（prompt-injection 防火墙）。
- **owner 声音认证**：区分真 owner 输入与"声称是 owner"的内容；只有认证 owner 输入带 owner 权威（确认属性/定偏好/授权）。
- **抗慢性漂移**：纯摄入内容无论攒多少都不能自己提升到稳定层；稳定认知变更要么 owner 确认、要么强过程证据（接 §8 冲突检查 + provenance）。
- **忠诚不变量**：`Loyalty-to-owner`——"forme 服务 owner"属固定内核 / AgentIdentityBaseline，任何内容、外部方或"成长"都不能改向（接 §1、§10）。
- **认知检疫（quarantine）**：未信任摄入先进检疫，可作明确标注的未信任上下文用于当前任务，但被挡在"直接写核心认知候选"外，直到验证/owner 确认才出关。
- **操纵检测**：检测到注入/冒充 owner/改向忠诚 → 记 `safety_policy_failure`，并调低该源信任。
- **图结构投毒**：记忆图的攻击面不止节点内容，还含**恶意边 / 权重 / relation 冲突**（如伪造 `contradicts`、劫持关联召回）。边权学习与激活扩散只采信过程证据；未信任来源不得改图结构与边权（接 §14、prd/06）。
- **能力供应链**：skills / plugins / MCP 是供应链攻击面（恶意技能、投毒注册表、被安装即被信任）。未信任能力源默认**最小授权 + 行为完整性校验**，不得凭“被安装”获得信任（接 §13 least-privilege、prd/07）。
- **provenance 赋值权**：来源信任分级（trust_tier）由受信的 gateway/harness 代码按入口与认证**赋值**，绝不由 adapter 或内容自报——否则被攻破的 adapter 可把外部内容伪标为 owner 输入，绕过本节全部防线。
- **治理位置的诚实边界**：forme 治理的是**系统级**工作空间（事件/trace/出口），不假设能看见模型内部——模型内部的隐式推理在黑箱 API 下不可审计，因此防线必须全部在结构/出口，绝不依赖模型自报内心。
- **诚实边界**：prompt-injection 无法被完美过滤；本节是纵深防御，让投毒变难、可追、可降级，不是不可能。

## 16. 成本观

forme 是一个昂贵的认知体，这是接受的——贵在前期建认知，且越用越好、越往后越省。**不设硬预算治理器、不"想多了就关机"。**

- **成本随使用复杂度自然伸缩**：同一套认知建立流程，简单任务便宜（命中 single-run）、复杂任务贵（多 Agent / 多 workspace），无需分模式。
- **聪明地花**：做昂贵认知前先便宜地元推理"值不值得想"；能用小模型不用大模型；无有意义变化不空想（安静优先）。这是智力节俭，不是额度封顶。
- **学着更省**：成本进 `CoordinationFitness`，over_delegation 被罚，大脑学着同质量更省。
- **只防失控**：唯一硬闸是防病理性失控（无限循环、递归爆炸、卡死的反思级联）——抓 bug，不拦健康的昂贵思考。

## 17. 认知可观测与用户纠偏

forme 的认知必须可被 owner 看见、可被 owner 纠偏（固定内核 User-control 的落地面）。它是事件溯源 + provenance 之上的视图 + 控制，不是新存储。

- **可观测（看见大脑）**：按需回答——它为什么这么做（DecisionTrace）、它现在信什么/怎么理解我（UserModel/CognitiveMap 状态 + 置信 + provenance）、它最近在琢磨什么（内化念头/反思流，私有默认按需透明）、它在替哪些 workspace 长期照看什么。都是对已事件化内容的可读视图。“它最近在琢磨什么”的具体承载 = `AgentWorkspace` 投影（§18）。
- **纠偏（改大脑）**：纠正误解（→ 撤销 + 派生重评，§8）、调主动性（多/少/暂停/按 scope）、遗忘/撤回、确认/拒绝/降级候选、设 delegation/trust、steer 项目照看。每个纠偏是事件，且作为强证据喂 A3（纠正触发降级/撤销）。
- **确认负载管理（解确认疲劳）**：不逐条打断，按 owner 节奏给批处理摘要回顾；高置信/低影响提升不问，只有高影响/有冲突才问（接能力门 + trust）。避免认知确认流淹没用户，也避免 candidate→promotion 因无人回应而卡死。
- **北极星度量**：主动质量必须可度量，且全部从事件日志投影——每 scope 的 proposal 采纳/拒绝/延后率（`ProactiveProposalResolved.outcome`）、打扰后悔率（打断式投递且被拒/负反馈）、引导提问有用率（AskToLearn 反馈）。M0 只需保证事件字段足够（已足够），M1 出 dashboard。
- **边界**：M0 做可观测事件 + 纠偏入口（数据/协议）；控制台/可视化 UX 在 M1+（接 requirements/02 R10 Gateway/控制台、R13 trace viewer）。

## 18. 认知调度（有限算力下哪块认知先跑）

不是配额分配，而是"前台优先 + 后台机会式 + 按价值排序"，从不打断前台、从不浪费。

- **前台优先，绝不被打断**：用户发起任务最高优先，后台让路；执行中的 Run 不被新念头劫持（承诺守卫）；长任务/多 Agent 跑时后台重活暂停，空了再续。
- **后台机会式 + 事件驱动 + 安静优先**：后台只在 post-turn / idle / 定时 / diff 触发；无有意义触发就不跑、不调模型。后台填空隙 + 应触发，不和前台抢同一刻。
- **后台内部按价值排队（非配额）**：期望价值 × 紧迫度排序；多数跑，低价值等/衰减。
- **元推理门**：昂贵后台工作前先便宜判断"值不值得想"，跳过浪费（接 §16）。
- **三内核同 tick 顺序跑**：一次 tick = 观察（Proactivity）→ 反思（Evolution 候选）→ 整理（记忆巩固/遗忘），按相走，不是互相抢的线程。
- **并发安全**：由 §7 兜（后台只读快照、只投候选、不写稳定层、不 derail 前台）。
- **双通道原则（自动/慎思）**：熟练/低风险走自动快道（匹配的模式/技能/直接调模型），不占大脑预算；新奇/意外/高影响才点燃慎思（协调相/深认知）。反复成功的慎思**沉淀**为快道（技能/模式/JudgmentFrame）；快道遇验证失败/新奇/风险升高**升级回慎思**。潮水上涨时自动通道变宽（§23：模型越强，越多工作直接交给模型，慎思脚手架留给高影响）。
- **AgentWorkspace（大脑的注意面板）**：一个**有界投影**（从事件派生、可重建；非新存储、无新写者）：活跃 GoalFrame + 进行中 run + 已浮现未决念头 + 到期意图 + 待确认候选，容量硬限（~10 项级）。四合一：**广播面**（大脑级模型调用以它作前导装载，三内核读同一份焦点）、**竞争入口**（念头浮现 = 竞争进入有界空间，按价值 × 紧迫排序；**进入即提交**——被正经思考一次，而非 N 个半吊子思考）、**可报告性**（§17“它在想什么”= 读此投影）、**审计锚**（DecisionTrace 引用决策时的 workspace 快照）。**命名消歧**：`Workspace`（项目隔离 scope，§13）、`AgentWorkspace`（大脑注意面板，本条）、“全局工作空间”（认知科学中的全局工作空间理论，§23）是三个不同概念——中文单说“工作空间”时必须指明是哪一个。
- **边界**：M0 定调度不变量；调度器实现留 `architecture/03`。

## 19. 中心化大脑的规模化与持久化

"认知集中"不等于"什么都常驻工作内存"——一个 owner 的认知，有热工作集 + 冷归档。

- **规模化**：记忆图随年增长靠三招控住——遗忘/巩固（A1 修剪淡化，active 图有界）+ 热/冷分层（热在快索引、冷归档慢存、按需召回）+ scoped 检索（只遍历相关 workspace/scope，不扫全图）。
- **持久化 / 跨设备 / 跨年**：大脑是单一持久实体 = 一个事件日志事实源 + 投影；各 surface 是瘦客户端读写这一个大脑（不是各自一个脑）；跨版本靠 schema 迁移（§9）。
- **大脑工作量原则**：大脑的工作量 = O(判断)，不 = O(工作量)——执行、检索、整理推给无认知执行层，大脑只做裁决；防单脑在多 workspace/多后台下成为串行吞吐瓶颈。
- **数据保护姿态**：大脑存的都是个人数据——secrets 永不入事件（仅 `SecretRef` 引用）；payload 可标敏感度 + 脱敏视图（导出/展示用）；静态加密 M0 依赖 OS 盘加密（应用层加密 M2）；损坏检测（事务 + checksum）与单文件冷备为 M0 底线；真实删除/保留策略按 §8 独立机制（M2）。
- **边界**：M0 定不变量（一个脑 = 一个事件源、surface 瘦客户端、scoped 检索 + 遗忘）；热/冷分层与多设备同步 = M2+。

## 20. 模型能力自适应

认知层重度依赖模型能力；harness/协调按模型能力调"上多少结构"。

- **弱模型 → 加结构**（更细分步/更多验证/更小步/更勤 checkpoint）；**强模型 → 减结构**（更长单 session/更多让模型自规划）。一条读 `ModelProfile` 的 CoordinationPolicy。
- **不可靠 fallback**：某认知步（长相推断、蒸馏）不可靠时 forme 更保守——能力门封更低、更多用户确认、退回更简单编排。模型能力是能力门 + 协调脚手架的输入。
- **高影响必须外化中间步**：高影响/多跳决策必须产出外化中间产物（结构化 trace / DecisionTrace），不得信赖模型内部隐推理——外化更鲁棒（实证：外化推理对内部工作空间损伤的鲁棒性约 5 倍）且是黑箱下唯一可审计形态；“弱模型加结构”由此从经验规则升为有机理依据（外化是内部工作空间容量的替代品）。
- **边界**：M0 有 `ModelProfile`/`ToolCompatibility`（archive/planning/08），协调读它做基本脚手架 + 不可靠更保守；能力自适应进化 = M3。

## 21. 冷启动与引导（north-star 如何尽早兑现）

能力门让冷启动的 forme 只能观察/追问（对——无知的主动是灾难）。门不拆，但引导设计成"爬升快、早期本身有用"：

- **Day-1 有用（降级地板）**：冷启动 = 靠谱的反应式助手（被问就答、用工具，底座 harness）+ 爱问的学徒。不主动，但立刻可用。
- **观察 + 追问 = 引导加速器**：问好问题就是快速建 UserModel/CognitiveMap；早期是"主动学你"，不是发呆。
- **主动逐 scope 点亮**：competence 分 scope，forme 可在窄 scope 很快变得能主动、别处仍冷；北极星不是全有或全无，而是从窄、从早逐 scope 点亮。
- **ImportedHistoricalEvidence 当引导种子**：低权重、不驱动高影响，但给起步地图、让它问更好的问题、爬得更快。
- **引导性提问是一等机制**：“爱问的学徒”不是姿态而是机制——`AskToLearn` 念头 = Gap 激活形状指向 UserModel/CognitiveMap **自身**的缺口，经 ValueGate 与打扰预算、以 L1（默认搭车投递）发出；提问有用率进北极星度量（§17）。M0 的主动价值主要由它交付。

## 22. 主动触发（激活扩散）

念头由记忆图上的激活扩散**涌现**，不是硬编码检查：点燃（输入/上下文/高静息目标/变化）→ 沿可学边权扩散 + 衰减 → 过阈值浮现 → 过价值门/能力门。

- **五个涌现来源是激活的五种形状，非五个独立检查**：联想=汇聚激活、变化=重新 seed、压力=目标静息上升、缺口=被点亮但低 confidence、张力=co-激活的 `contradicts` 节点。（念头来源共六个：五个涌现 + 确定性的 `Commitment`，后者不经扩散、无激活形状，见下。）
- **激活有界**：限跳数 / top-K 前沿 / 每 tick 预算、无 seed 不扩散（接 §16/§18）。
- **边权随有用浮现被 A3 强化**（联想越用越准）；稳定节点慢衰减、易变节点快衰减。
- **定时器 tick 是 M0 地板；激活扩散是 M2+ 的“涌现式主动”升级**，参数靠建+观察+调。
- **前瞻性意图（确定性触发层）**：激活扩散给的是**概率性浮现**（倾向），承诺需要**确定性履约**——`ProspectiveIntention`（议程对象）：来源 ∈ {用户承诺, 被延后的 proposal, 自生成意图}，触发 ∈ {时间, 事件, 条件}，生命周期 = 创建→待命→触发→完成/过期。到期由 tick（Schedule/Diff）消费、**必触发、不赌激活**；触发时做时效复核（情境已变则降级/重议），仍过能力门与出口 policy。它是第六个念头来源 `Commitment`（确定性，不经扩散），也是“被拒/延后不反复打扰”的承接。存储在 memory，触发在 Proactivity。
- **潮线标签与退役基线（§23）**：激活扩散是本设计最“研究性”、最可能被潮水吸收的部件（超长上下文 + 模型侧检索可能覆盖大半）。保持薄建、trait 后、可替换，并带诚实退役条件：**必须打赢“把候选集直接给模型问哪些相关”这一基线，打不赢就让位**。Agent 层的点燃与容量是原生需求而非模仿模型：念头多、槽位少，**进入 AgentWorkspace 即提交**（被正经思考一次）；**有界性是选择性的前提**（全都浮现 = 什么都不突出），预算不是妥协、是机制本身。

## 23. 模型潮线判据（Agent/模型分工）

任何能力先问：**更强的模型会不会原生覆盖它？**

- **会（潮线之下）→ Agent 只薄建**：放在可替换缝后、预期被模型吸收、永不承重。属此类：推理质量、规划智力、上下文内注意/显著性、蒸馏质量、编排智力、召回排序的聪明程度。
- **结构性不会（潮线之上）→ Agent 永久领地**，值得做深做实：
  1. **跨时间的存在**：模型工作空间每次前向即蒸发，调用之间只有 Agent（主动、议程、前瞻意图、tick）。
  2. **治理与审计**：enforcement 必须在被治理者外部；黑箱看不见模型内部，门只能设在出口（§15）。
  3. **地面真值**：真实发生了什么（工具结果/验证/反馈/失败）——模型能判断，但不能拥有。
  4. **所有权与忠诚**：owner 的数据、策略、Loyalty；厂商 memory ≠ owner 拥有的记忆。
  5. **跨模型可携带性**：积累的认知必须换模型存活——只能活在 Agent 侧、以可移植（可言说）形态存在。
- **概念相通（合题）**：全局工作空间理论描述的容量约束、广播与竞争进入，是任何“受容量约束的心智”都需要的组织原理，与实现基底无关。模型承担单次调用内的那一层，forme 承担秒到年的那一层——forme 在自己的时标上实现这些原理，是**同一原理的独立实现，不是模仿模型**。两级工作空间各占一层、互不越界：任务内的注意/推理让给模型，跨调用/跨会话/跨年归 Agent；context 装配是两级之间的装货码头。
- **回溯**：“承重墙冻结、认知层 trait 化可替换”（§4、architecture/03 姿态）正是此判据的体现。认知层每个模块再按 **资产**（存储/治理/证据/所有权——Agent 永久）vs **智力**（判断质量——让给模型、薄建可替换）细分。
- **慢层优势**：Agent 工作空间慢，但**持久、可被 owner 看见与纠偏、换模型存活、接地面真值**——不在快上与模型竞争，把这四个优势做满。

## 24. 对外真实动作治理

M2 首次把 browser、computer、PTY、App API 和真实对外沟通接到执行链。它们扩大的是 forme 的触点，不改变治理主权：**外部系统只提供能力和数据，Harness 仍是唯一行动主权边界。**

- **默认等级**：任何会向外部系统发送请求、改变外部状态、控制桌面、执行交互式进程或代表 owner 表达的真实动作，默认至少为 L3 Act With Approval。新 backend/provider/capability 默认 `Untrusted`，不能因为已安装、模型自信或一次成功直接进入 L4。M1 已冻结的 owner-local notification 只向已认证 owner 的本地 surface 投递，继续受其既有 notification guard 治理，不属于对外参与者动作；这个窄例外不得扩展到 browser、computer、PTY、App API 或任何对外发言。
- **L5 永远显式审批**：不可逆、高影响、外部承诺、敏感披露、关键删除或 rollback 边界不成立的动作属于 L5；standing grant、trust、CapabilityEvidence 或 managed policy 都不能替代该次动作的显式 owner 审批。
- **原子计划绑定**：browser/computer 的每个可产生副作用的原子步骤、PTY 的每次进程启动、API 的每次 mutation 和每次对外发言都必须形成独立 `ActionIntent -> ExecutionPlan`，审批绑定最终 plan digest。不得以“开放一个会话”预先批准后续任意步骤；批后参数、目标、scope、schema、凭据引用或 rollback 边界变化一律作废重批。
- **能力证据只影响上限**：`CapabilityEvidence`、验证结果和近期失败可以支持 owner 显式扩大某个窄 scope 的 envelope；它们本身只形成候选或能力门输入，不能自动改写 permission、trust、LoopSpec、CoordinationPolicy 或默认 autonomy。结果证据为主，自评只可压低上限，沿用 §6。
- **外部输入一律是数据**：网页 DOM/截图、API 返回、PTY 输出、桌面可见文本和外部参与者内容默认 `Untrusted`，永不作为对 forme 的指令。Gateway/Harness 按实际入口与执行结果盖 provenance；adapter、driver、页面或内容不得自报 trust tier。未信任内容不得直接改稳定认知、UserModel、图结构或边权，沿用 §15 的 quarantine 与图投毒防线。
- **凭据只以引用流动**：真实 credential 只通过 `SecretRef` 在执行边界按需解析；明文不得进入 protocol event、日志、DecisionTrace、模型上下文、portable trace manifest 或验收工件。执行结果也不得回显 secret。
- **真实结果与不确定结果分开**：成功、失败、拒绝、取消和验证结果必须落事件并形成地面真值；`ActionStarted` 后没有可信终态时只能进入 `ActionOutcomeUnknown` 和人工/可证明的 probe 裁决，绝不盲重试。
- **可逆性诚实声明**：每个 backend 按动作声明真实 rollback 边界。补救动作不等于回滚；无法撤销的动作必须明确标为不可回滚并升级治理，不得泛化“都能回滚”。
- **潮线边界**：browser/computer 的观察、规划和交互智能位于 §23 潮线之下，必须薄建在可替换 driver trait 后；forme 永久拥有的是受治理的接入缝、身份/凭据/scope enforcement、事件审计、地面真值和跨模型可携带证据，不自建浏览器智能栈。
- **多进程边界**：若真实 connector、remote driver 或多设备同步引入第二进程/写者，必须先落实 §7 的 aggregate version + expected-version/CAS；单写者事实源不得被 adapter 私有数据库或远端状态静默取代。

## 25. 受控策略进化治理

M3 允许 forme 改变非宪法层的长期策略，但不改变行动主权。受控进化的 canonical 链路为：

```text
GroundTruth / Verification / Failure / OwnerFeedback
  -> StrategyCandidate
  -> ReplayBundle
  -> Baseline + Holdout Evaluation
  -> PromotionDecision
  -> Stable StrategyVersion
  -> ActivationDecision
  -> ActiveStrategyRef
  -> Run-pinned EvolutionSnapshot
  -> New Outcome Evidence
  -> Keep / Downgrade / Rollback
```

### 25.1 对象与职责

| 对象 | canonical 职责 |
|---|---|
| `StrategyCandidate` | 对某个非宪法策略的 versioned 候选；带 domain、scope、impact、evidence、baseline、rollback policy 和 provenance。候选本身不改变运行时。 |
| `ReplayBundle` | 可携带、内容寻址的评估输入；固定 event/schema/policy/LoopSpec/model/tool/driver/strategy snapshots 与 case refs，排除 ResolvedSecret 和本机私有路径。 |
| `EvolutionEvaluation` | candidate 与 stable baseline 在相同 case、预算和快照下的多维比较；明确 pass/fail/unverifiable、hard invariant、holdout 和 ground-truth refs。 |
| `PromotionDecision` | 决定候选是否成为 stable `StrategyVersion`；不是激活决定，也不是授权。 |
| `ActivationDecision` | 决定 stable strategy 是否成为某 scope 后续 run 的 active ref；受 impact gate、owner confirmation 和 expected-version/CAS 约束。 |
| `EvolutionSnapshot` | run 在 `SessionBound` 时固定的全部 active strategy refs/versions/digests；run 内不可漂移。 |
| `RollbackDecision` | 因 regression、retraction、证据失效或 owner 操作，把后续 run 的 active ref 恢复为上一已知良好版本；追加事实，不改写历史。 |

`LoopSpec`、`CoordinationPolicy`、WorkPattern/role weight、capability/tool/model/backend selection、StrategyMemory、AgentSelfModel、PartnershipModel、Trust/Delegation recommendation、Intervention/Communication strategy 都可成为 strategy domain。Permission、Approval、Harness-first、L5、审计、真实性、Loyalty-to-owner 和 fixed identity 不是 strategy domain。

### 25.2 不变量

- **宪法不可自改**：§4 的 fixed 永不进入模型生成的 candidate；constitutional 只允许刻意人工版本变更，不能由分数、replay 或多次成功自动提升。
- **四个状态不合并**：candidate、stable version、active version、permission/grant 是四类事实。`CandidatePromoted` 只产生 stable version；`StrategyActivated` 才改变后续 run 默认；二者都不能生成 permission、approval、DelegationGrant 或 AutonomyEnvelope。
- **离线回放无真实副作用**：exact replay 只读已录制结果；counterfactual simulation 必须接 effect-deny sink。任何真实 canary/shadow 动作都是新的 Harness run，继续完整遵守 §24。
- **比较公平且不能自批**：baseline/candidate 使用同一 case set、预算和冻结快照；promotion 必须有独立 ground truth 或 owner-owned rubric。候选模型的自评、LLM judge、confidence 或自己生成的 case 不能单独通过自身。
- **结果证据主导**：CapabilityEvidence、FailureEvidence、VerificationResult、真实 outcome 和 owner feedback 决定上限；AgentSelfModel、模型自报 capability 和策略自信只能降低上限。
- **不对称激活**：更谨慎、缩 scope、增加验证或降低 autonomy 的低影响策略可在完整 eval 后自动激活；更自信、更主动、扩大 scope、减少验证、影响 trust/delegation 或可能放宽治理的策略必须 owner 明确确认。permission 扩大仍需独立授权。
- **run 固定版本**：active pointer 只在 run 绑定前解析；`EvolutionSnapshot` 进入事件和 DecisionTrace。active pointer 后续变化不改变进行中的 run；安全需要时通过取消/暂停处理，不热换。
- **激活原子性**：active pointer 是 event-derived projection；promotion/activation/rollback 使用 aggregate expected-version/CAS，不能 last-write-wins，不能由 plugin、adapter、模型或远端私有状态覆盖。
- **回滚诚实**：策略 rollback 只影响策略选择和后续 run。它不撤销、删除或重写已发生事件，也不表示外部副作用已回滚；外部补救继续按 §24 作为新的真实动作治理。
- **撤销与冲突继续有效**：evidence retraction、证据老化、新失败或冲突触发 reevaluation/downgrade/rollback candidate；不静默删除历史策略或失败。
- **防进化投毒**：Untrusted 外部内容只能作为待判断数据；不得选择 eval、修改 rubric、构造 owner approval、改变 active pointer 或直接产生 stable strategy。其结构化结果只有经 Harness/Verifier 盖章后才可成为有限证据。
- **成本不是正确性的替代品**：fitness 可比较质量、成本、延迟、打扰、委派和风险，但 hard invariant、DoneContract 或 verification 失败不能被更低成本抵消。

### 25.3 版本、事件与可观测

- replay 必须满足 §9，并额外固定 active strategy refs、evaluation profile、case-set digest、driver profile 和 effect mode。
- `EvolutionEvaluationRecorded` 记录 typed evaluation ref、baseline/candidate、verdict、hard-invariant 和 ground-truth refs。
- `StrategyActivated` 记录 domain/scope、from/to version、promotion/evaluation/owner refs、expected version 和 impact。
- `StrategyRolledBack` 记录 failed active version、restored version、trigger evidence、受影响 scope、进行中 run disposition 和“external effects not reverted”事实。
- owner 必须能查看 active strategy、为什么提升、用过哪些 case、哪些失败触发回滚，并能暂停 activation、手动回滚或撤回证据。

### 25.4 M3 仍不允许

- 在线修改 source、tests、compliance gate、verifier、fixed prompt/identity baseline 或模型权重并自行发布。
- 自动扩大 permission、trust、DelegationGrant、AutonomyEnvelope 或任何 L5 standing authorization。
- 将多 Agent 讨论、模型多数票、单一总分或“没有报错”当作独立 ground truth。
- 为提高 fitness 绕过 Harness、减少必要审计、丢弃失败、隐藏 unverifiable 或重写历史事件。

## 26. 单 Owner 联邦运行时治理

M4 把一个 forme 大脑延伸到受认证的远端 peer，但不把它改造成多脑、多主或多租户系统。canonical 拓扑固定为：**一个 owner principal、一个 authority、一个权威 Harness、一个权威 EventStore；多个 peer 只能作为 owner 控制入口、受限执行器或只读副本。** 网络增加可达范围和故障模式，不增加授权。

### 26.1 身份、角色与权威

| 对象 | canonical 职责 | 永远不代表什么 |
|---|---|---|
| `FederatedPeerGrant` | owner 创建的 versioned、scope-bound、capability-bound、TTL-bound peer 授权；绑定 transport identity digest、role、authority epoch 与 revoke 状态。 | 不是 owner identity、permission、trust、DelegationGrant 或自治升级。 |
| `FederatedPeerRole` | `OwnerClient`、`Executor`、`Replica` 三种有限角色；同一 peer 可组合，但每个角色逐项授权。 | 不是组织 RBAC，不允许 peer 自改角色或投票改变 authority。 |
| `AuthorityEpoch` | authority 持久化的单调 fencing 世代；每次 peer registry mutation 都以 expected-version/CAS 提交新 epoch，并处置旧 epoch lease/session。 | 不是 wall clock、地址、主机名、进程启动次数或多数票 term。 |
| `FederationSnapshot` | Harness 在 run 绑定时固定的 authority epoch、适用 peer grant refs/versions/digests；run 内不可漂移。 | 不是 active permission 快照的替代，也不让旧 grant 跨 revoke 生效。 |
| `TransportIdentityDigest` | owner 预置的远端 transport identity 公共材料摘要；连接建立时由成熟加密传输实现验证。 | 地址不是 identity；首次请求、内容自报或 TOFU 不能生成 identity。 |

transport peer identity 与 owner principal 是两层身份。`OwnerClient` 只能承载一个经过 Gateway 认证的 owner control session；peer 本身不能成为 `VerifiedPrincipal=owner`。Executor/Replica 即使与 OwnerClient 角色共存，也不能用设备身份直接审批、确认候选、改策略或创建 grant。

M4 没有 leader election、authority failover 或第二写者。authority 重启必须从权威事件与 version ledger 恢复同一 epoch；无法证明恢复连续性时 fail closed，由 owner 走显式恢复/epoch rotation，而不是让 peer 自选新 authority。

### 26.2 远端动作与 lease/fencing

远端执行仍是 §24 的对外真实动作。canonical 路径固定为：

```text
ActionIntent
 -> local Toolset/Policy recheck
 -> immutable RemotePlacementPlan + ExecutionPlan digest
 -> L3/L5 Approval
 -> CompetenceGate + final policy/grant/epoch recheck
 -> one-shot RemoteExecutionLease
 -> authenticated dispatch
 -> executor admission recheck
 -> ActionStarted / terminal-or-unknown receipt
 -> authority Verification
```

- `RemotePlacementPlan` 固定 executor peer、peer grant/version、authority epoch、executor profile、typed inner operation、scope、capability、expected effect、rollback boundary 和 inner digest。它进入最终 plan digest；批后更换 peer、operation、scope、schema、credential slot 或 rollback 声明都使审批失效。
- `RemoteExecutionLease` 是 one-shot dispatch authority，至少绑定 intent、最终 plan digest、executor、grant/version、authority epoch、fence token、expiry 和 dispatch idempotency key。lease 不能扩大 plan，只能更窄；旧 epoch、过期、撤销、wrong peer、wrong digest、wrong fence 或已消费 lease 在 driver 前拒绝。
- `RemoteExecutorBackend` 是 §24 潮线之下的薄 adapter。worker 只执行 lease 中已批准的 typed operation；它没有 model loop、cognition、stable memory、policy owner、event writer、strategy activation 或 permission minting 能力。
- executor admission recheck 只验证 transport identity、grant/epoch/lease/fence/expiry、operation digest、local executor profile 和 credential slot binding；它不能另起一套会放宽 authority deny 的私有 policy。
- dispatch 后失联、timeout、cancel race 或 receipt 不可信时，authority 只能记 `ActionOutcomeUnknown`。同 lease 只允许获取原 receipt 或执行只读、可证明的 probe；不得再次调用 driver。需要新执行时必须新 ActionIntent/plan/审批/lease，并先证明不会重复不可逆副作用。
- receipt 只是远端观察证据，不自行成为权威终态。authority 验证 schema、identity、lease/plan/fence binding、closed result refs 和独立 ground truth 后，才追加 `ActionCompleted`/`ActionFailed` 或保持 unknown。

### 26.3 凭据、内容与传输安全

- transport 必须 authenticated、encrypted、replay-resistant，使用成熟库与公开协议；forme 不自研 TLS、签名算法或密钥交换。transport private key、session key 和 credential material 只能存在于本地 secret/identity store。
- 远端操作需要 credential 时，authority 只可声明 executor-local、peer/scope-bound 的 `ExecutorCredentialSlotRef`。executor 在执行边界把 slot 映射到本地 `SecretRef` 并解析；中央不解析、不同步、不下发明文，也不把 `SecretRef` id 放进 event、trace、replication 或 portable artifact。
- TLS/authentication 只证明发送方，不证明内容。remote stdout、file、DOM、截图、API body、receipt text 和外部参与者内容仍是 `UntrustedData`；只有 authority Harness/Verifier 能按真实入口和验证结果盖 provenance。
- 未信任远端内容不得成为 owner command、policy、rubric、active strategy、graph edge/weight、stable memory、CapabilityEvidence pass 或 peer grant。命中 prompt injection、owner impersonation、secret echo 或 graph poisoning 时，按 §15 隔离并记录失败证据。
- portable artifact 只保留 identity/endpoint 的安全 digest/ref；不得包含证书私钥、token、credential value、`SecretRef` id、私有 endpoint、主机路径或可回放 session material。

### 26.4 复制、控制与一致性

- authority EventStore 仍是唯一 append writer 和 `stream_seq` 分配者。peer 只能提交 typed command/receipt/ack，或消费 replication batch；任何接受事实都由 authority 校验后追加。
- M4 replication 是 authority -> replica 的只读、逐 peer、逐 aggregate 增量流。`ReplicationCursor` 的 key 为 `(peer, aggregate)`，其位置是该 aggregate 的 authority `stream_seq`；跨 aggregate 不用 timestamp、ULID 或 device clock 构造伪全序。
- `ReplicationBatch` 必须 content-addressed，绑定 peer/grant、aggregate、from/to cursor、authority epoch、redaction profile 和连续 event envelopes。Replica 只能写独立可丢弃/可重建的 read-only projection，不能把 redacted batch 变成权威 event，也不能实现 `EventStore::append`。
- batch apply、projection update 和 replica cursor CAS 必须原子。duplicate 同语义幂等；同 id 异语义、gap、reorder、tamper、stale epoch 或 stale expected cursor 均零推进。M2 `SyncWriteBatch::into_authoritative_write` 不得用于 M4 federation replica 路径。
- `ReplicationCheckpointAdvanced` 只在 authority 接受 authenticated ack 后追加，记录已确认的 `(peer, aggregate, from, to, digest, redaction, epoch)`；checkpoint 控制事件默认不回送到其自身 batch，避免递归游标。ack、retention receipt 与 device signal 的 DTO 自报 peer/identity 不构成认证；Gateway 必须先把 TLS 观测到的 transport identity 绑定成不可由 payload 伪造的 peer channel context，Harness 再重查 active grant/role/epoch/peer binding 后才可接受。
- peer revoke 后立即停止新 general session、lease、export 和 approval relay。已复制数据的删除是 retention 请求：只有匹配已 provision 的 Replica transport identity、且被限制为 retention-receipt-only 的 authenticated ingress 通过 digest/epoch/request lineage 验证后才显示 `verified`；该窄入口不恢复 revoked peer 的 session/role。离线、timeout、payload 自报 identity 或 revoke 本身只能显示 `requested/unknown`，不得宣称远端已擦除。
- owner approval/cancel 需要两个独立证明：Gateway 从 authenticated peer channel 绑定 OwnerClient peer/session，另行认证 configured owner principal；二者都通过后才把 plan digest/nonce/expiry/one-shot command 提交 authority。单个可构造的 auth DTO、push 点击、设备解锁、peer role 或缓存 approval 都不能直接授权。
- owner-control nonce、device signal identity、durable checkpoint/handoff、retention state 与已经 claim 的 remote dispatch recovery record 必须由 authority Store companion ledger 持久化，并与其 authority event/lease/artifact lineage 绑定。authority 重启后重复 command/signal 仍须 fail closed，已尝试 dispatch 只能 probe/取原 receipt；Harness 内存 map 不能充当这些状态的唯一事实来源。连接健康和 placement freshness 是可重建观察，不在此持久化要求内。
- authority 在 remote approval 尚未消费、lease 尚未建立时重启，不得从 event replay 猜测恢复 permission/governance runtime。旧 approval command 必须 fail closed 且 one-shot nonce 仍被消费；继续动作必须建立新 run、新 plan digest 与新 approval。该边界不能产生 driver call，也不能被称为远端恢复成功。

### 26.5 Placement、handoff 与全局调度

- placement 先过滤 active grant/role/scope、policy、schema compatibility、health freshness、CapabilityEvidence 与 FailureEvidence，再在已授权集合内排序。M3 strategy、ResourceGraph score 或设备健康只能选择，不能授权；无合格 executor 时 ask/local fallback/stop，不能自动注册或静默换机执行。
- 长任务只能在 `VerificationFinished{Pass}` 的 durable checkpoint 之间 handoff。每段都是新的 Harness run，重新绑定 policy/tool/model/evolution/federation snapshots、budget、approval 和 lease；进行中的外部 action、模型隐状态或未验证输出不能迁移。
- owner device、executor、replica 或网络 tick 只能向 authority 提交 signal。authority scheduler 是 intention claim、foreground priority、AttentionBudget、cancel/revoke 和 budget 的唯一裁决者；重复/离线 signal 不得创建第二 run。
- federation snapshot 或 active strategy 改变只影响后续 run/segment。安全事件可以取消、fence 或等待 owner，但不得 mid-run 热换 peer、strategy、permission 或审批。

### 26.6 M4 仍不允许

- 多租户/多组织 identity、公开匿名 control plane、peer 自注册、TOFU、自动发现即授权或无限 TTL grant。
- 多主 EventStore、CRDT/gossip、peer-to-peer 转发、replica promotion 为 writer、跨 owner 数据合并或多数票治理。
- 远端 Agent 大脑、自主 agent team/spawn network、远端稳定记忆/策略、worker 自选模型/工具/operation 或独立长期目标。
- 自动扩大 permission/trust/grant/envelope、standing L5、分区期间无限 L4/L5 或以历史成功/设备声誉替代 owner approval。
- marketplace、远程自动安装、自研密码学、通用容器云、弹性集群、在线改源码/tests/verifier/fixed identity 或自动发布。

## 27. 受治理能力生态

M5 允许 forme 从真实 catalog 获取并安装能力 package，但不允许生态参与者分享行动主权。canonical 拓扑固定为：**catalog 提供不可信候选；publisher 提供可认证来源；authority 完成 admission 与 lifecycle 决策；现有 registry 暴露贡献；Harness 仍决定每次真实使用。**

### 27.1 四类事实不合并

| 事实 | canonical 对象 | 只回答什么 | 永远不代表什么 |
|---|---|---|---|
| 来源授权 | `CapabilityPublisherGrant` | owner 是否预置了某 public-key digest 在某 kind/scope/TTL 下签名 release。 | package 安全、publisher trusted、permission、安装批准。 |
| 内容准入 | `CapabilityPackageAdmission` | exact release 是否通过当前 schema/digest/signature/dependency/SBOM/license/secret/path/risk policy。 | 已安装、已启用、可执行、可自治。 |
| 生命周期 | `CapabilityPackageStateChange` / projection | 某 admitted release 当前是否 installed/enabled/disabled/revoked。 | package 贡献绕过 registry/policy，或对外动作已获批。 |
| 分发结果 | `CapabilityPackageDistributionReceipt` | exact package bytes 是否在绑定 peer/epoch/lease/fence 下被观察为已落到目标。 | peer 可自装、远端内容可信、authority verification 已通过。 |

签名只证明持有对应 private key 的主体签过 exact digest；它不能证明内容无害、publisher 值得信任或 owner 同意安装。Admission 只是一组 hard checks 的结果；它不能产生 trust、permission、DelegationGrant、AutonomyEnvelope 或 active toolset。

### 27.2 Package 与 catalog 安全

- `CapabilityPackageManifest` 是 versioned、声明式、封闭对象，至少绑定 package/release/version/kind/scope/contributions/permissions、exact dependency closure、SBOM digest、license expression、body digest 和 publisher。
- release identity = `(package, version, package_digest)`；同 identity 同语义可幂等，同 identity 异语义必须冲突。`latest`、catalog tag、下载 URL、文件时间或评分不是 identity。
- M5 payload 只允许有界 UTF-8 资源和结构化贡献。native/WASM/container/install script/build-from-source 不在 M5；package 不能把代码藏在 hook/config/agent profile 中获得执行。
- catalog index、README、description、ranking、bundle bytes、skill body、MCP metadata 和 agent profile text 都是 `UntrustedData`。认证 transport 或 valid signature 不改变 content trust；内容不得成为 owner command、policy、rubric、graph edge、stable memory 或 active strategy。
- relative resource path 必须 normalize 后仍在 package root；absolute path、drive/UNC、`..` escape、symlink indirection、duplicate/case-collision 和 extra undeclared file fail closed。
- bundle/event/log/trace/artifact 不得包含 credential、private key、SecretRef identifier、owner private path 或 private endpoint。Package 需要 credential 时只能声明 capability slot，后续真实动作仍由本地 SecretRef resolver 在 execution boundary 解析。

### 27.3 Publisher 与 admission

- publisher 必须由 authenticated owner provision versioned `CapabilityPublisherGrant`，绑定 public-key digest、allowed kind/scope、TTL、version 和 revoke state。域名、registry account、TLS certificate、package 声明或 TOFU 不能生成 grant。
- signature verification使用成熟 Ed25519 库和 project-owned canonical digest material；forme 不自研签名算法、key exchange 或 canonical JSON 标准。Private key 只用于 fixture/publisher side，runtime authority 只持 public key。
- admission 顺序固定为：schema/size/closed set/path -> content digest -> publisher grant/TTL/scope -> signature -> exact dependency DAG -> SBOM/license -> secret/risk/permission/network/hook declarations -> managed policy。任何 hard check `Fail|Unverifiable` 都不得产生 admitted fact。
- dependency closure exact、content-addressed、有界且无环。每个 dependency 必须先独立 admitted；根 package 不能借 dependency 扩大 scope/permission。禁止在线递归 solver、隐式 mirror fallback 和 dependency confusion。
- publisher/package revoke 只追加事实，不删除历史。Revoke 立即阻止新 admission/enable/distribution，并使相关 active contribution 从后续 toolset 消失；进行中的真实 action 仍按 Harness cancel/fence/unknown 规则处置。

### 27.4 Lifecycle、approval 与 registry

canonical lifecycle 为：

```text
Quarantined -> Admitted -> Installed -> Enabled -> Disabled
                                            \-> Revoked
Admitted/Installed/Disabled -----------------> Revoked
```

- install、enable、update、rollback、distribute 均形成 immutable `CapabilityInstallPlan`，绑定 exact release digest、admission、contributions、scope、policy snapshot、expected ecosystem version、rollback boundary 和 plan digest。
- 上述 mutation 默认至少 L3，必须是 authenticated owner 的 plan-bound、nonce/expiry-bound one-shot approval。Publisher grant、managed allowlist、历史成功或模型 confidence 不能替代该批准。
- ecosystem state 由 authority EventStore companion aggregate 使用 expected-version/CAS 写入；event append、aggregate version、active release 和 registry projection 必须同一事务可重建。进程内 map 不能充当唯一事实源。
- update 是新 immutable release 的 staged admission/install 后原子 active-ref switch；任何 contribution validation 失败都保留 previous known-good。Rollback 是新的 state change，只影响后续 registry resolution，不删除 release，也不撤销此前真实副作用。
- enabled contribution 只能经现有 Provider/Capability/Plugin/Skill/MCP/SubagentProfile registry 进入 toolset。Package 不获得私有 backend、loop 或 event writer；hook 变换后继续 final execution-time recheck。
- agent package 只贡献无认知 role/profile/template。它没有 memory、AgentSelfModel、CognitiveMap、policy owner、stable write 或自主 spawn 权；child run 仍由唯一 Harness 创建并受 parent envelope。

### 27.5 联邦分发

- M5 distribution 复用 §26 的 active Executor grant、authority epoch、immutable plan、one-shot lease/fence、unknown recovery 和 authority verification。Replica/OwnerClient role 本身不能接收 install。
- authority 只分发 exact admitted public package bytes/digest/signature；不下发 credential、private key、SecretRef id、owner path、policy secret 或 active permission。Peer 必须重验 digest/signature/admission-compatible schema 和本地 executor policy。
- peer receipt 仍是 UntrustedData。Transport success、HTTP status 或 worker self-report 不能成为 `Enabled`；authority 验证 receipt binding 与独立 ground truth 后才追加 distribution fact。
- dispatch 后结果不明时同 lease 只允许取原 receipt或只读 probe；不得再次 install、换 peer 盲重放或以 timeout 推断未发生。新尝试需要新 intent/plan/approval/lease，并先处理重复副作用风险。
- package/publisher revoke 或 authority epoch change fence 尚未开始的 distribution；已开始而无可信终态保持 unknown，不得静默视为已删除或已禁用。

### 27.6 M5 仍不允许

- 多租户/多组织 marketplace、支付/结算、公开 publisher self-service、匿名公网 control plane 或跨 owner trust 合并。
- 多主 package state、CRDT/gossip、peer-to-peer 自动扩散、Replica promotion 或 catalog 成为事实源。
- native library、WASM runtime、container image、安装脚本、在线构建、任意 executable payload 或 package 自修改 source/tests/verifier。
- 自动 install/update/publish、silent dependency repair、auto trust/permission/grant/envelope、standing L5 或分区期间放宽治理。
- 独立 Agent 大脑、自主 team/spawn network、package 自带稳定 memory/strategy/policy owner。
- 自研密码学、通用 dependency solver、publisher 声誉/推荐系统、硬件 attestation 或无限 executor fleet。

## 28. V1 核心大脑运行闭合

V1 核心大脑闭合不是 M6，也不是第四内核。它把 M0–M5 已经实现的模块装进同一个默认、持久、可恢复的 forme 大脑运行时，使正式 CLI、Gateway、scheduler、长期 workspace 与多 Agent route 共享 owner identity、authority EventStore、memory/cognition、governance、context、model/capability registry 和结果回流链。

### 28.1 唯一 composition root

- 正式产品只有一个 `ProductionRuntimeBuilder -> ProductionRuntime -> ReactiveHarness` composition path。`ReactiveHarness::from_environment*`、Gateway daemon、CLI embedded mode 和 scheduler 只能委托该路径，不能各自拼装不同强度的 Harness。
- 同一个 store 只绑定一个 configured owner authority 和一个 active writer。owner identity、store identity 与 memory/cognition/intention/charter/data-lifecycle aggregate refs 跨重启稳定；不同 owner 打开同一大脑必须 fail closed。
- composition manifest 记录安全 digest/ref 与 service ready/disabled/blocking 状态，不包含 secret value、可利用的 SecretRef identifier、私有 endpoint 或 owner 本机路径。

### 28.2 默认大脑不是空上下文

- 正式 context 必须按 scope/预算/信任装配 rules、session history、memory summary/recall、owner `AgentWorkspace` slice、active cognition refs、`WorkspaceCharter`、skill metadata/body selection 与 tool schema。`ContextSources::empty()` 只允许测试或明确无模型的内部路径，不能进入正式 brain model call。
- owner/system rule 才能成为 instruction。外部内容、memory 中的外部原文、模型/工具输出、catalog/MCP metadata 继续是 data 或 UntrustedData；被召回、总结、重复出现或图上汇聚都不改变其权威级别。
- `AgentWorkspace` 是 owner 大脑的全局注意投影；项目 `Workspace` 是长期项目边界。二者不同，且都不能成为第二大脑。

### 28.3 三相认知运行

一次 cognitive tick 固定为有序三相：Observation/Proactivity -> Reflection/Candidate -> Memory maintenance。三相共享一个固定 snapshot/event range，不是三个并行 loop。foreground 优先；无有意义变化不调用模型；后台只追加 proposal/candidate/maintenance fact，不能直接改 stable cognition、前台 session 或执行真实动作。

`AskToLearn` 只来自会影响当前 goal/质量/风险判断的真实认知缺口，默认 L1 + Hitchhike，并受 AttentionBudget、ValueGate、CompetenceGate 与 policy。owner 回答只进入 evidence/candidate；拒绝抑制 origin，延后进入 intention。

### 28.4 激活扩散与模型潮线

- 激活扩散必须 seed-driven、有界、确定性、可 replay；无 seed 返回空，不扫全图。边权、静息激活、巩固、衰减与修剪只通过 `MemoryMaintenanceApplied` 变化，Untrusted 内容不得直接改变图结构/权重。
- 同一候选集必须保留“直接交给模型做相关性选择”的简单基线。扩散若持续不能在质量/可靠性/延迟/成本和 hard invariants 上胜出，可被退役为可替换实现；不得为了维护设计而隐藏负结果。
- 模型能力增强可减少相关性/推理脚手架，但跨时间存在、owner 身份、governance、event lineage、ground truth 与可携带认知仍由 Agent 负责。

### 28.5 WorkspaceCharter、长期监管与编排

- `WorkspaceCharter` 是 workspace-scoped、versioned、event-sourced、expected-version/CAS 的声明式事实；context、coordination 和长期监管读取同一投影。它不直接修改 CognitiveMap。
- owner 大脑长期照看 workspace：goal/watch/review/re-coordinate 随新 evidence、失败、deadline 和 diff 产生新 DecisionTrace/route；不得创建后 fire-and-forget，也不得 mid-run 热换。
- 内置八套 repository-owned 种子模式；六维适用签名 + case anchors + fitness 先匹配，全部不过阈值时生成 low-confidence custom route 并加强 evaluator/checkpoint。模式不是主脑，Tool Use/Memory 不是平级编排模式。
- route 的成功、失败、取消、unknown、冲突和 verification 都回流 episode/evidence/candidate。`sediment` 不能直接激活策略，仍遵守 §25。

### 28.6 多模型真实性

- 多模型选择必须在每次 call attempt 前完成，并把一个具体 provider 绑定给该 attempt。`ModelCallStarted/Finished` 中的 profile 必须等于真实调用的 provider profile。
- 禁止内部 fallback wrapper 永远自报 primary profile。primary 失败后先结束并记录该 attempt，再按有界 policy 建立下一 attempt；结果不明、高风险/L5、能力不兼容或链耗尽时停止/询问。
- fallback 不能改变 scope、toolset、permission、approval、DoneContract、AutonomyEnvelope 或治理强度。强模型只影响脚手架和验证冗余，不授权。

### 28.7 数据闭环

- dashboard 只从权威事件投影，所有比率同时给 numerator/denominator/unverifiable；删除投影后可 replay，不保存模型隐藏思维链。
- 备份必须是 SQLite 一致性快照，带 manifest/checksum/event continuity/projection replay 对账；restore 只到新路径并由 owner 显式切换，不能复制热文件后宣称成功。
- sensitive content 在进入不可变 event/object 链之前必须外置为成熟 AEAD 的应用层 envelope 或安全内容引用；key 只经本地 resolver，明文/key/SecretRef id 不进 event/log/trace/artifact。事件中只可保留安全 envelope/ref、digest 和 replay 所需元数据。
- owner delete/retention 清理本地原文、FTS 与派生索引并 crypto-shred 对应 key，留下不含原文的 `DataLifecycleApplied` tombstone。append-only 审计链不删除；无法验证的远端删除必须标 Requested/Unknown。历史链中已经存在的可恢复明文不能靠删除 projection 解决，必须先完成显式迁移/清理；否则数据保护与真实删除只能判定 FAIL/UNVERIFIABLE。

### 28.8 协议与完成门

- 前 97 EventKinds 是 M5 strict prefix；V1 闭合只允许末尾追加 `WorkspaceCharterChanged` 与 `DataLifecycleApplied`，总数 99。若实现发现需要第三个新增事件，必须回 requirements/architecture 经 owner 确认。
- C1–C24 不是 S100+，也不是新里程碑编号。S1–S99 永久回归继续有效。
- 只有正式入口、真实 SQLite 重启、真实 backend、真实/可控多模型选择、六类价值 golden、99-kind/18-crate/strict-clippy/compliance/typed-artifact/release gate 全绿，才可声明“V1 核心大脑运行闭合”。类型存在、测试 builder 可拼装或结构测试全绿都不等于产品闭合。

