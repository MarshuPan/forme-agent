# prd/12 协调 + 编排 + subagent(Coordination Kernel)

模块实施级 PRD。归属 `coordination` crate + harness 的 `Orchestrator` 壳。把目标/情境/资源/认知地图/信任/失败证据协调成行动路线;单脑多 Agent 编排。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 **D7**(最小 CoordinationReasoner)、结构 **14**(CoordinationReasoner/ResourcePlan/DoneContract/DecisionTrace)、(**13** 委派的编排侧)。
- **架构**:architecture/03 §5.3(`CoordinationReasoner`/`OrchestrationLibrary`)、§5.6(签名)、§2.4(协调约束对象);设计底稿 archive/planning/10(协调内核)、planning/15(多 Agent 编排)。
- **口径**:canonical **§1**(Coordination Kernel:产 ResourcePlan/DoneContract/AutonomyEnvelope/ExecutionRoute/DecisionTrace,不绕 harness、不覆盖 policy)、**§13**(单脑多 Agent、编排模式三层归位、模式≠大脑)。
- **验收**:主证 **S6**(行动路线)、**S11**(subagent 受限委派)。
- **不偏离**:**产约束不产分步**(模型在 loop 内规划);subagent 无独立认知(canonical §13)。

## 1. 职责与边界

- **做**:`CoordinationReasoner`(GoalFrame → 自我盘点(AgentSelfModel)→ 看缺口 → 定资源 → 产 ResourcePlan/DoneContract/AutonomyEnvelope/DecisionTrace);`OrchestrationLibrary`(适用签名匹配模式/不够则定制/有效则经 A3 沉淀 → ExecutionRoute DAG);`Orchestrator`(经 harness spawn subagent、整合结果)。
- **不做**:policy 判定(prd/04 enforce 本模块产的 AutonomyEnvelope)、执行(prd/08)、认知语义(prd/10,coordination 只经 ref 读)。
- **crate**:`coordination`(依赖 protocol/cognition/capabilities)+ harness `Orchestrator` 壳。**M0 范围**:最小 reasoner(规则/单模型调用产约束);编排进化 = M2–M3。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02(protocol/store——含 `WorkspaceCharter` 声明式约束,coordination 语义引用;区别于学习式 CognitiveMap)、prd/07(capabilities——资源清单)、prd/10(cognition——CognitiveMap/ref)。
- **下游被依赖**:prd/03(harness 协调相调 `CoordinationReasoner`;`Orchestrator` 经 harness spawn)。
- **发出事件**:J 组 `GoalFramed`/`ResourcePlanned`/`DoneContractSet`/`AutonomyEnvelopeSet`/`DecisionTraceRecorded`/`OrchestrationRouteCreated`/`SubagentSpawned`/`SubagentResultReturned`。
- **实现 trait**:`CoordinationReasoner`、`OrchestrationLibrary`。
- **必守不变量**:**产约束不产分步**(§5.3);**AutonomyEnvelope 由 prd/04 enforce**(coordination 不自 enforce、不覆盖 policy);**单脑**(subagent 无认知,canonical §13);**DecisionTrace 引用**(可解释:CognitiveMapRef/UserModelAttributeRef/AgentSelfAttributeRef/TrustProfileRef/FailureEvidenceRef);**subagent scoped + summary/result 回传 + 不写稳定层**(canonical §7/§13);**模式≠大脑**(编排模式永不作主脑/最外层循环,canonical §13)。

## 3. 数据结构与类型

```rust
pub struct GoalFrame { pub goal: Goal, pub constraints: Vec<Constraint> }
pub struct SituationModel { pub known: Vec<Fact>, pub missing: Vec<Gap> }
pub struct ResourceInventory { pub tools: Vec<CapabilityId>, pub skills: Vec<SkillId>, pub mcp: Vec<McpToolRef>, pub subagents: Vec<SubagentProfile> }

pub struct ResourcePlan { pub selected: Vec<CapabilityId>, pub rationale: Rationale }
pub struct DoneContract { pub criteria: Vec<DoneCriterion>, pub stop_conditions: Vec<StopCondition> }
pub struct DoneCriterion { pub evidence: EvidenceSelector, pub predicate: Predicate, pub tolerance: Option<Tolerance> }  // 可执行判据:证据选择器(动作结果/事件段/文件态)+ 谓词 + 容差;不可验证时显式 Unverifiable(prd/15)
pub struct DecisionTrace { pub refs: DecisionRefs, pub rationale: Rationale, pub workspace_snapshot: WorkspaceSnapshotRef }  // refs = map/user/self/trust/failure;快照 = 决策时的 AgentWorkspace(canonical §18)
// AutonomyEnvelope 见 prd/04

// 编排(canonical §13:模板 vs 实例)
pub struct OrchestrationPattern { pub spec: WorkPattern, pub signature: ApplicabilitySignature, pub fitness: Fitness }
pub struct ApplicabilitySignature { /* 6 维 + case anchors(planning/15) */ }
pub struct ExecutionRoute { pub nodes: Vec<RouteNode>, pub edges: Vec<RouteEdge> }  // 子任务 DAG
pub struct RouteNode { pub subtask: Subtask, pub role: SubagentProfile, pub resource_slice: Toolset, pub done: DoneContract }
pub struct SubagentProfile { pub role: Role, pub toolset: Toolset, pub model: ModelProfileRef, pub permission: Scope, pub budget: Budget }  // 无认知
```

## 4. 接口与签名

```rust
pub trait CoordinationReasoner {
    fn frame(&self, goal: GoalInput, ctx: &CoordinationContext) -> GoalFrame;
    fn plan(&self, frame: &GoalFrame)
        -> Result<(ResourcePlan, DoneContract, AutonomyEnvelope, DecisionTrace)>;   // 产约束
}
pub trait OrchestrationLibrary {
    fn match_pattern(&self, sig: ApplicabilitySignature) -> Option<OrchestrationPattern>;
    fn route(&self, pattern: Option<OrchestrationPattern>, frame: &GoalFrame) -> ExecutionRoute; // None=定制
    fn sediment(&self, route: &ExecutionRoute, outcome: &RouteOutcome) -> Option<CandidateId>;    // 经 prd/10 A3
}
// Orchestrator(harness 壳):spawn_subagent(route_node) 经 harness child run
```

## 5. 行为/算法/状态机

- **frame**:goal → `GoalFrame`(目标 + 约束)。
- **plan**:自我盘点(读 AgentSelfModel,prd/11)→ 看缺口(`SituationModel`)→ 定资源(读 CognitiveMap prd/10 + capabilities prd/07)→ 产 `ResourcePlan`/`DoneContract`/`AutonomyEnvelope`/`DecisionTrace`。**产约束,不产分步**——模型在 loop 内于约束下规划。
- **orchestration**:`match_pattern`(6 维适用签名)→ 命中用模式 / 不够则**定制** / 有效则经 prd/10 A3 **沉淀**;`route` 产 `ExecutionRoute`(DAG)。单 vs 多是匹配里的一步;可以输出"不进库、大脑直接处理"。
- **subagent**:`Orchestrator` 产 route → **经 harness** spawn child run(scoped toolset/model/permission/budget、无认知);结果按 summary/result 回;整合冲突 → 上抛大脑裁(canonical §8/§13)。
- **路线运行语义(M0 即写明,实现可薄)**:RouteNode 状态机 `Pending→Running→Done|Failed|Cancelled`;依赖失败策略按边声明(abort_route / continue / replan→上抛);**父取消级联子取消**;子代理预算自父 run 预算**原子预留**(Σ子 ≤ 父剩余,结束回收);文件副作用隔离(子代理独立工作区,产物经 summary/result contract 交回,不直写父工作区);重试按节点声明、以 `intent_id` 幂等键防重复副作用。
- **blocked**:无 DoneContract 或资源不可信 → **不假装已具备行动路线**(标 blocked,不进执行相)。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**:`goal_framing_failure`(目标理解错/完成标准不清/没先协商完成)、`resource_selection_failure`(选错资源)。
- **AutonomyEnvelope 由 prd/04 enforce**(coordination 只产、不 enforce、不覆盖 policy)。
- **subagent**:无认知、scoped、不写父稳定层、越权被拒(经 prd/04)。
- **模式≠大脑**:编排模式(含 13 种)是工具,大脑永远在其上决定用谁/何时用(canonical §13)。

## 7. 配置与可观测

- **config key**:`coordination.pattern_seed_library`、`coordination.signature_weights`、`coordination.reasoner_mode`(M0:规则/单模型调用)。
- **可观测**:J 组事件;`DecisionTrace` 可解释(为何用某些资源、不用另一些)。

## 8. 实施步骤与顺序

1. `GoalFrame`/`SituationModel`/`ResourceInventory`。
2. `CoordinationReasoner::plan`(自我盘点 + 缺口 + 资源选择 → 约束)。
3. `DecisionTrace`(refs)。
4. `OrchestrationLibrary`(match/定制/sediment → `ExecutionRoute`)。
5. `Orchestrator`(经 harness spawn subagent、scoped、整合)。

## 9. 测试

- **S6(总纲 §7.9)**:`GoalFramed` → `ResourcePlanned`/`DoneContractSet`/`AutonomyEnvelopeSet`/`DecisionTraceRecorded`;可解释;无 DoneContract 时 blocked。
- **S11(总纲 §7.10)**:`SubagentSpawned`(scoped)→ 越权被拒 → `SubagentResultReturned`;不写父稳定层。
- **单元**:产约束不产分步;模式匹配/定制兜底/沉淀;subagent scoped;blocked 路径。

## 10. 验收

- coordination 产约束不产分步;DecisionTrace 可解释;编排 match/定制/sediment;subagent scoped + 回传 + 不写稳定层;单脑;模式≠大脑;S6/S11 通过。

## 11. 原创性与合规

- 协调内核 + 编排库(适用签名 + 沉淀)是本项目自有设计(archive/planning/10、planning/15)。13 种模式是通用的 Agent 设计模式,其分层归位与实现为 forme 自有。
- **不复制**第三方编排框架源码、prompt、模式命名体系或 DAG 结构表达。
