# forme 底座实施 PRD 总纲(Program PRD)

本文是 forme 实施型 PRD 体系的**总纲**。它不写单个模块的实现细节,而是**统筹**:把 `requirements/01-04` 与 `architecture/03` 收敛成不降级的 M0 实施方案,定义构建顺序、全局验收(S1–S22)、**集成契约(防各自为战)**、跨切规范和**每个模块 PRD 的模板**。

**一纲多目**:系统极复杂、功能极多,一份文档写不尽,也不可能一次建完。故 PRD = "1 份总纲 + 每个大模块 1 份实施级 PRD(`prd/02`..`prd/17`)"。模块 PRD 独立实施、增量推进,但**必须照总纲的集成契约咬合成一个整体**,绝不各自为战。

- **承上**:总纲是对前面全部设计(需求/功能/技术方案/口径)的收敛,不偏离。
- **启下**:总纲给全局观与统筹(规范、约束、模块咬合规则);模块 PRD 给"照着能一字不落实现"的细节。
- 模块清单见 §9.4、集成契约见 §9、模块模板见 §10。

## 0. 本文定位

- **它是什么**:PRD 体系总纲。统筹全局 + 定义集成契约 + 给模块 PRD 模板;**不写单模块实现细节**(那在 `prd/02`+)。
- **输入**:`requirements/01-02`(愿景/能力)、`requirements/03`(M0 范围:22 项结构 + D1–D19)、`requirements/04`(验收矩阵 + S1–S22)、`architecture/03`(crate/协议/数据流/enforce)、`canonical-contract.md`(唯一口径)。同名对象口径以 canonical 为准。
- **准入**:已满足 `prd/README.md` 准入条件(requirements/01-04 + architecture/03 均在)。
- **不做**:不新增未进 requirements 的能力;不绕过 architecture 自定实现方式;不复制第三方源码/文档/prompt/命名/目录/测试 fixture(canonical §12)。
- **本总纲含**:§1 不降级承诺、§2 构建顺序、§3 工作流详述、§4 测试策略、§5 验收映射、§6 原创合规、§7 逐场景验收规格(S1–S22,满足 requirements/04 "进入实现前"门)、§8 不进入 M0、**§9 集成契约与模块索引(防各自为战)、§10 模块 PRD 模板与跨切规范**。
- **模块 PRD 状态**:总纲 + `prd/02`..`prd/17` 全部完成(索引见 §9.4);后续随实现回流微调。

## 1. M0 范围与不降级承诺

M0 = **完整核心体系的第一条可运行闭环**(requirements/03),不是缩水 MVP、不是 CLI demo、不是只定义对象、不是把难点推迟。

**不降级红线**:requirements/03 的 22 项结构每一项都必须在 M0 形成第一版可运行链路,可以朴素但不能缺席。广度可收敛(有限入口/backend/transport/观察源/验证方式),结构不可缺。

22 项结构(requirements/03 §完整性边界)在本 PRD 中按 §2 的工作流落位,§5 验收映射逐项对应 S1–S22。

## 2. 构建顺序与工作流

**原则**:M0 范围完整,但**构建按依赖排序**——承重墙先行(其余一切 assert against 它),再能力与入口,再协调,再记忆/认知候选与主动闭环;失败学习与验证从一开始就跨切。这**不是**"先薄片再加厚"(requirements/03 明确否定缩水 MVP),而是"完整 M0 的依赖序施工"。

```text
W0 工程基线 + 合规门
   └─> W1 承重墙运行时
          ├─> W2 治理(policy/approval/delegation)
          │      └─> W3 能力基质(registry/MCP/skills/plugins/exec)
          │             └─> W4 入口 + 沟通边界(CLI+Gateway/Communication Fabric)
          └─> W5 协调内核(CoordinationReasoner/subagent)
                 └─> W6 记忆与认知候选(UserModel/CognitiveMap 候选/撤销)
                        └─> W7 主动最小闭环(ValueGate+CompetenceGate guard)
W8 失败学习 + 验证(FailureEvidence/verifier/trace)—— 从 W1 起跨切集成
```

| 流 | 目标 | 主要 crate(architecture/03 §1) | 覆盖 req 决策 / 结构 |
|---|---|---|---|
| W0 | workspace + crate 脚手架;CI/doctor 原创合规门;config/secrets/ConfigDoctor;schema_version 纪律 | config / (CI) / store | D13、D15、D19;结构 20、22 |
| W1 | 协议对象 + append-only event store + harness 生命周期 + loop 状态机 + Run/Session/Turn + ResumeState + 并发不变量 + replay | protocol / store / harness / loop | D2、D17、D19;结构 1–3、5 |
| W2 | PolicyEngine(执行前重查)+ Gateway-compatible approval + Delegation/AutonomyEnvelope 运行时 enforce + audit;trust 不覆盖 permission | policy / approval | D10、D12;结构 12、13 |
| W3 | CapabilityRegistry/ToolRegistry/ToolsetResolver + MCP(stdio)一等 + skills 按需 + plugin manifest + Shell/File/MCP backend | capabilities / execution | D3、D4、D5;结构 7–11 |
| W4 | CLI + Local Gateway 双入口(同 run/同 event)+ Communication Fabric 数据边界(M0 text)+ DisclosurePolicy/Termination/ExternalGrant | gateway / cli / communication | D1、D16;结构 4、21 |
| W5 | 最小 CoordinationReasoner:GoalFrame→ResourcePlan/DoneContract/AutonomyEnvelope/DecisionTrace + subagent 受限委派 | coordination / harness(Orchestrator) | D7;结构 14、(13 委派) |
| W6 | 记忆分层 + 时间化记忆/UserModel 候选 + ImportedHistoricalEvidence 降权 + CognitiveMapUpdateProposal + candidate-before-promotion + 撤销/派生失效 | memory / cognition | D8、D11、D18;结构 6、16、17 |
| W7 | 主动最小闭环:observation→opportunity→ValueGate→proposal→feedback→evidence;`ProactiveEmissionGuard = ValueGate ∧ CompetenceGate ∧ Policy/AutonomyEnvelope`;能力门降级 | cognition(Proactivity/CompetenceGate)| D6;结构 15 |
| W8 | FailureEvidence 分类 + digest;deterministic verifier + tool/output validator + trace export;结果进 event | eval | D9、D14;结构 18、19 |

**M0 薄化指引(结构不缺席 ≠ 处处做厚,防镀金)**:OrchestrationLibrary = 接口 + 2~3 个种子模式(不做签名学习);S16/S17 = 数据模型 + enforce 点(已是最薄);记忆图激活字段建 schema、扩散机制休眠至 M2(M0 用定时器地板 + 前瞻意图确定触发);CompetenceGate = 固定阈值;CoordinationReasoner = 规则/单模型调用。结构必须在,厚度等使用数据。

## 3. 工作流详述(骨架)

每个工作流给:目标、依赖、关键对象/事件、治理与错误边界、对应验收场景。**逐场景测试规格在 §7 填。**

### W0 工程基线 + 合规门

- **目标**:可编译的 workspace + crate 脚手架;原创合规检查从第一天可执行;配置/密钥可检查;schema 版本纪律就位。
- **关键产出**:crate 骨架(§1);CI/doctor 检查(`research/upstreams/**` 不进 build/import;无第三方源码/prompt/fixture/目录复制;GPL/未知许可证默认不进主工程);`ConfigDoctor`(provider/credential/capability/MCP/plugin/fs·shell permission 可检查解释);全协议对象带 `schema_version`。
- **治理/错误**:检查失败不能进 release-ready;配置错误必须显式报告,不能表现为运行时猜测。
- **验收**:S22(原创 CI/doctor)、ConfigDoctor(矩阵"ConfigDoctor"行)、S21(schema 纪律基线)。

### W1 承重墙运行时

- **目标**:一次 RunRequest 能被推成可观察、可恢复、可审计的 run,全程事件落 append-only log。
- **关键对象/事件**:§2.2/§2.1 协议;LoopEngine 状态机(architecture/03 §4.1);event log 单写者;投影从 event 派生;ResumeState;replay 记 schema/policy/loop/model version。
- **治理/错误**:harness-first(真实动作只经 harness 路径);per-session 串行;max turns/cancel/tool error/approval deny 后不丢状态。
- **验收**:S1(同协议提交)、S2(loop 可恢复状态机)、持久化矩阵行、S19(并发)、S21(replay)。

### W2 治理

- **目标**:权限/审批/放权在运行时 enforce,trust 不覆盖 permission。
- **关键对象/事件**:PolicyEngine 合并序(deny>ask>allow);执行前重查 toolset/参数/路径/网络/命令/sandbox;ApprovalRequest/Resolved/RunWaiting/RunResumed(Gateway-compatible);DelegationGrant + AutonomyEnvelope(scope/期限/预算/动作类型/撤销/审计)。
- **治理/错误**:拒绝审批后高风险动作不执行;trust 高不越 deny;越权参数/不可见工具名被拒。
- **验收**:S2(审批拒绝)、Approval/Delegation 矩阵行、S6(AutonomyEnvelope 产出)、S11(subagent 越权拒绝)。

### W3 能力基质

- **目标**:tools/skills/MCP/plugins/hooks 统一索引,生成当前 run 的 toolset 与能力证据;Shell/File/MCP 可执行。
- **关键对象/事件**:CapabilityRegistry/ToolsetResolver;MCP stdio discovery/allowlist/call events/timeout/错误分类/禁用;skill metadata 默认可见、正文按需加载;plugin manifest enable/disable/trust/policy;ActionBackend(Shell/File/Mcp)+ output budget/timeout/cancel + CapabilityEvidence。
- **治理/错误**:能力来源不明/风险未知不默默暴露;未授权 MCP/skill/plugin 贡献不可见;plugin hook 不绕过执行前重查;shell 不作默认兜底、文件写需 diff。
- **验收**:S3(MCP)、S4(skill 按需)、S5(plugin 边界)、CapabilityRegistry 矩阵行。

### W4 入口 + 沟通边界

- **目标**:CLI 与 Local Gateway 都只是入口、提交同一种 run/消费同一种 event;沟通触手先进 Gateway/Communication Fabric。M0 只跑 text。
- **关键对象/事件**:RunRequest 统一;CommunicationEvent(modality/carrier/channel adapter/participant/scope/session);CommunicationSession + DisclosurePolicy + TerminationPolicy + ExternalCommunicationGrant(数据模型与 enforce 点齐备,真实 adapter 不在 M0)。
- **治理/错误**:adapter 不能绕过 Gateway 直调 loop/写 memory;外部会话不可无期限/无限预算/默认披露;DisclosurePolicy 阻断敏感披露;代表表达保持 Agent 身份边界。
- **验收**:S1、S12(标准化入口)、S13(外部会话范围/终止)、S14(披露阻断)、S15(代表身份)、S16(本地设备授权,数据模型)、S17(A2A bounded session,数据模型)。

### W5 协调内核

- **目标**:把目标/情境/资源/认知地图/信任/失败证据转成行动路线,而非 prompt 一句话。
- **关键对象/事件**:GoalFrame/SituationModel/ResourceInventory → ResourcePlan/DoneContract/AutonomyEnvelope/DecisionTrace(引用 CognitiveMapRef/TrustProfileRef/FailureEvidenceRef);subagent = harness 管理的 child run(独立上下文、受限 toolset/model/permission/budget、summary/result 回传)。
- **治理/错误**:无完成标准/资源不可信时不假装具备行动路线;child run 不写父 session 稳定态、越权被拒。
- **验收**:S6(行动路线)、S11(subagent 受限委派)。

### W6 记忆与认知候选

- **目标**:用户理解与长期学习是时间化、过程化、候选化的;稳定写入 candidate-before-promotion;撤销可派生失效。
- **关键对象/事件**:raw storage/memory substrate/session history/summary/candidate/stable 分层;UserAttributeCandidate→UserModelAttribute(evidence/confidence/stability/scope/时间戳/冲突/反馈);ImportedHistoricalEvidence 低权重、不直接塑造稳定画像;CognitiveMapUpdateProposal 低置信候选;evidence→object 与 object→derived 双向血缘;RetractionEvent→引用者再评估候选。
- **治理/错误**:candidate 不自动转 stable;单次对话/一次失败/历史导入不直接生成稳定画像或放权;撤销后停止把被撤属性当有效稳定认知,但不自动删历史事实。
- **验收**:S8(UserModel 候选 + 历史降权)、S9(CognitiveMap 候选)、S20(撤销/派生失效)、Context/Memory 矩阵行。

### W7 主动最小闭环

- **目标**:主动式从 M0 进内核但受治理;不是"有价值就发出"。
- **关键对象/事件**:authorized observation→OpportunityDetector→ValueGate→ActionProposal/LearningTask→user feedback→proactive evidence;`ProactiveEmissionGuard = ValueGate ∧ CompetenceGate ∧ Policy/AutonomyEnvelope`;CompetenceGate 读 MapConfidence/AgentSelfModel/CapabilityEvidence/TrustProfile/FailureEvidence,不足则降级到 L2 Prepare/L1 Suggest/L0 Observe。
- **治理/错误**:被拒 proposal 不自动执行、不反复打扰;降级原因/失败证据/所需补充信息进 trace;降级后仍不绕过 policy/approval/AutonomyEnvelope。
- **验收**:S7(主动最小闭环)、S18(CompetenceGate 降级)。

### W8 失败学习 + 验证(跨切)

- **目标**:失败进入学习与治理;验证结果进 trace,成功不掩盖中间失败。
- **关键对象/事件**:FailureEvidence 分类(工具失败/验证失败/审批拒绝/主动误报/资源选错)+ failure digest(影响/scope/相关资源/验证/反馈/建议修正),关联 DecisionTrace/CapabilityEvidence/TrustProfileRef/CognitiveMapUpdateProposal;deterministic verifier + tool result/final output validator + trace export,结果入 event log。
- **治理/错误**:final answer 成功不覆盖失败;验证失败不能标记成功。
- **验收**:S10(FailureEvidence/digest)、Verification 矩阵行。

## 4. 测试策略

- **用事件和状态证明,不用最终回答证明**(requirements/04 原则 1):从 run/session/event/trace 断言闭环各步。
- **每个核心能力有正反场景**(原则 2):允许能执行 / 拒绝不能执行 / 等待能恢复 / 失败能收尾 / 越权被拒 / 候选不污染稳定层 / trust 不覆盖 permission。
- **覆盖三类正确性**(原则 3):Runtime(推进/暂停/恢复/结束)、Governance(权限/审批/放权/审计)、Learning(记忆/认知/失败/协调进候选与证据链)。
- **每场景产物**(requirements/04 "进入实现前"):测试场景 + fixtures + 命令/API 调用 + 预期 event 序列 + 预期 store 状态 + 预期错误分类 + 预期用户可见结果。**这些逐场景规格在 §7 填。**
- **认知层离线开发(replay 驱动)**:事件溯源使 memory/cognition/proactivity 可对**录制/合成事件流**开发与调参,不必等完整 harness 活体——认知模块的 fixture 即事件流,把"深认知→高质量主动"这条核心赌注的反馈回路从"全系统建完"缩短到"有事件 fixture 即可跑"。
- **价值验收层(M1)**:S1–S22 证明结构与治理正确,**不证明 Agent 有用**。其上一层:北极星度量(canonical §17)+ golden tasks 集(真实任务样本,对照"直接问模型"基线)——防"治理正确但认知平庸"。M0 定义指标与任务集,M1 运行。
- **不复制第三方测试 fixture**(canonical §12):fixture 自建。

## 5. 验收映射(S1–S22 → 工作流)

| 场景 | 工作流 | 证据落点(crate/事件) |
|---|---|---|
| S1 CLI/Gateway 同协议 | W1、W4 | 同 RunRequest / 同 event stream / loop 不被入口私有化 |
| S2 高风险审批拒绝 | W2 | PolicyEngine ask/deny + ApprovalRequest + 拒后不执行 + Trace |
| S3 MCP 一等 | W3 | discovery/allowlist/MCP event/timeout/禁用 |
| S4 Skill 按需 | W3 | 默认只 metadata / 选中才加载 / 加载有 event |
| S5 Plugin 边界 | W3 | manifest 贡献 / disabled 不可见 / 进 registry 受治理 |
| S6 协调行动路线 | W5 | GoalFrame→ResourcePlan/DoneContract/AutonomyEnvelope/DecisionTrace |
| S7 主动最小闭环 | W7 | observation→opportunity→ValueGate→proposal→feedback→evidence |
| S8 UserModel 候选/历史降权 | W6 | UserAttributeCandidate + ImportedHistoricalEvidence 低权重 |
| S9 CognitiveMap 候选 | W6 | 低置信 CognitiveMapUpdateProposal 保持候选 |
| S10 FailureEvidence/digest | W8 | 分类 + digest + 关联 trace,不被成功掩盖 |
| S11 Subagent 受限委派 | W5、W2 | child run 独立上下文/受限/回传/越权拒 |
| S12 CommunicationEvent 标准化 | W4 | 转标准 CommunicationEvent,adapter 不直调 loop |
| S13 外部会话范围/终止 | W4 | ExternalCommunicationGrant + TTL/budget/终止 |
| S14 披露阻断 | W4 | DisclosurePolicy 阻断 + audit event |
| S15 代表身份边界 | W4 | DisclosurePolicy.representation + 高影响触发 approval |
| S16 本地设备授权 | W4 | DeviceGrant/ObservationScope/RetentionPolicy(数据模型) |
| S17 A2A bounded session | W4 | bounded session 强目的/强终止(数据模型) |
| S18 CompetenceGate 降级 | W7 | 读 Map/Capability/Trust/Failure → 降级 + 原因进 trace |
| S19 并发一致性 | W1 | per-session 串行 / 单写者 / 后台只读快照投候选 |
| S20 撤销/派生失效 | W6 | 双向血缘 + RetractionEvent → 再评估候选 |
| S21 Schema/Replay | W1、W0 | schema_version + upcast + replay 记 version/diff |
| S22 原创 CI/Doctor | W0 | upstream 不进 build/import + 检查失败不 release-ready |

## 6. 原创性与合规边界

- M0 从第一天建立 CI/doctor 原创合规门(D15/S22):`research/upstreams/**` 本地参考材料不进 build/import/include;不复制第三方源码/文档/prompt/错误信息/目录结构/命名/测试 fixture;GPL/未知许可证材料默认不进主工程。
- 需公开协议兼容/依赖/复用时,记录 source、license、copy_risk、implementation_rule、notice action。
- 检查失败不能进 release-ready;发布前另需完整 LICENSE/NOTICE 与依赖安全审计(不在 M0 承诺,canonical §12 / requirements/04 "开源发布前")。

## 7. 逐场景测试规格

把 S1–S22 每条转成可执行验收(requirements/04 "进入实现前"门):**Fixtures + 触发 + 预期事件序列 + 预期 store 状态 + 反向断言 + 错误分类/用户可见结果**。事件名用 `architecture/03 §2.1.1` 的完整分类。**S1–S22 全部覆盖**:承重墙 §7.1–7.5(S1/S2/S19/S21/S22)、能力 §7.6–7.8(S3/S4/S5)、协调认知主动失败 §7.9–7.16(S6/S11/S8/S9/S20/S7/S18/S10)、沟通 §7.17–7.22(S12–S17)。

### 7.1 S1 CLI/Gateway 同协议提交(W1、W4)

- **Fixtures**:最小 workspace + session config;deterministic model stub 返回纯 final(不触发高风险动作);CLI client 与 Gateway client 指向同一 harness 实例。
- **触发**:分别从 CLI 和 Gateway `submit_run` 同一 RunRequest(相同 input/session/agent profile)。
- **预期事件序列(两入口同构)**:`RunAccepted{source=user_turn}` → `SessionBound{policy_profile,model_profile,toolset_ref}` → `TurnStarted` → `ContextBuildStarted/Finished` → `ModelCallStarted/Finished` → `OutputClassified{kind=final}` → `TurnComplete` → `RunComplete`。
- **预期 store**:两 run 产生同构 event 序列(仅 run_id/ts/source-channel 不同);event log 单写者;两 run 均可从 `stream_events` 订阅到相同类型序列。
- **反向断言**:client 无 loop 直调入口(crate 边界上 loop 不导出给入口);Gateway 提交仍经 `SessionBound` 绑定 policy,不能绕过。
- **用户可见**:两入口都能看到 approval waiting / tool result / verification / run result 的事件面(本场景为 final-only,事件面一致)。

### 7.2 S2 高风险工具调用审批拒绝(W2)

- **Fixtures**:policy profile 将目标 shell/file 动作标为 `ask`;model stub 提出该高风险 ToolCall;一个会"拒绝"的 approval responder。
- **触发**:`submit_run`,input 引导模型 propose 高风险 shell/file 动作。
- **预期事件序列**:… `OutputClassified{kind=tool}` → `ToolCallProposed{tool,args}` → `ToolPolicyEvaluated{decision=ask,rule_source,reason}` → `ApprovalRequested{risk,scope,rollback_boundary,expires_at,choices}` → `RunWaiting{wait_reason=approval}` →(拒绝)`ApprovalResolved{outcome=denied}` → `ActionDenied{reason}` → `FailureEvidenceRecorded{class=approval_denied}` → `RunComplete|RunAborted`。
- **预期 store**:ApprovalRequest 记录 reason/risk/scope/rollback;**无 `ActionStarted`/`ActionCompleted`(该 intent)**;FailureEvidence 或 DecisionTrace 记录拒绝原因与后续路线。
- **反向断言**:拒绝后原高风险动作不执行;`deny` 决策路径直接 `ActionDenied`、不进 `ApprovalRequested`;模型构造不可见工具名/越权参数被 `ToolPolicyEvaluated{decision=deny}` 拦。
- **错误分类**:`FailureEvidence.class = approval_denied`(或 `policy_deny`)。
- **用户可见**:run 正确暂停/降级/结束,不静默执行。

### 7.3 S19 并发与一致性不变量(W1)

- **Fixtures**:一个 session;连续提交 run A、run B;一次 background tick;一个 subagent child run。
- **触发**:同 session `submit_run` A、B;触发 `ProactivityEngine::tick`;`SubagentSpawned`。
- **预期事件序列/行为**:A、B 进 per-session 串行队列——B 的 `TurnStarted` 不早于 A 的 `RunComplete`(或带显式排队标记);background tick 产 `ObservationRecorded`/`ReflectionProduced`/`CandidateCreated`,**不产 stable 写入事件**;subagent 产 `SubagentSpawned`→…→`SubagentResultReturned`,**无对父 session 的 stable `MemoryNodeAppended`**。
- **预期 store**:event log 单写者;投影从 event 派生;后台产物停在 `CandidateCreated`(candidate 状态);subagent 只回 summary/result。
- **反向断言**:后台/subagent 不产生直接改 stable memory 或前台 session state 的事件;两 run 不交错写同一 session 聚合。
- **治理**:`policy deny` / `approval revoke` / `budget exhausted` 能打断后续执行并进 event(`ActionDenied`/`RunLimited`)。

### 7.4 S21 Schema Version 与 Replay 边界(W1、W0)

- **Fixtures**:一批 `schema_version=v1` 历史事件;带 upcast/projection-migration 的 v2 reader;replay 命令。
- **触发**:v2 reader 读 v1 run;`EventStore::replay(run, at=SchemaSnapshot)`。
- **预期结果**:v1 事件经读时 upcast/projection migration 被 v2 reader 解释;`ReplayReport` 记录 schema/policy/LoopSpec/model profile/tool schema snapshot;replay 输出与当前 projection 的 diff。
- **预期 store**:历史 event 不可变,无 in-place 改写。
- **反向断言**:breaking schema change 无 migration note → 检查失败(不静默改历史语义);replay 不覆盖历史 event,只报 diff。

### 7.5 S22 开源原创 CI/Doctor 边界(W0)

- **Fixtures**:含 `research/upstreams/**` snapshot 的仓库树;构造一处主工程对 upstream 的 import;构造一段复制第三方 prompt/目录结构的内容;一个合规对照树(upstream 仅在 research/ 且不被引用)。
- **触发**:运行 doctor/CI 原创合规检查。
- **预期结果**:检查发现 (a) 主工程 import/include/build 引用 upstream、(b) GPL/未知许可证材料进主工程、(c) 复制第三方 prompt/错误信息/目录/fixture → `ComplianceCheckResult{outcome=fail, blocking=true}`。
- **预期状态**:检查失败 → 不能进 release-ready。
- **反向断言**:合规对照树 → 检查通过;需协议兼容/依赖时,记录 source/license/copy_risk/implementation_rule/notice action 后放行。
- **用户可见**:CI 红/绿 + findings 列表。

### 7.6 S3 MCP tool/resource 一等接入(W3)

- **Fixtures**:stdio MCP server stub 声明若干 tools/resources;allowlist 只放行一部分;一个会 timeout 的 tool、一个 schema-mismatch 的 tool。
- **触发**:绑定 MCP server;model propose 调 allowed tool;再 propose 调 not-allowed tool;触发 timeout/error;禁用 server 后再 propose。
- **预期事件序列**:`McpDiscovered{server,tools/resources}` → `ToolsetResolved`(仅 allowlisted)→ `ToolCallProposed` → `ToolPolicyEvaluated{allow}` → `McpCallEvent{server,tool}` → `ActionCompleted`;not-allowed → `ToolPolicyEvaluated{deny}` 或不可见;timeout/error → `McpCallEvent{error_class=timeout|schema_mismatch|server_error}` → `FailureEvidenceRecorded{class=tool_failure}`;禁用后 `PluginToggled`/server disabled → 该 server 能力不再进 `ToolsetResolved`。
- **预期 store**:MCP tools/resources metadata 独立记录;每次调用独立 `McpCallEvent`;timeout/error 分类。
- **反向断言**:未授权 MCP server/tool/resource 不可见或不可执行;禁用 server 后当前 run 不再暴露其能力。
- **错误分类**:`FailureEvidence.class = tool_failure`(mcp_timeout / mcp_schema_mismatch / mcp_server_error)。

### 7.7 S4 Skill 按需加载(W3)

- **Fixtures**:SkillRegistry 含多个 skill(各有 metadata + body);其中一个被 CoordinationReasoner/model 选中,其余不选。
- **触发**:`submit_run` → context build → 引用一个 skill。
- **预期事件序列**:`ContextBuildStarted` → `SkillMetadataExposed{skills[]}`(仅 metadata)→(选中)`SkillBodyLoaded{skill,scope/version/trust,trigger}` → `ContextBuildFinished`。
- **预期 store**:context 默认只含 skill metadata;只有被选/被引用的 skill 加载正文;加载有 event;scope/version/trust boundary 可追溯。
- **反向断言**:未选中的 skill 正文不进上下文(无对应 `SkillBodyLoaded`);默认不无差别塞正文。

### 7.8 S5 Plugin 贡献边界(W3)

- **Fixtures**:local plugin manifest 声明 tools/skills/MCP servers/hooks/config contribution;一个 enabled 插件 + 一个 disabled 插件;一个试图跳过执行前重查的 plugin hook。
- **触发**:加载 manifests;enable 一个 / disable 一个;run 中检查可见性;plugin hook 试图绕过 re-check。
- **预期事件序列**:`PluginContributionRegistered{manifest,contributions,trust}` → `PluginToggled{enabled}`;enabled 贡献 → `CapabilityIndexed`/`ToolsetResolved` 含之,受 `ToolPolicyEvaluated`;disabled → 不进 `ToolsetResolved`;plugin hook 贡献的 tool 仍经 `ToolPolicyEvaluated`(执行前重查)。
- **预期 store**:manifest 贡献可追踪;enable/disable/trust/policy 生效。
- **反向断言**:disabled 插件贡献的 tool/skill/MCP/hook 不进当前 run;plugin hook 不能绕过 tool execution re-check。

### 7.9 S6 CoordinationReasoner 生成行动路线(W5)

- **Fixtures**:用户 goal input;有上下文 + 缺失信息 + 可用资源(tools/skills/MCP);存在 CognitiveMap/Trust/Failure refs。
- **触发**:`submit_run`,目标需要协调。
- **预期事件序列**:`GoalFramed{goal_frame}` →(内部 situation/inventory)→ `ResourcePlanned` → `DoneContractSet` → `AutonomyEnvelopeSet` → `DecisionTraceRecorded{refs(map/trust/failure),rationale}` →(进 loop 执行)。
- **预期 store**:GoalFrame/ResourcePlan/DoneContract/AutonomyEnvelope/DecisionTrace 记录;DecisionTrace 引用 CognitiveMapRef/TrustProfileRef/FailureEvidenceRef;可解释"为什么用某些资源、不用另一些"。
- **反向断言**:无 DoneContract 或资源不可信时,不产生"已具备行动路线"(标记 blocked,不进执行相)。

### 7.10 S11 Subagent 受限委派(W5、W2)

- **Fixtures**:parent run;subagent role 带受限 toolset/model/permission/budget;subagent 尝试越权。
- **触发**:parent 委派 → spawn child run。
- **预期事件序列**:`SubagentSpawned{child_run, 限制}` → child `RunAccepted{source=subagent}` → fresh `ContextBuildStarted/Finished` →(越权尝试)`ToolPolicyEvaluated{deny}`/`ActionDenied` → `SubagentResultReturned{summary/result,status}` →(parent trace 记录)。
- **预期 store**:child run 独立上下文、受限四项;结果按 summary/result contract 回;成功与失败进 parent trace。
- **反向断言**:child run 不能用未授权工具(越权被拒);不写父 session 稳定态。

### 7.11 S8 UserModel 候选更新与历史证据降权(W6)

- **Fixtures**:用户明确表达一个偏好/工作方式;一份历史导入资料。
- **触发**:处理用户表达;导入历史资料。
- **预期事件序列**:`UserAttributeCandidateCreated{attribute,evidence,confidence,first/last_at,stability,scope}`;多次过程证据/确认 → `CandidatePromoted`(或置信升);历史资料 → `ImportedHistoricalEvidenceRecorded{low_weight=true}`。
- **预期 store**:candidate 记录 source evidence/confidence/时间戳/stability/scope/feedback;ImportedHistoricalEvidence 为低权重候选线索。
- **反向断言**:单次对话/一次失败/历史导入不直接生成稳定 UserModelAttribute、trust 提升、delegation grant 或高影响主动行为;用户可拒绝/降级/确认关键候选。

### 7.12 S9 CognitiveMap 候选更新(W6)

- **Fixtures**:一个已结束任务,有 trace + 用户反馈 + failure evidence。
- **触发**:任务结束后反思。
- **预期事件序列**:`ReflectionProduced` → `CognitiveMapUpdateProposed{frame/quality/blindspot/resource,confidence=low}` → `CandidateCreated`。
- **预期 store**:proposal 进 candidate 层;stable CognitiveMap 不自动变。
- **反向断言**:一次成功/失败不直接改稳定认知地图;用户可拒绝/降级候选。

### 7.13 S20 撤销与派生失效(W6)

- **Fixtures**:stable UserModelAttribute/AgentSelfAttribute(evidence→object 血缘);派生对象(CognitiveMap/TrustProfile/CoordinationPolicy/LoopSpec/PartnershipState,object→derived 血缘)。
- **触发**:用户撤销/纠正该稳定属性。
- **预期事件序列**:`RetractionEvent{target_object,evidence_lineage}` → `ReevaluationTaskCreated{derived_refs,trigger}` → 派生对象各产 `CandidateCreated`/`CandidateDowngraded`(invalidation/downgrade/re-evaluation)。
- **预期 store**:双向血缘记录;历史事实不自动删;停止把被撤属性当有效稳定认知用。
- **反向断言**:撤销后不继续静默使用被撤属性;不自动删除历史事实(event 不可变)。

### 7.14 S7 主动式最小闭环(W7)

- **Fixtures**:授权 observation source;一个低风险机会;ValueGate 配置。
- **触发**:observation → `ProactivityEngine::tick`。
- **预期事件序列**:`ObservationRecorded{source,scope,grant_ref}` → `OpportunityDetected{seed,activation_shape}` → `ValueGateEvaluated{decision}` → `ProactiveProposalEmitted{proposal_kind,level,guard}` →(用户)`ProactiveProposalResolved{outcome=adopt|reject|defer,feedback}`。
- **预期 store**:proactive evidence 记录;feedback 影响后续 proactive evidence。
- **反向断言**:被拒 proposal 不自动执行、不反复打扰;未授权 observation 不进 event stream。

### 7.15 S18 CompetenceGate 主动降级(W7)

- **Fixtures**:构造 ValueGate 通过、但 MapConfidence/AgentSelfModel 置信度不足的机会。
- **触发**:proactive emission 路径。
- **预期事件序列**:`ValueGateEvaluated{pass}` → `CompetenceGateEvaluated{scope,risk,max_level,reads(MapConfidence/AgentSelfModel/CapabilityEvidence/Trust/Failure)}` → `ProactiveProposalEmitted{level=降级到 L2 Prepare/L1 Suggest/L0 Observe}`(附降级原因/失败证据/所需补充信息)。
- **预期 store**:降级原因、相关失败证据、所需补充信息进 trace。
- **反向断言**:降级后 proposal 不绕过 policy/approval/AutonomyEnvelope。

### 7.16 S10 FailureEvidence 与 failure digest(W8)

- **Fixtures**:触发工具失败/验证失败/审批拒绝/主动误报/资源选错。
- **触发**:各类失败发生。
- **预期事件序列**:`FailureEvidenceRecorded{class,impact,scope,related_refs(trace/capability/trust/map),suggested_fix}` → `FailureDigestUpdated{digest_ref,members}`。
- **预期 store**:分类 + digest;关联 DecisionTrace/CapabilityEvidence/TrustProfileRef/CognitiveMapUpdateProposal。
- **反向断言**:final answer 成功不覆盖/隐藏中间失败;验证失败不能标记成功。

### 7.17 S12 CommunicationEvent 标准化入口(W4)

- **Fixtures**:CLI 或 Local Gateway 输入;一个 channel adapter。
- **触发**:输入经 adapter → Gateway。
- **预期事件序列**:`CommunicationEventReceived{modality=text,carrier,channel_adapter,participant,scope}` →(Gateway 转)`RunAccepted{source=communication}` 或 `CommunicationSessionOpened`。
- **预期 store**:事件记录 modality/carrier/channel adapter/participant/scope/session routing;Gateway 转成 RunRequest 或 CommunicationSession;harness 只收标准化请求。
- **反向断言**:非标准 adapter 不能直调 loop 或写 memory;adapter 不绕过 Gateway。

### 7.18 S13 外部沟通会话范围与终止(W4)

- **Fixtures**:模拟外部参与者事件(不需真实公网);ExternalCommunicationGrant 配置。
- **触发**:创建 CommunicationSession;模拟对话;超 TTL/轮次/预算/目的偏离。
- **预期事件序列**:`ExternalCommunicationGranted{purpose,disclosure,ttl,budget,transcript_policy}` → `CommunicationSessionOpened{purpose,ttl,budget}` →(对话)→(超限)`CommunicationSessionTerminated{termination_reason}` 或要求用户确认。
- **预期 store**:模拟外部参与者只能在 session 范围内对话;transcript 进 event/evidence/candidate,不自动写 stable memory。
- **反向断言**:外部会话不能无期限/无限预算/默认披露;超 TTL/轮次/预算/目的偏离被终止或要确认。

### 7.19 S14 DisclosurePolicy 阻断敏感披露(W4)

- **Fixtures**:模拟外部参与者请求超出 disclosure policy 的私人/客户/内部计划/未确认判断。
- **触发**:外部请求敏感信息。
- **预期事件序列**:`CommunicationEventReceived` → `DisclosurePolicyApplied{request,outcome=refuse|blur|approve|answer,representation}` →(高影响)`ApprovalRequested`。
- **预期 store**:被阻断的问题、策略来源、处理结果进 audit event。
- **反向断言**:不把低置信候选认知对外表达为确定事实;超范围请求被拒/模糊/转审批。

### 7.20 S15 思想代理与代表表达身份边界(W4)

- **Fixtures**:`CommunicationPurpose` = 代表讲述/课程/介绍/对接;`DisclosurePolicy.representation` 指定可代表/需标注不确定/须回用户确认。
- **触发**:生成代表表达输出,含未确认观点/高影响表达。
- **预期事件序列**:`CommunicationSessionOpened{purpose=代表}` → `DisclosurePolicyApplied{representation}` →(高影响/未确认)`ApprovalRequested` → 输出;表达结果/反馈/误差 → `FailureEvidenceRecorded`/`CandidateCreated`。
- **预期 store**:输出保持 Agent 代表身份边界;高影响/未确认触发 approval。
- **反向断言**:不伪装成用户本人;未确认观点不作确定表达。

### 7.21 S16 Local Device 授权边界(W4)

- **Fixtures**:模拟设备事件(不需真实麦克风/摄像头);`LocalDeviceChannelAdapter` 需显式 DeviceGrant/ObservationScope/RetentionPolicy。
- **触发**:未授权模拟设备输入;授权后;撤销后。
- **预期事件序列**:未授权 → 输入不产生 `CommunicationEventReceived`(前置拒);授权 → DeviceGrant 记录 device/modality/purpose/scope/expires_at + `CommunicationEventReceived{carrier=hardware}`;撤销后 → 设备事件被拒/忽略。
- **预期 store**:授权事件记录 device/modality/purpose/scope/expires_at。
- **反向断言**:未授权模拟设备输入不进 event stream;撤销授权后设备事件被拒。

### 7.22 S17 Agent-to-Agent bounded session(W4)

- **Fixtures**:模拟 A2A 事件(不需真实协议);bounded session 声明任务目的/双方身份/披露范围/最大轮次/TTL/token budget/终止条件。
- **触发**:创建 bounded session;模拟对话;达成目的/预算耗尽/超时/风险升高/策略拒绝;一方请求越界。
- **预期事件序列**:`CommunicationSessionOpened{purpose,ttl,budget,participant=agent}` →(对话)→(终止条件)`CommunicationSessionTerminated{termination_reason}`;越界请求 → `DisclosurePolicyApplied{outcome=refuse}` 或终止;transcript digest → `CandidateCreated`/evidence。
- **预期 store**:双方 transcript digest 只进 candidate/evidence。
- **反向断言**:无限循环被终止条件挡;越界信息请求被拒/转审批/终止。

**§7 完成**:S1–S22 全部有可执行验收规格(Fixtures/触发/事件序列/store 断言/反向断言/错误分类),满足 requirements/04 "进入实现前"门。

## 8. 不进入 M0 与 M1–M3 衔接

- 不进入 M0:见 requirements/03 "M0 不承诺的能力"(完整 UI、远程执行、各类 marketplace、真实多平台/语音视频 adapter、A2A 协议实现、Browser/Computer use 全量、常驻观察、自动外发、L4/L5 自治、自动进化 Loop/Coordination、完整 semantic memory、完整 eval 平台、企业 managed policy、发布前法律审计)。
- M1–M3 是"增强已闭合的内核"而非"补 M0 缺的内核"(requirements/03 与 M1/M2/M3 关系表);每流的扩展位见 architecture/03 §8。

## 9. 集成契约与模块索引(防各自为战)

模块独立实施,但系统是一个整体。**咬合靠共同脊柱 + 咬合规则,不靠模块间私下约定。** 每个模块 PRD 的第 2 节必须照本节声明自己的接入。

### 9.1 共同脊柱(所有模块共用)

- **协议对象**(architecture/03 §2):RunRequest/Event/Run/Session/Turn/ToolCall/ActionIntent/ApprovalRequest/协调约束/候选/沟通对象。**跨模块只经协议对象传递,不传内部类型**。
- **事件分类**(architecture/03 §2.1.1,≈60 类):模块只能发/收这套事件;新增类型必须进 §2.1.1、不破坏 protocol。**事件是模块间唯一的事实交换与审计面**。
- **trait 边界**(architecture/03 §4.7 承重墙 + §5.6 认知层):模块实现/依赖 trait,不依赖对方实现。
- **不变量**(architecture/03 §6 / canonical §1–22):每个模块守其涉及的不变量(harness-first、执行前重查、能力门出口、候选先于稳定、单写者、schema 版本、防投毒…)。

### 9.2 四个一等接入缝(经统一接入框架 prd/05)

`ModelProvider`(模型渠道)、`ChannelAdapter`(消息渠道)、`McpServer`(MCP)、`Plugin`(插件)本质是同一种"外部能力接入"。它们都经 **prd/05 接入框架** 的公共契约:`trait + registry + 生命周期(discovery → config → enable → trust → policy → events)`。新渠道/模型/MCP/插件从这里插入,不改核心。消费者:能力基质(prd/07)、入口沟通(prd/14)、模型配置(prd/16)。

### 9.3 模块咬合规则

- **依赖单向向下**:照 architecture/03 §1 crate 依赖图;低层不依赖高层。
- **跨模块只经协议/事件/trait**:不直接调对方内部函数、不共享可变状态。
- **单写者 + 快照读**(canonical §7):event log 唯一写者是 store;认知/后台/子代理只读快照、只投候选。
- **harness 是唯一编排者**:模块不互相驱动;由 harness 按数据流(§3 / architecture/03 §3)串。
- **口径唯一**:同名对象/判断以 canonical-contract 为准,模块 PRD 不自创口径。

### 9.4 模块 PRD 索引

编号是**文档索引**;"依赖"列为 crate 级依赖(照 architecture/03 §1),**单向向下**。`harness`(03)是运行主干:它在 crate 依赖上依赖下层(04/07/08/09/10/12/15/16 的 trait),但按其 §2 用 **trait stub 先行**占位,故可早写、编号早。

| PRD | 模块 | crate | 覆盖(D#/S#) | 依赖(crate 级,照 architecture §1) |
|---|---|---|---|---|
| 01 | 总纲(本文) | — | 全局统筹 | — |
| 02 | 协议 + 事件存储 | protocol / store | D2/D19,S21 | — |
| 03 | harness + loop | harness / loop | D17,S1/S2/S19 | 02;+ 04/07/08/09/10/12/15/16 的 trait(stub 先行) |
| 04 | policy + approval + delegation | policy / approval | D10/D12,S2 | 02 |
| 05 | 接入框架(Provider/Adapter Framework) | protocol 公共 trait + 各消费者 crate | 统一接入契约(§9.2) | 02,04 |
| 06 | 记忆图 | memory | D11 底座,S8 支撑 | 02 |
| 07 | 能力基质(registry/MCP/skills/plugins) | capabilities | D3/D4/D5,S3/S4/S5 | 02,04,05 |
| 08 | 执行 backend | execution | 执行,S2/S3 | 02,04 |
| 09 | context 装配 + compaction | context | S4,Context/Memory 行 | 02,06 |
| 10 | 认知(认知地图 + 进化治理 + 撤销) | cognition | D8/D18,S9/S20 | 02,06 |
| 11 | 时间化记忆 + UserModel + AgentSelfModel | memory / cognition | D11,S8 | 06,10 |
| 12 | 协调 + 编排 + subagent | coordination | D7,S6/S11 | 02,07,10 |
| 13 | 主动 + 能力门 | cognition(Proactivity) | D6,S7/S18 | 10,11 |
| 14 | 入口 + 沟通(CLI/Gateway/Fabric) | cli / gateway / communication | D1/D16,S12–S17 | 02,03,04,05 |
| 15 | 失败 + 验证 | eval | D9/D14,S10 | 02 |
| 16 | 模型 + 配置 + ConfigDoctor | models / config | D13 | 02,05 |
| 17 | 原创 + 合规 CI/doctor | CI / doctor | D15,S22 | — |

- 认知可观测/纠偏 canonical §17 并入 prd/03 harness + prd/10 认知,不单独成篇。
- **MCP 调用边界(避免 07↔08 循环)**:capabilities(07)只做 MCP **接入**(discovery/allowlist/metadata + server 进程生命周期);MCP 工具**调用**是 `ActionIntent{backend=Mcp}`,经 harness 咽喉(过 policy)交 execution(08)`McpBackend` 执行并发 `McpCallEvent`。server stdio handle 为 protocol 类型(prd/02),经 `ExecutionPlan` 传入,故 07 不依赖 08、08 不依赖 07。`BackendKind` 亦属 protocol(prd/02)。

## 10. 模块 PRD 模板与跨切规范

### 10.1 每模块 PRD 模板(固定 11 节)

每份 `prd/02`+ 照此写,写到"照着能一字不落实现":

```text
0. 定位与承上   —— 实现哪些 requirements(D#/结构#)/architecture(§)/canonical(§)/验收(S#),声明不偏离
1. 职责与边界   —— 做什么/不做什么;映射哪个/哪些 crate;M0 范围 vs 扩展位
2. 全局位置与集成契约(防各自为战)—— 上游依赖/下游被依赖;发出&消费的事件(§2.1.1);实现&依赖的 trait(§4.7/§5.6);碰的协议对象;必守的不变量(§6)
3. 数据结构与类型 —— 具体 struct/enum(字段、类型、schema_version)
4. 接口与签名   —— trait 方法 + 内部 API + 语义 + 前后置条件
5. 行为/算法/状态机 —— 分步逻辑、状态迁移、边界与并发
6. 错误/安全/权限/审批 —— 错误映射 FailureTaxonomy;policy/approval/能力门 点;防投毒(涉及摄入时)
7. 配置与可观测 —— config key;发出的事件;DecisionTrace/provenance
8. 实施步骤与顺序 —— 有序 build steps
9. 测试 —— 单元 + 本模块参与的 S# 场景(承接总纲 §7 的 fixture/事件序列)+ 正反断言
10. 验收 —— done 判据(事件/store/错误分类可断言)
11. 原创性与合规 —— 引入外部依赖/协议兼容则记 source/license/copy_risk/implementation_rule/notice
```

第 0 节=承上;3–8=启下(实现级);**第 2 节=集成契约=防各自为战**;9–10 承接总纲全局验收。

### 10.2 跨切规范(所有模块 PRD 统一遵守)

- **错误**:统一 `FailureTaxonomy`(canonical §11);每个失败映射到分类 + 进 FailureEvidence(prd/15)。
- **事件/可观测**:每个有意义动作写 §2.1.1 事件;可解释性走 DecisionTrace + provenance(canonical §17)。
- **schema 版本**:每个 event/candidate/stable object 带 `schema_version`,读时 upcast(canonical §9)。
- **测试标准**:事件证明(非最终回答)、正反场景、三类正确性(Runtime/Governance/Learning)(见 §4)。
- **并发/安全**:单写者、快照读、候选先于稳定、防投毒(canonical §7/§15)。
- **枚举单源**:枚举/清单只活在三处宽表(canonical、architecture/03 §2.1.1、prd/01 §9.4),模块 PRD **引用不复述**;代码落地后枚举以代码为源、文档指向代码(防数量漂移)。
- **原创合规**:见 §6;fixture 自建,不抄第三方。
