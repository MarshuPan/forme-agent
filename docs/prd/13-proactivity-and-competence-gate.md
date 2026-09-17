# prd/13 主动 + 能力门(Proactivity Kernel & CompetenceGate)

模块实施级 PRD。归属 `cognition`(Proactivity 部分)。**主动内核**——授权观察→机会→价值门→能力门→提议;能力门是"高质量主动"(北极星)的守门人。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 **D6**(主动最小闭环)、结构 **15**(Proactive minimum loop)。
- **架构**:architecture/03 §5.2(`ProactivityEngine`/`CompetenceGate`)、§5.6(签名)、§2.1.1 I 组。
- **口径**:canonical **§6**(`ProactiveEmissionGuard = ValueGate ∧ CompetenceGate ∧ Policy/AutonomyEnvelope`;能力门降级)、**§18**(认知调度:前台优先)、**§22**(激活扩散、五个涌现来源=五形状 + `Commitment` 确定性来源)、**§2**(L0–L5)、**§21**(冷启动逐 scope 点亮)。
- **验收**:主证 **S7**(主动最小闭环)、**S18**(CompetenceGate 降级)。
- **不偏离**:只产 Proposal、不动手(canonical §1);主动不默认监听/外发/自治(D6)。

## 1. 职责与边界

- **做**:`ProactivityEngine`(背景 tick:observation → 机会检测(M0=定时器+简单启发;M2+=激活扩散)→ 念头 → ValueGate → proposal;五个涌现来源 + `Commitment` 确定性来源);`CompetenceGate`(scope/risk → 允许最高 InterventionLevel,读 Map/Self/Capability/Trust/Failure,**结果证据为主**);`ProactiveEmissionGuard`;`OpportunityDetector`;`ValueGate`;`AttentionBudget`(打扰治理);前瞻意图触发(消费 prd/06 `IntentionStore`)与 `AskToLearn` 引导提问;`DelegationProposal` 发射。
- **不做**:记忆图存储(prd/06,主动在其上 seed/spread)、认知语义(prd/10)、tick **调度**(prd/03 harness 托管调度器,本篇提供**用快照执行的 tick 逻辑**)、执行(harness 路由 proposal)。
- **crate**:`cognition`(Proactivity)。**M0 范围**:定时器 tick 地板;激活扩散 = M2+。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/06(记忆图——seed/spread)、prd/10(cognition——MapConfidence)、prd/11(AgentSelfModel/UserModel)、prd/15(FailureEvidence)。
- **下游被依赖**:prd/03(harness 用快照调 `ProactivityEngine::tick`、路由 proposal、出口调 `CompetenceGate::ceiling`)。
- **发出事件**:I 组 `ObservationRecorded`/`OpportunityDetected`/`ValueGateEvaluated`/`CompetenceGateEvaluated`/`ImpulseRaised`/`ProactiveProposalEmitted`/`ProactiveProposalResolved`/`ProspectiveIntentionCreated`/`ProspectiveIntentionResolved`(`ReflectionProduced` 由 prd/10 认知发,主动 tick 只触发)。
- **实现 trait**:`ProactivityEngine`、`CompetenceGate`。
- **必守不变量**:**只 emit Proposal**(真实行动回 harness,canonical §1)、**ProactiveEmissionGuard**(三者与)、**能力门降级**(canonical §6)、**主动不默认监听/外发/自治**(canonical §1)、**前台优先调度**(prd/03 §18)、**激活有界**(canonical §22)、**被拒 proposal 不自动执行、不反复打扰**(S7)。

## 3. 数据结构与类型

```rust
pub enum ImpulseSource { Gap, Change, Tension, Association, Pressure, Commitment }   // 前五 = 激活的五种涌现形状;Commitment = 前瞻意图确定性触发,不经扩散(canonical §22)
pub enum Reach { Internalize, ActIndependently, Collaborate, ExternalProxy }  // 触及梯度,含向内引力
pub struct Impulse { pub source: ImpulseSource, pub reach: Reach, pub seed: Vec<NodeId> }

pub struct Opportunity { pub seed: Vec<NodeId>, pub shape: Option<ActivationShape> } // Commitment 无 shape
pub enum ValueDecision { Worth(Value), NotWorth(Reason) }   // 打扰成本 vs 价值

pub struct CompetenceInputs {   // canonical §6 五输入
    pub map_confidence: MapConfidence, pub self_model: AgentSelfAttribute,
    pub capability_evidence: Vec<CapabilityEvidence>, pub trust: TrustProfile, pub failure: Vec<FailureEvidence>,
    pub verification_evidence: Vec<VerificationEvidence>, // 结果事件 ref；旧 payload 缺省为空
}
pub enum InterventionLevel { L0Observe, L1Suggest, L2Prepare, L3ActWithApproval, L4Autonomous, L5HighImpact }
pub struct EmissionGuard { pub value: ValueDecision, pub competence: InterventionLevel }  // 认知侧两因子;Policy/AutonomyEnvelope 第三因子由 harness 在出口 enforce(prd/03 §5.1)
pub struct AttentionBudget { pub quiet_hours: Vec<TimeWindow>, pub interrupt_rate: RatePolicy, pub urgency: UrgencyMap }  // 全局打扰治理(canonical §17);搭车投递不计打扰
pub enum Proposal { Action(ActionProposal), Communication(CommunicationProposal), Learning(LearningTask), Delegation(DelegationProposal) }
// Delegation = 以 CapabilityEvidence 为据申请扩展 envelope(prd/04 承接;高影响必须用户确认;申请零副作用)
// AskToLearn = CommunicationProposal 的一种 purpose(引导性提问,面向 owner,L1,canonical §21)
```

## 4. 接口与签名

```rust
pub trait ProactivityEngine {
    fn tick(&self, trigger: TickTrigger, snap: &CognitionSnapshot) -> Vec<Impulse>;  // 快照执行(prd/03 调)
    fn emit(&self, impulse: Impulse, guard: EmissionGuard) -> Option<Proposal>;       // 只产 proposal
}
pub trait CompetenceGate {
    fn ceiling(&self, scope: Scope, risk: Risk, ctx: &CompetenceInputs) -> InterventionLevel;  // 不足则降级
}
```

## 5. 行为/算法/状态机

- **tick(canonical §18,harness 用快照执行)**:`ObservationRecorded` → `OpportunityDetector`(**M0 = 定时器 + 简单启发/规则;M2+ = 在 prd/06 记忆图上 seed 激活扩散**、五种 `ActivationShape`,canonical §22 潮线)→ 念头 `Impulse`(触及梯度 + 向内引力:优先内化,再独立/协作/对外)→ `ValueGate`(值不值得打扰)→ `emit`。
- **emit**:认知侧组装 `EmissionGuard = ValueGate ∧ CompetenceGate` → `Proposal`(不足则**降级**);**只产 Proposal、永不动手**。完整 `ProactiveEmissionGuard` 的第三因子 **Policy/AutonomyEnvelope 由 harness 在出口 enforce**(prd/03 §5.1,Proposal 变动作时过 policy)——认知层不自评 policy。
- **CompetenceGate::ceiling**:`(scope, risk)` → 允许最高 `InterventionLevel`,读 `CompetenceInputs`(Map/Self/Capability/Trust/Failure + Verification result refs);不足降级 `L4→L3→L2→L1→L0`(canonical §6)。
- **五个涌现来源=五形状**(canonical §22):联想=汇聚、变化=重 seed、压力=目标静息上升、缺口=亮但低 confidence、张力=co-激活 Contradicts;第六来源 `Commitment` 确定性触发、无 shape。
- **被拒 proposal**:不自动执行,feedback → proactive evidence;**不反复打扰**(打扰成本);defer → 转为带条件的 `ProspectiveIntention`(议程承接,见下)。
- **前瞻意图触发(确定性,canonical §22)**:tick 先 `IntentionStore::claim_due`(原子认领 + lease,防重复履约;prd/06)→ 到期意图直接成 `Impulse{source: Commitment}`(**不经扩散、必触发、恰好一次**);触发时做时效复核(情境已变则降级/重议),仍过能力门与 harness 出口 policy;产 `ProspectiveIntentionResolved`。来源:用户承诺 / 被延后 proposal / 自生成意图 → `IntentionStore::create`(产 `ProspectiveIntentionCreated`)。
- **引导性提问 AskToLearn(canonical §21)**:Gap 激活形状指向 UserModel/CognitiveMap **自身**缺口 → `CommunicationProposal{purpose=AskToLearn}`(面向 owner,L1);受打扰预算;有用率进北极星度量(canonical §17)。**M0 的主动价值主要由它交付。**
- **投递通道与打扰治理(canonical §17)**:L1 默认**搭车投递**(附在下一次回复/摘要,零打扰、不计预算);打断式通知是例外,消耗 `AttentionBudget`(quiet hours/频控/紧急度分级);确认类走 digest 批处理。
- **AgentWorkspace 进入竞争(canonical §18)**:浮现 ≠ 被思考——念头竞争进入**有界** `AgentWorkspace`(价值 × 紧迫排序,容量硬限);**进入即提交**(被正经深思一次,落 DecisionTrace),未进入者留在记忆(可再浮现/衰减)。该投影由 cognition 从事件派生维护(非新存储);§17 可报告性与 DecisionTrace 快照皆读它。
- **冷启动(canonical §21)**:能力门让冷启动只能观察/追问;能力逐 scope 点亮,窄 scope 先能主动。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**:`proactivity_failure`(误报/打扰过度/时机不对/价值判断错)。
- **ProactiveEmissionGuard**:三门与;任一不过则降级/不发。
- **证据权重(canonical §6)**:`ceiling` 以结果证据(CapabilityEvidence/FailureEvidence/验证)为主;自评置信(MapConfidence/AgentSelfModel)只能**压低**上限、不能抬高上限(防自指循环)。
- **主动不默认**:不常驻看屏、不默认外发、不 L4/L5 默认自治(D6)。
- **只 emit Proposal**:真实行动、审批、执行都回 harness/policy。

## 7. 配置与可观测

- **config key**:`proactivity.tick.cadence`(经 prd/03)、`proactivity.value_gate.*`、`competence_gate.thresholds`(M0 固定)、激活参数(经 prd/06,build+observe+tune)。
- **可观测**:I 组事件；`ProactiveProposalEmitted` 记录最终 delivery/attention cost；能力门降级原因、失败证据、verification result refs 与所需补充信息进 trace(S18/S32)。

## 8. 实施步骤与顺序

1. `OpportunityDetector`(在 prd/06 seed 激活扩散、五形状)——M0 定时器 tick 地板。
2. 念头 `Impulse`(触及梯度 + 向内引力)。
3. `ValueGate`(打扰成本 vs 价值)。
4. `CompetenceGate::ceiling`(读五输入、降级)。
5. `ProactiveEmissionGuard` + `emit`(只 Proposal)。
6. feedback → proactive evidence。

## 9. 测试

- **S7(总纲 §7.14)**:`ObservationRecorded`→`OpportunityDetected`→`ValueGateEvaluated`→`ProactiveProposalEmitted`→`ProactiveProposalResolved`;被拒不自动执行、不反复打扰;未授权 observation 不进 stream。
- **S18(总纲 §7.15)**:ValueGate 过但 MapConfidence/AgentSelfModel 不足 → `CompetenceGateEvaluated` → 降级 `L2/L1/L0`;原因/失败证据进 trace;降级后不绕 policy/approval/AutonomyEnvelope。
- **单元**:emit 只产 Proposal;guard = 三门与;能力门降级 + 自评只压不抬;五形状 + Commitment 确定触发(到期必触发/时效复核/defer 转议程);AskToLearn 指向自身缺口且受打扰预算;搭车投递不计打扰、打断计预算;DelegationProposal 拒绝零副作用;冷启动只观察/追问。

## 10. 验收

- 主动最小闭环;ProactiveEmissionGuard;能力门降级;只 emit Proposal;前台优先(prd/03);被拒不反复;S7/S18 通过。

## 11. 原创性与合规

- 主动内核、能力门、激活扩散、触及梯度是本项目自有设计(北极星,planning + canonical §6/§22)。
- **不复制**任何第三方主动/调度代码或 prompt。
