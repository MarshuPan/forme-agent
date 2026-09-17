# forme 底座技术方案(Foundation Architecture)

本文是 forme 的正式技术方案。它把设计层的结论映射成 Rust 工程:crate/模块边界、协议对象、数据流、状态机、存储和治理 enforcement 点。

## 0. 本文定位

- **它是什么**:工程桥 + 团队编码契约。定义模块边界、协议、M0 数据流,精确到能让团队并行开工。
- **输入**:`../requirements/01-04`(做什么、M0 范围、验收)、`canonical-contract.md` §1–28(唯一口径/不变量)、`02-proactive-cognitive-kernel.md`(内核怎么转)。同名对象与判断以 `canonical-contract.md` 为准；M1-M5 与 V1 核心大脑闭合只在本篇冻结面上 additive 扩展。
- **姿态(为会改而建)**:**承重墙具体且冻结;认知层 trait 化 + schema 版本化、可替换**。认知本体论会随实现迭代,不焊死。
- **范围**:M0 主体 scoped 到 `requirements/03` 的完整闭环；M1–M5 与 V1 核心大脑闭合只做 additive 扩展，继续保持同一 18-crate 图和冻结承重墙。
- **不是 PRD**:不拆任务/测试/fixture(那是 `prd/01`);不复制第三方代码/结构/命名(`canonical-contract.md` §12)。
- **状态**:骨架(§1–3)+ 承重墙/认知层/治理/存储/M0 边界(§4–8)+ 详篇(§2.1.1 完整事件分类、§4.7 承重墙 trait 签名、§5.6 认知层 trait 签名)均完成;接口契约齐,团队可据此开 crate。协议 struct 字段抠到实现级细节留 `prd/01` 或实现期。

## 1. Rust workspace 与 crate 边界

**原则**:

- **依赖单向向下**:低层 crate 不依赖高层。`protocol` 在最底,无依赖,人人依赖它。
- **承重墙具体,认知层 trait 化**:`harness/loop/store/policy/approval/execution/tools` 定死;`memory/cognition/coordination` 的语义实现放 trait 后面,可替换。
- **`harness` 是薄治理容器**:只管生命周期 + 在咽喉点 enforce + 持有事件日志 + 路由提议;**不含认知逻辑**(认知逻辑在 `cognition/coordination`)。
- **认知集中、执行分布**:只有一个 `harness` 拥有认知子系统;子代理是 `harness` 派出的 child run,无独立认知(canonical §13)。

| 层 | crate | 职责 | 主要依赖 |
|---|---|---|---|
| **L0 基础** | `protocol` | 共享类型:RunRequest/Event/RunResult/ToolCall/ActionIntent/ApprovalRequest/…;schema_version。 | —— |
| | `store` | append-only event log(单写者)、SQLite/FTS、projection、schema 版本/迁移。 | protocol |
| | `config` | typed config、precedence、secrets 引用、ConfigDoctor。 | protocol |
| | `models` | ModelProvider/ModelProfile/ModelRoutingPolicy/ToolCompatibility。 | protocol |
| **L1 治理/执行(承重墙)** | `policy` | PermissionProfile、PolicyEngine、ApprovalPolicy、DelegationGrant、AutonomyEnvelope、能力门 enforce 入口。 | protocol |
| | `approval` | ApprovalBroker、ApprovalRequest/Grant、多入口审批通道。 | protocol, policy |
| | `execution` | ActionIntent→ExecutionPlan→ActionBackend(Shell/File/Mcp)、ExecutionBackendRegistry、output budget/timeout/cancel。 | protocol, policy |
| | `capabilities` | CapabilityRegistry、ToolRegistry、ToolsetResolver、SkillRegistry、McpRegistry、PluginRegistry、HookRegistry、CapabilityEvidence。 | protocol, policy(接入生命周期经 prd/05;MCP 调用作为 ActionIntent 交 execution,不依赖它) |
| | `context` | 上下文装配:rules/history/memory-summary/skills-metadata/tool-schema/context-slice。 | protocol, store, memory |
| | `eval` | Verifier、trace export、FailureEvidence、FailureTaxonomy、failure digest。 | protocol, store |
| | `memory` | Raw Storage 接口、记忆图 + projection、记忆链候选/稳定、属性存储(UserModelAttribute/AgentSelfAttribute)。**事实源 owner**(canonical §5)。 | protocol, store |
| **L2 运行内核(承重墙)** | `loop` | LoopEngine:model→tool proposal→policy/approval→execute→observe→continue/stop;停止条件;turn 状态机。 | protocol, capabilities, context, policy, models |
| | `harness` | AgentHarness:bind run/session/turn、驱动 loop、咽喉点 enforce、持久化事件、调用协调相、spawn 子代理(Orchestrator)、托管背景 tick、路由 proposal/candidate。 | protocol, store, policy, approval, loop, coordination, cognition, context, models, capabilities, execution, eval |
| **L3 认知层(trait 化、可替换)** | `cognition` | 认知语义 owner(canonical §5):CognitiveMap、EvolutionGovernor(A3)、Proactivity(背景 tick + 念头 + 激活扩散)、CompetenceGate、UserModel/AgentSelfModel 语义。**trait 接口 + 可替换实现**。 | protocol, memory, store |
| | `coordination` | CoordinationReasoner:GoalFrame/SituationModel/ResourcePlan/DoneContract/AutonomyEnvelope/DecisionTrace;编排模式库 + 适用签名 + Orchestrator 拓扑产出。**trait 化**。 | protocol, cognition, capabilities |
| **L4 对外/入口** | `communication` | Communication Fabric:CommunicationEvent/Session、ChannelAdapter、对外膜(DisclosurePolicy/TerminationPolicy/ExternalCommunicationGrant)。 | protocol, policy, harness |
| | `gateway` | Local Gateway/App Server:提交 run、广播 event、共享 approval protocol。 | protocol, harness, communication |
| | `cli` | 人机入口,提交 RunRequest / 消费 Event,不拥有 loop。 | protocol, gateway |

注:`Orchestrator`(多 Agent 编排执行)是 `harness` 的子模块(它 spawn child run);编排**拓扑决策**在 `coordination`,**执行**在 `harness`。不单独成 crate,避免与 harness 形成环。

## 2. 核心协议对象(`protocol` crate)

协议是所有 crate 的共同契约。CLI/Gateway/UI 只看协议,不读内部对象。所有协议对象带 `schema_version`(canonical §9,支撑 replay)。

### 2.1 事件模型(事实源)

- **`Event`** = append-only 事实单位:`{ event_id, run_id, turn_id, type, payload, schema_version, ts, provenance }`。event log 是唯一事实源,记忆图/session state/投影都从它派生(canonical §7 单写者)。
- M0 定义类型与 payload 边界;类型一旦定,后续不破坏 protocol。下表是 M0 完整事件分类——它是审计、replay、可观测与 prd §7 逐场景事件序列断言的契约。

#### 2.1.1 完整事件分类(M0 基线 + 里程碑 additive)

`payload` 列只列契约关键字段;所有事件隐含 `{event_id, run_id, stream_seq, turn_id?, schema_version, ts, provenance}`(`stream_seq` = run 内单调序,排序权威)。"服务"列指向 canonical 不变量(§)与验收场景(S)。

**A. Run / Session 生命周期**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `RunAccepted` | source, session_ref, input_ref | §1, S1 |
| `SessionBound` | policy_profile, model_profile, toolset_ref, workspace, effect_mode?/evolution_snapshot?(M3-A additive), federation_snapshot?(M4-A additive) | §1/§25/§26, S1/S53-S84 |
| `RunComplete` / `RunAborted` / `RunFailed` / `RunLimited` | stop_reason, result_ref | §1, S2 |
| `RunWaiting` / `RunResumed` | wait_reason, resume_ref(ApprovalGrant/ResumeState) | §1, S2 |

**B. Turn / Context**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `TurnStarted` / `TurnComplete` | turn_index | §1 |
| `ContextBuildStarted` / `ContextBuildFinished` | sources(rules/history/memory_summary/skills_meta/tool_schema), slice_refs | S4, Context/Memory 行 |
| `CompactionStarted` / `CompactionFinished` | lineage_ref, preserved_refs | §4.6 |

**C. Model**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `ModelCallStarted` / `ModelCallDelta` / `ModelCallFinished` | model_profile, usage, finish_reason | §1, 持久化行 |
| `OutputClassified` | kind(final/tool/handoff) | §1 |

**D. Tool / Policy / Approval / Handoff**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `ToolCallProposed` | call_id, tool, args | §1, S2 |
| `ToolPolicyEvaluated` | decision(allow/ask/deny), rule_source, reason | §1, S2 |
| `ApprovalRequested` | approval_id, action_summary, risk, scope, rollback_boundary, expires_at, choices | D12, S2 |
| `ApprovalResolved` | outcome(granted/denied/expired), grant_ref | D12, S2 |
| `HandoffRequested` / `HandoffResolved` | target, reason | §4.1 |

**E. Action / Execution**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `ActionPlanned` | intent_id, backend, expected_effect, source, remote_placement?(M4-A additive) | §6/§26, S2/S71 |
| `ActionStarted` / `ActionOutputDelta` | backend, scope, remote_lease?(M4-A additive) | 执行行, S71-S73 |
| `ActionCompleted` / `ActionFailed` | result_ref / failure_ref, remote_receipt?(M4-A additive) | 执行行, S10/S71-S74 |
| `ActionDenied` / `ActionCancelled` | reason | §1, S2 |
| `ActionOutcomeUnknown` | intent_id, probe_hint, remote_lease?(M4-A additive) | 恢复扫描(prd/03), S2/S72 |

**F. Verification**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `VerificationStarted` / `VerificationFinished` | verifier_kind, outcome(pass/fail), against(DoneContract_ref) | D14, S10, Verification 行 |

**G. Failure**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `FailureEvidenceRecorded` | class, impact, scope, related_refs(trace/capability/trust/map), suggested_fix | D9, S10 |
| `FailureDigestUpdated` | digest_ref, members | D9, S10 |

**H. Candidate / Evolution / 撤销**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `CandidateCreated` | target, evidence_refs, confidence, provenance, target_tier, capability_update?(M2-C additive), strategy_candidate?(M3-A additive) | §4/§25, S8/S9/S50/S56 |
| `CandidateConflictDetected` | conflict_with, kind | §8 |
| `CandidatePromoted` / `CandidateRejected` / `CandidateDowngraded` / `CandidateDecayed` | by(auto/user), reason | §4, S8/S9 |
| `RetractionEvent` / `RevocationEvent` | target_object, evidence_lineage | §8, S20 |
| `ReevaluationTaskCreated` | derived_refs, trigger | §8, S20 |

**I. Cognitive / Proactive**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `ObservationRecorded` | source, scope, grant_ref | D6, S7 |
| `OpportunityDetected` | seed, activation_shape(optional; Commitment 为 `None`) | D6, S7/S32 |
| `ValueGateEvaluated` | decision, reason | D6, S7 |
| `CompetenceGateEvaluated` | scope, risk, max_level, reads(MapConfidence/AgentSelfModel/CapabilityEvidence/Trust/Failure/VerificationEvidence) | §6, S18/S32 |
| `ImpulseRaised` | source(gap/change/tension/association/pressure/commitment), reach | §22, S7 |
| `ReflectionProduced` | inputs(trace/feedback/failure), candidate_refs | S9 |
| `ProactiveProposalEmitted` | proposal_kind, level, guard(ValueGate∧CompetenceGate∧Policy), delivery, attention_cost | D6, S7/S18/S32 |
| `ProactiveProposalResolved` | outcome(adopt/reject/defer), feedback | S7 |
| `ProspectiveIntentionCreated` / `ProspectiveIntentionResolved` | source(commitment/deferred/self), trigger(at/on_event/on_condition), optional schedule(session/envelope/budget), goal_frame?(M2-C additive) / outcome(fired/done/expired/cancelled) | §22, S7/S29-S31/S49 |

**J. Coordination / Orchestration / Subagent**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `GoalFramed` | goal_frame, long_term?(M2-C additive) | D7, S6/S49 |
| `ResourcePlanned` / `DoneContractSet` / `AutonomyEnvelopeSet` | plan / contract / envelope | D7/D10, S6 |
| `DecisionTraceRecorded` | refs(map/user/self/trust/failure), rationale, workspace_snapshot, resource_graph_snapshot?(M2-C additive), evolution_snapshot?(M3-A additive), federation_snapshot?(M4-A additive) | D7/§25/§26, S6/S48/S58-S83 |
| `OrchestrationRouteCreated` | pattern_ref, route(DAG), goal_frame?/checkpoint?(M2-C additive) | §13, S6/S49 |
| `SubagentSpawned` | child_run, toolset/model/permission/budget 限制 | S11 |
| `SubagentResultReturned` | summary/result, status | S11 |

**K. Capability**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `CapabilityIndexed` / `ToolsetResolved` | sources, toolset_ref | CapabilityRegistry 行 |
| `McpDiscovered` / `McpCallEvent` | server, tools/resources, timeout/error_class | D3, S3 |
| `SkillMetadataExposed` / `SkillBodyLoaded` | skill, scope/version/trust, trigger | D4, S4 |
| `PluginContributionRegistered` / `PluginToggled` | manifest, contributions, enabled, trust, managed policy/snapshot refs?(M2-C additive) | D5, S5/S51 |
| `CapabilityEvidenceRecorded` | capability, outcome | D9 |

**L. Communication**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `CommunicationEventReceived` | modality, carrier, channel_adapter, participant, scope | D16, S12 |
| `CommunicationSessionOpened` / `CommunicationSessionTerminated` | purpose, ttl, budget, termination_reason | D16, S13/S17 |
| `ExternalCommunicationGranted` | purpose, disclosure, ttl, budget, transcript_policy | D16, S13 |
| `DisclosurePolicyApplied` | request, outcome(answer/blur/approve/refuse), representation, binding?(session/participant/purpose/content/policy inputs; M2-B additive) | D16, S14/S15/S45 |
| `CommunicationProposalEmitted` | proposal, level | D16 |

**M. Memory**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `MemoryNodeAppended` / `MemoryEdgeAppended` | node/edge(type/tier/confidence/scope/weight) | §14, Context/Memory 行 |
| `MemoryMaintenanceApplied` | deltas_ref(边权/巩固/修剪,批量归并) | §14/§22, replay 完整性 |
| `UserAttributeCandidateCreated` | attribute, evidence, confidence, first/last_at, stability, scope | D11, S8 |
| `ImportedHistoricalEvidenceRecorded` | source, low_weight=true | D11, S8 |
| `CognitiveMapUpdateProposed` | frame/quality/blindspot/resource, confidence(low) | D8, S9 |

**N. Config / Compliance**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `ConfigDoctorReport` | checks(provider/credential/capability/mcp/plugin/fs·shell/scheduler/notification/federation), findings | D13/D93, ConfigDoctor/S84 |
| `ComplianceCheckResult` | scope(upstream/license/copy/dependency/notice/release-tree/secret), outcome, blocking | D15/D73, S22/S69 |

**O. M3 Evolution Control（末尾 additive，保持前 86 项 wire 顺序）**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `EvolutionEvaluationRecorded` | evaluation_ref, baseline/candidate versions, verdict, hard-invariant refs, ground-truth refs | §25, S53-S69 |
| `StrategyActivated` | domain/scope, from/to version, evaluation/promotion/owner refs, impact, expected version, active snapshot | §25, S56-S68 |
| `StrategyRolledBack` | failed/restored version, trigger refs, in-flight disposition, external_effects_reverted=false, active snapshot | §25, S57/S63/S68 |

**P. M4 Federation Control（末尾 additive，保持前 89 项 wire 顺序）**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `FederatedPeerRegistered` | peer grant/version, roles, scopes, transport identity digest, ttl, authority epoch, committed federation version | §26, S70/S77 |
| `FederatedPeerRevoked` | peer, revoked grant/version, new authority epoch, in-flight disposition, committed federation version | §26, S70/S77-S78 |
| `RemoteExecutionLeaseChanged` | lease, state(reserved/acquired/released/expired/fenced), executor, grant/version, plan digest, authority epoch, fence token, reason | §26, S71-S74/S77/S79 |
| `ReplicationCheckpointAdvanced` | peer, aggregate, from/to stream_seq, batch digest, redaction profile, authority epoch, committed federation version | §26, S75-S78/S83 |

**Q. M5 Capability Ecosystem（末尾 additive，保持前 93 项 wire 顺序）**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `CapabilityPublisherChanged` | publisher grant/version/key digest/kind/scope/ttl/revoke、committed ecosystem version | §27, S85/S93 |
| `CapabilityPackageAdmitted` | admission(package/release/digest/publisher/policy/check/dependency/SBOM/license refs)、committed ecosystem version | §27, S86-S89 |
| `CapabilityPackageStateChanged` | change(package/release/from/to/install plan/approval/active generation/rollback)、committed ecosystem version | §27, S90-S94 |
| `CapabilityPackageDistributionRecorded` | receipt(package/release/digest/peer/epoch/plan/lease/fence/ground truth/verified)、committed ecosystem version | §27, S95-S98 |

**R. V1 Core Brain Closure（末尾 additive，保持前 97 项 wire 顺序；不是 M6）**

| 事件 | 关键 payload | 服务 |
|---|---|---|
| `WorkspaceCharterChanged` | charter(workspace/version/goals/constraints/prohibitions/done/review cadence/actor/digest)、expected/committed version | §28, C13/C17 |
| `DataLifecycleApplied` | receipt(operation/scope/subject digest/cleaned projections/destroyed key digests/remote disposition/evidence)、expected/committed version | §28, C22-C24 |

### 2.2 生命周期对象

- **`Run`**:一次执行,`{ run_id, source, status, budget, stop_reason, result_ref }`。`source ∈ {user_turn, proactive_job, schedule, subagent, communication, internal, replay(M3-A), simulation(M3-A), owner_control(M4-A)}`。
- **`Session`**:长期容器,绑定 workspace、agent profile、policy profile、model profile、memory scope。
- **`Turn`**:run 内一次 model/tool 交替边界。
- **`RunResult`**:`{ status, stop_reason, outputs, evidence_refs }`。

### 2.3 工具/动作/审批

- **`ToolCall`**:模型 proposal,`{ call_id, tool, args, status, result, approval_ref }`——是 proposal 不是命令(canonical §1)。
- **`ActionIntent`**:`{ intent_id, source, goal, backend_hint, capability_ref, scope, risk_hint, expected_effect, rollback_expectation }`。**`source` + `expected_effect` 是能力门出口 enforce 的 key(canonical §6/§15)。**
- **`ExecutionPlan`**:由 ActionIntent 经 ExecutionPlanner 产出,绑 backend/审批/验证/回滚边界。
- **`ApprovalRequest`/`ApprovalGrant`**:`{ approval_id, action_summary, risk_level, scope, requested_permissions, affected_resources, rollback_boundary, expires_at, choices }`;Gateway-compatible(canonical/requirements D12)。

### 2.4 协调约束对象(`coordination` 产出)

- **`GoalFrame`** / **`SituationModel`** / **`ResourceInventory`**。
- **`ResourcePlan`**(选定资源 + 去找任务)、**`DoneContract`**(完成标准/停止条件)、**`AutonomyEnvelope`**(scope/capability/action_type/risk_limit/approval_rule/budget/timebox/rollback,canonical §3)、**`DecisionTrace`**(引用 CognitiveMapRef/UserModelAttributeRef/AgentSelfAttributeRef/TrustProfileRef/FailureEvidenceRef…,是可解释主梁)。
- **编排**:`OrchestrationPattern`(模板,=CoordinationSpec/WorkPattern + 6 维适用签名 + fitness)、`ExecutionRoute`(实例 = 子任务 DAG:节点=子任务+角色+资源切片+DoneContract,边=依赖/交接)。

### 2.5 提议/候选对象(认知层 → harness 治理)

- **`ActionProposal` / `CommunicationProposal` / `LearningTask` / `ReflectionTask` / `ProactiveJob` / `DelegationProposal`**:认知层只能提交这些,真实行动回 harness(canonical §1)。`DelegationProposal` = 以 CapabilityEvidence 为据申请扩展 AutonomyEnvelope(高影响,必须用户确认;prd/04 承接为 grant 流程)——放权成长闭环的 Agent 侧入口。
- **`CandidateUpdate`**:UserModel/AgentSelfModel/CognitiveMap/Trust/Coordination 的候选更新,带 evidence/confidence/provenance/conflict/target_tier(canonical §4/§8/§14)。`augmentation_strategy` 是 proposal 上的决策输出字段,不是持久对象(canonical §5)。

### 2.6 沟通对象

- **`CommunicationEvent`**:`{ modality, carrier, channel_adapter, participant, scope, session_ref }`(modality=text/voice/image/video;M0 只跑 text)。
- **`CommunicationSession`**:`{ participant, purpose, modality_set, disclosure_policy, termination_policy, autonomy_envelope, ttl, budget, transcript_policy }`(对外膜,canonical §10/§13)。

## 3. M0 完整闭环数据流

requirements/03 的 M0 闭环,落到 crate 与 enforce 点:

```text
[入口] cli / gateway
  -> communication: 归一成 CommunicationEvent(M0=text)
  -> protocol: 转 RunRequest
[绑定] harness: bind Session/PolicyProfile/ModelProfile/Toolset;分配 run_id/turn_id;写 RunAccepted/SessionBound 事件
[协调相] coordination(读 cognition: CognitiveMap/UserModel/AgentSelfModel/Trust/Failure refs)
  -> 产出 GoalFrame/ResourcePlan/DoneContract/AutonomyEnvelope/DecisionTrace
  -> 若需多 Agent:匹配/定制 OrchestrationPattern → ExecutionRoute(DAG)
[执行相] loop(在约束内):model call → ToolCall/ActionIntent proposal
  -> [enforce] capabilities: toolset scope 重查
  -> [enforce] policy: allow/ask/deny + DelegationGrant/AutonomyEnvelope
  -> [enforce] approval: ask 时 ApprovalRequest → 等待/恢复(ResumeState)
  -> [enforce] 能力门(出口,key=source+expected_effect):主动/对外动作不足则降级
  -> execution: ActionBackend(Shell/File/Mcp)执行,结构化结果
  -> eval: Verifier 对照 DoneContract
[持久化] store: 全程写 append-only event(单写者);失败写 FailureEvidence
[认知整合] 多 Agent 结果 → integrator 拼装/检测冲突,真冲突上抛 harness
[后台] cognition 背景 tick(post-turn/idle/diff):激活扩散产念头、反思产 CandidateUpdate、整理/遗忘
  -> 候选经 EvolutionGovernor(A3):证据/冲突/影响 → 自动提升 / 用户确认 / 拒绝
  -> 子代理与后台只读快照、只投候选,不直接写稳定层(canonical §7)
[返回] RunResult / Proposal / CandidateUpdate → 用户反馈 → 回到 harness
```

闭环不变量(§6 详列,这里先标):入口不拥有 loop;协调选、harness enforce;工具调用是 proposal、执行前重查;能力门在出口;候选先于稳定;后台不写稳定层、单写者;失败进 FailureEvidence;认知输入按 §15 防投毒。

## 4. 承重墙子系统

承重墙是 proven、稳定、定死的那层:harness 运行时、协议持久化、治理 enforce、执行。**它不含认知逻辑**(认知在 §5)。

### 4.1 AgentHarness:生命周期与状态机

AgentHarness 是运行容器(canonical §1 Harness-first):把一次 RunRequest 推成可观察、可恢复、可审计的 run。

**API 面**(所有入口只调这些):`submit_run(RunRequest)->run_id`、`stream_events(run_id)`、`wait/cancel(run_id)`、`resume(run_id, ApprovalGrant)`、`drain(session)`。

**LoopEngine 状态机**(harness 驱动,承接 archive/architecture/01):

```text
RunAccepted -> SessionBound -> TurnStarted
  -> ContextBuildStarted/Finished
  -> ModelCallStarted/Delta/Finished -> OutputClassified
  -> [tool] ToolCallProposed -> ToolPolicyEvaluated
        -> (ask) ApprovalRequested -> ApprovalResolved
        -> ActionPlanned -> ActionStarted/OutputDelta -> ActionCompleted -> (result 入上下文,回 TurnStarted 续)
  -> [handoff] HandoffRequested/Resolved
  -> VerificationStarted/Finished
  -> CompactionStarted/Finished
  -> TurnComplete -> (续) | RunComplete | RunAborted | RunFailed | RunLimited | RunWaiting
```

harness 每相职责:**bind**(session/policy/model/toolset)、**enforce**(咽喉点)、**persist**(写 event)、**route**(调协调相、路由 proposal、spawn 子代理)。**只 enforce + 编排,不含认知逻辑。**

**停止条件**:final output、max turns、token/cost/wall-time 限、用户取消、审批拒绝且无法继续、provider/tool 重试耗尽、上下文超限且压缩失败、handoff 无目标/循环、verifier 失败不可修、进入 HITL 等待。

### 4.2 Run / Session / Turn

- 对象见 §2.2。
- **并发(canonical §7)**:同 session 的 run 进 per-session 串行队列;event log 单写者;子代理 child run 只读快照、只回 summary/result、不写父 session 稳定态。
- **lineage**:run→session;subagent child-run→parent run(Orchestrator 记 DAG 边)。

### 4.3 Event log + store + 投影(`store`)

- **append-only event log = 唯一事实源**,单写者(canonical §7)。
- **投影**从 event 派生:session state、transcript、记忆图(§5)、可检索摘要;投影可重建(replay)。
- **SQLite + FTS**:run/session/turn/tool/approval 索引 + 全文检索。
- **schema 版本/迁移(canonical §9)**:event/object 带 schema_version,读时 upcast;breaking change 留 migration note;replay 记所用 schema/policy/loop/model version。
- **ResumeState**:approval 等待、tool 中断、handoff 的可恢复状态落盘;resume 从 event + ResumeState 重建。

### 4.4 Policy / Approval / Delegation enforce(`policy` / `approval`)

- **PolicyEngine**:allow/ask/deny;合并序 managed→user→workspace→agent→surface→session→skill/tool/plugin requested→hook→final;**deny>ask>allow**;hook 可收紧、不可绕过 managed/user deny。
- **执行前重查**:工具调用是 proposal;执行前重查 toolset scope + 参数 + 路径/网络/命令 + sandbox(canonical §1 Policy-at-execution)。
- **ApprovalBroker**:结构化 ApprovalRequest(§2.3);Gateway-compatible 多入口投递;一次性/会话级/参数模式批准;过期 + scope;结果写 event。
- **Delegation / AutonomyEnvelope enforce**:harness 检查当前动作是否在 DelegationGrant + AutonomyEnvelope 内;**trust 不覆盖 permission**(canonical §1)。

### 4.5 执行 backend(`execution`)

```text
ActionIntent -> ExecutionPlanner -> ExecutionPlan -> [policy / approval / 能力门] -> ActionBackend -> ResultNormalizer -> Event + CapabilityEvidence
```

- **ActionBackend(M0)**:ShellBackend、FileBackend、McpBackend(stdio);ExecutionBackendRegistry。
- **ActionBackend additive**:M1 Notification，M2 Browser/Computer/PTY/AppApi，M4 `RemoteExecutorBackend`。M5 package lifecycle 是 authority control companion，不新增绕过 backend 的任意执行；catalog fetch 复用 AppApi，远端 distribution 复用 Remote。Remote 只包装已批准的 typed inner operation，并继续复用本节冻结的 `ActionBackend`；lease/transport 走 companion trait，不修改原签名。
- output budget / timeout / cancellation;action events(started/output/completed/failed/denied/cancelled)。
- **边界**:shell 不作默认兜底;文件写需 diff/变更摘要;最小授予(least-privilege);可回滚边界按 backend 声明、不泛化承诺。

### 4.6 ResumeState / Compaction 边界

- **ResumeState**:见 4.3——approval/中断/handoff 可恢复。
- **Compaction**:属 session/harness 生命周期,不是普通工具;产 lineage/摘要边界;BeforeCompaction/AfterCompaction hook;**不丢审批/工具结果/关键决策 lineage**。M0 做接口 + 事件,自动压缩策略 M1。

### 4.7 承重墙核心 trait 签名(M0)

签名是 M0 接口契约——形状定死,细节可精化但边界不动。`Result`/各 `*Id`/各 `*Ref` 为 protocol 级类型。

```rust
// harness — 运行容器(入口只调这些)
trait AgentHarness {
    fn submit_run(&self, req: RunRequest) -> Result<RunId>;
    fn stream_events(&self, run: RunId) -> EventStream;
    fn wait(&self, run: RunId) -> Result<RunResult>;
    fn cancel(&self, run: RunId) -> Result<()>;
    fn resume(&self, run: RunId, input: ResumeInput) -> Result<()>;   // 覆盖 approval/工具裁决/handoff(prd/03)
    fn drain(&self, session: SessionId) -> Result<()>;
}

// store — 单写者事件日志 + 投影 + replay
trait EventStore {
    fn append(&self, event: Event) -> Result<EventId>;            // 唯一写入路径
    fn read_run(&self, run: RunId) -> EventCursor;
    fn project<P: Projection>(&self, scope: ProjectionScope) -> Result<P::State>;  // 派生、可重建
    fn replay(&self, run: RunId, at: SchemaSnapshot) -> Result<ReplayReport>;
}

// store — M2-C additive CAS/sync extension; the frozen EventStore shape is unchanged
trait VersionedEventStore: EventStore {
    fn aggregate_version(&self, aggregate: RunId) -> Result<AggregateVersion>;
    fn append_expected(&self, event: Event, expected: AggregateVersion) -> Result<ExpectedAppend>;
    fn apply_sync_batch(&self, batch: SyncWriteBatch) -> Result<SyncApplyReport>;
    fn export_sync_batch(&self, request: SyncExportRequest) -> Result<SyncTransferBatch>;
}

// policy — 执行前重查;合并 deny>ask>allow
trait PolicyEngine {
    fn merge(&self, layers: &[PolicyLayer]) -> EffectivePolicy;
    fn evaluate(&self, ctx: &PolicyContext, intent: &ActionIntent) -> PolicyDecision; // allow/ask/deny
    fn enforce_envelope(&self, grant: &DelegationGrant, env: &AutonomyEnvelope, intent: &ActionIntent) -> EnvelopeDecision; // trust 不覆盖 permission
}

// approval — Gateway-compatible
trait ApprovalBroker {
    fn request(&self, req: ApprovalRequest) -> Result<ApprovalTicket>;
    fn resolve(&self, ticket: ApprovalTicket, grant: ApprovalGrant) -> Result<()>;
    fn pending(&self, scope: ApprovalScope) -> Vec<ApprovalRequest>;
}

// execution — backend(Shell/File/Mcp)
trait ActionBackend {
    fn kind(&self) -> BackendKind;
    fn plan(&self, intent: &ActionIntent) -> Result<ExecutionPlan>;
    fn execute(&self, plan: ExecutionPlan, sink: &EventSink) -> Result<ActionResult>; // 流式写 action 事件
    fn cancel(&self, action: ActionId) -> Result<()>;
}
```

**出口接缝**:harness 在投递 `source∈{proactive,schedule,idle}` 或 `expected_effect=outward` 的 ActionIntent 前,先调 `CompetenceGate::ceiling`(§5.6),不足则降级——这是承重墙调认知 trait 的唯一上行点(canonical §6)。

## 5. 认知层 trait(可替换、schema 版本化)

**姿态**:认知层放 trait 后面——harness/coordination 依赖 trait、不依赖实现。M0 出厂一套简单默认实现;实现可替换/进化而**不动承重墙**。认知对象 schema 版本化(canonical §9)。这是"为会改而建"的落点。

### 5.1 memory(记忆,事实源,store-side)

记忆是事实源 owner(canonical §5/§14),cognition 通过它读。trait:

- `MemoryGraph`:节点/边增查 + 激活操作(seed/spread/decay/surface,见纪要 §3.1)。节点带 resting/current activation、tier、confidence、provenance;边带可学 weight。
- `MemoryProjection`:timeline / 主题·实体索引 / 摘要 / 向量召回(session·每日·长期切片)。
- `CandidateStore`:记忆链候选→验证→稳定生命周期(canonical §14)。

### 5.2 cognition trait(认知语义 owner)

- `CognitiveMapStore`:MapNode/MapEdge/JudgmentFrame/QualityModel/BlindSpotModel + MapConfidence(scope 化)。
- `EvolutionGovernor`(A3):候选进 → 证据/冲突/影响/provenance 闸 → 自动提升 / 用户确认 / 拒绝 / 降级 / 回滚 / 衰减(canonical §4/§8)。**所有稳定认知写入的唯一通道。**
- `ProactivityEngine`:背景 tick(post-turn/idle/diff)→ 激活扩散产念头(五个涌现来源 = 激活形状;第六来源 `Commitment` 确定性触发)→ ValueGate;**只产 proposal/candidate,不动手**。
- `CompetenceGate`:`(scope, risk) -> 允许的最高 InterventionLevel`,读 MapConfidence/AgentSelfModel/CapabilityEvidence/TrustProfile/FailureEvidence(canonical §6)。
- `UserModel` / `AgentSelfModel`:属性存 memory,语义/置信/提升治理在此。

### 5.3 coordination trait

- `CoordinationReasoner`:GoalFrame → 自我盘点(读 AgentSelfModel)→ 看缺口 → 定来源(读 CognitiveMap)→ 产 ResourcePlan/DoneContract/AutonomyEnvelope/DecisionTrace。**产约束,不产分步**(模型在 loop 内规划)。
- `OrchestrationLibrary`:按适用签名匹配编排模式 / 不够则定制 / 有效则经 A3 沉淀;产 ExecutionRoute(DAG)。单 vs 多就是匹配里的一步。

### 5.4 认知层 ↔ 承重墙接缝(canonical §7/§13)

- **核心提议、harness enforce、绝不绕过**:认知 trait 产 proposal/candidate/约束;harness enforce + persist。
- **只读快照、只投候选、不直接写稳定层**:认知层与子代理读快照、emit 候选;稳定写入只经 `EvolutionGovernor`(受 harness 治理)。
- **能力门在出口由 harness 调用**:trait 给上限,harness 在动作/投递出口 enforce(key = `ActionIntent.source` + `expected_effect`)。

### 5.5 M0 默认实现 vs 可替换边界

trait 是契约,M0 实现可以简单:

- `CoordinationReasoner` M0 = 显式规则 / 单次模型调用产约束;
- `CompetenceGate` M0 = 固定阈值(学习调参留后续);
- `ProactivityEngine` M0 = 定时器 tick 地板(激活扩散是 M2+ 升级);
- `CognitiveMapStore`/`EvolutionGovernor` M0 = 候选生成 + 用户确认提升(replay/eval 提升留 M3)。

**实现简单不破契约**——trait 边界一旦定,M2/M3 换强实现不动承重墙。

### 5.6 认知层核心 trait 签名(M0)

```rust
// memory — 事实源:图 + 激活 + 投影 + 候选
trait MemoryGraph {
    fn add_node(&self, node: MemoryNode) -> Result<NodeId>;
    fn add_edge(&self, edge: MemoryEdge) -> Result<EdgeId>;          // weight 可学
    fn seed(&self, seeds: &[NodeId], shape: ActivationShape);        // 五个涌现来源 = 五种 shape
    fn spread(&self, budget: ActivationBudget) -> Vec<Activated>;    // 点燃→扩散→衰减→浮现
    fn query(&self, q: GraphQuery) -> Vec<MemoryNode>;
}
trait MemoryProjection {
    fn timeline(&self, scope: MemoryScope, window: TimeWindow) -> Vec<Episode>;
    fn recall(&self, cue: RecallCue, k: usize) -> Vec<MemoryRef>;
    fn summary(&self, scope: MemoryScope) -> MemorySummary;
}
trait CandidateStore {
    fn create(&self, c: CandidateUpdate) -> Result<CandidateId>;
    fn list(&self, filter: CandidateFilter) -> Vec<CandidateUpdate>;
    fn transition(&self, id: CandidateId, to: CandidateState, by: Actor) -> Result<()>;  // 契约义务:必须发射对应 H 组事件
}

// cognition — 语义 owner
trait CognitiveMapStore {
    fn read(&self, scope: MapScope) -> CognitiveMapView;             // frames/quality/blindspot
    fn confidence(&self, scope: MapScope) -> MapConfidence;
    fn propose_update(&self, p: CognitiveMapUpdateProposal) -> Result<CandidateId>; // 低置信候选
}
trait EvolutionGovernor {                                            // 稳定写入的唯一通道
    fn intake(&self, c: CandidateUpdate) -> GovernanceDecision;      // promote/confirm/reject/downgrade
    fn on_retraction(&self, e: RetractionEvent) -> Vec<ReevaluationTask>;
    fn decay(&self, tick: Tick) -> Vec<CandidateId>;
}
trait ProactivityEngine {
    fn tick(&self, trigger: TickTrigger, snap: &CognitionSnapshot) -> Vec<Impulse>; // post-turn/idle/diff
    fn emit(&self, impulse: Impulse, guard: EmissionGuard) -> Option<Proposal>;     // 只产 proposal,不动手
}
trait CompetenceGate {
    fn ceiling(&self, scope: Scope, risk: Risk, ctx: &CompetenceInputs) -> InterventionLevel; // 读 Map/Self/Capability/Trust/Failure
}

// coordination — 产约束,不产分步
trait CoordinationReasoner {
    fn frame(&self, goal: GoalInput, ctx: &CoordinationContext) -> GoalFrame;
    fn plan(&self, frame: &GoalFrame)
        -> Result<(ResourcePlan, DoneContract, AutonomyEnvelope, DecisionTrace)>;
}
trait OrchestrationLibrary {
    fn match_pattern(&self, sig: ApplicabilitySignature) -> Option<OrchestrationPattern>;
    fn route(&self, pattern: Option<OrchestrationPattern>, frame: &GoalFrame) -> ExecutionRoute; // None = 定制
    fn sediment(&self, route: &ExecutionRoute, outcome: &RouteOutcome) -> Option<CandidateId>;   // 经 A3 沉淀
}
```

**接缝硬约束**(canonical §1/§7):`ProactivityEngine::emit` 只返回 `Proposal`(真实行动回 harness);`EvolutionGovernor::intake` 是稳定认知写入唯一入口;认知 trait 入参用 `*Snapshot`(只读快照),输出用 `Candidate*`/`Proposal`(只投候选)。

## 6. 治理 enforcement 点(每条不变量落在哪)

每条 canonical 不变量都有一个具体 enforce 位置——这让架构**可验证地**守住契约。

| 不变量(canonical) | enforce 在哪 | 怎么 enforce |
|---|---|---|
| Harness-first（§1） | `harness`/`loop` | 真实动作只能经 execution、execution 只在 harness 驱动的 loop 路径里被调;无旁路 |
| Policy-at-execution / 工具是 proposal（§1） | `loop` ToolPolicyEvaluated → `policy` | 执行前重查 toolset/参数/路径/网络/命令/sandbox |
| 能力门出口（§6） | `harness` egress | 对 `source∈{proactive,schedule,idle}` 或 `expected_effect=outward` 的动作过 CompetenceGate 上限,不足降级 |
| Autonomy/Delegation;trust 不覆盖 permission（§1/§3） | `harness`→`policy` | 动作前查 DelegationGrant+AutonomyEnvelope;permission 是硬边界 |
| Candidate-before-promotion（§1/§4） | `cognition` EvolutionGovernor | 稳定认知写入唯一通道;其余只 emit 候选 |
| 单写者/并发（§7） | `store` + `harness` 调度 | event log 单写者;后台只读快照、只投候选;per-session 串行 |
| 撤销/派生失效（§8） | `store` lineage + `cognition` | RetractionEvent → 引用者再评估(失效候选) |
| schema 版本/replay（§9） | `store` | schema_version + 读时 upcast;replay 记 versions |
| 认知输入防投毒 + Loyalty（§15） | `capabilities`/`context`/`memory` 摄入 + 固定内核 | 来源信任分级、内容≠指令、quarantine;忠诚不可改向 |
| 成本（§16） | `cognition`/`coordination` 元推理门 | 昂贵认知前判值不值;runaway 硬闸 |
| 可观测/纠偏（§17） | 全 enforce 点写 event;`harness` 路由纠偏 | 纠偏 = 事件喂 A3 |
| 多 Agent 单脑（§13） | `harness` Orchestrator | 子代理无认知、资源最小授予、结果回传、不写稳定层 |
| 记忆 vs 认知 owner（§5/§14） | `memory`(事实)/`cognition`(语义) | 单一事实源 owner |
| 事件审计（§1） | 所有 crate → `store` | 每个咽喉点写 event |

## 7. 存储(`store`)

- **event log**:append-only、单写者;`{event_id, run_id, stream_seq, turn_id, type, payload, schema_version, ts, provenance}`(`stream_seq` 排序权威)。**唯一事实源**。
- **投影(从 event 派生、可重建)**:session_state、transcript、记忆图、capability evidence、candidate store、可检索摘要、agent_workspace(大脑注意面板,canonical §18)、M3 active strategy、M4 peer/lease/replication checkpoint、M5 publisher/admission/package active release/distribution。M4 replica projection 是独立只读视图，不实现权威 append。
- **SQLite + FTS**:run/session/turn/tool/approval 结构化索引 + 全文检索。
- **记忆图存储**:`nodes`(id/type/content_ref/tier/confidence/scope/resting_activation/recency…)、`edges`(from/to/type/weight/…);`current_activation` 为瞬时(计算/缓存,不必持久)。
- **candidate store**:`candidate`(target/evidence/confidence/provenance/conflict/target_tier/status)→ 提升 → 稳定;带双向血缘(§8)。
- **schema 版本/迁移(§9)**:每对象 schema_version;读时 upcast 或 migration projection;breaking change 留 note;replay 记 schema/policy/loop/model version。
- **M0 边界**:SQLite 单文件;向量召回 M0 最小或后置;热/冷分层、多设备同步 M2+(canonical §19)。

## 8. M0 边界 + M1–M3 扩展点

M0 = `requirements/03` 的 22 项完整闭环。每项有 crate/trait 家 + 扩展位(trait 默认实现 + 事件预留,不破契约即可演进)。

| M0 能力（requirements/03) | crate/trait 家 | M0 实现 | 扩展点 |
|---|---|---|---|
| harness/loop/Run-Session-Turn/Event 协议 | harness/loop/protocol/store | §4 | 多入口/workflow M1+ |
| CLI + Local Gateway 双入口 + Gateway-compatible approval | gateway/cli/communication/approval | text + 双入口 | 多端/平台 M2+ |
| SQLite/FTS + event log | store | §7 | 热冷分层/同步 M2+ |
| Context/Memory/Skills/Tool-schema 分层 | context/memory/capabilities | 分层 + 候选边界 | 自动压缩 M1 |
| ToolRegistry/ToolsetResolver/CapabilityRegistry | capabilities | 一等接入 | CapabilityReasoner M1 |
| MCP 一等(stdio) + local skills + local plugin manifest | capabilities | stdio + manifest | dynamic discovery/runtime M1 |
| Shell/File/MCP backend | execution | §4.5 | Notification M1；Browser/PTY/Computer/AppApi M2；RemoteExecutor M4 |
| Policy/Approval/Audit + Delegation/AutonomyEnvelope | policy/approval | §4.4 | managed policy M2 |
| CoordinationReasoner/ResourcePlan/DoneContract/DecisionTrace | coordination(trait) | 规则/单模型调用产约束 | 资源图/编排进化 M2–M3 |
| Proactive 最小闭环 | cognition ProactivityEngine(trait) | 定时器 tick 地板 | 激活扩散 M2+ |
| 时间化记忆/UserModel 候选边界 | memory/cognition | 候选链 + 元数据 | 多时间尺度稳定化 M2 |
| CognitiveMapUpdateProposal | cognition(trait) | 低置信候选 | 提升/replay M3 |
| FailureEvidence/digest | eval | 分类 + digest | 聚类/regression M2 |
| deterministic verifier/trace export | eval | §4 verifier | LLM judge M2 |
| Model profile/config/secrets/ConfigDoctor | models/config | 骨架 + doctor | routing M1 |
| Communication Fabric 数据边界 | communication | text + session/disclosure 边界 | 平台/语音 M2 |
| 原创性与合规检查 | CI/doctor | CI 门(canonical §12) | 发布审计 M2 |

跨切面不变量(能力门/并发/撤销/schema/认知输入安全/成本/调度,canonical §6/§7/§8/§9/§15/§16/§18)M0 即 enforce(见 §6)。

## 9. 当前状态

architecture/03 完成:**crate 边界 + 协议(含 §2.1.1 完整事件分类)+ M0 数据流 + 承重墙(含 §4.7 trait 签名)+ 认知层 trait(含 §5.6 签名)+ 治理 enforcement + 存储 + M0 边界/扩展点**。承重墙具体冻结、认知层 trait 化可替换、每条不变量有 enforce 位、每项能力有家有扩展位。事件 taxonomy 只末尾 additive：M2 前缀 86、M3 前缀 89、M4 前缀 93、M5 前缀 97；M5 已合入，V1 核心大脑闭合只按 canonical §28 追加 R 组两项并保持前 97 strict prefix。

`prd/01` 已承接本篇拆成 S1–S22 可执行验收。认知层涌现参数(激活扩散/契合度/能力门阈值等)按"建 + 观察 + 调",不在本篇定死。
