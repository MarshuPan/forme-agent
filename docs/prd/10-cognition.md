# prd/10 认知(Cognition:认知地图 + 进化治理 + 撤销)

模块实施级 PRD。归属 `cognition` crate。**认知层核心**——语义 owner:认知地图、进化治理(A3,稳定认知写入唯一通道)、撤销/派生失效。区别于 memory(prd/06 = 事实源)。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 **D8**(CognitiveMapUpdateProposal)、**D18**(撤销/派生失效);结构 **17**(CognitiveMapUpdateProposal 候选生成)。
- **架构**:architecture/03 §5.2(`CognitiveMapStore`/`EvolutionGovernor`)、§5.6(签名)、§5.4(接缝)。
- **口径**:canonical **§4**(候选→提升、稳定性层级、**固定 vs 可进化的谨慎不对称**)、**§5**(cognition = 语义 owner)、**§8**(撤销/派生失效)、**§14**(认知层)、**§17**(纠偏喂 A3)、**§15**(防投毒:未信任永不直接提升)。
- **验收**:主证 **S9**(CognitiveMap 候选)、**S20**(撤销/派生失效)。
- **不偏离**:认知 = 派生的、受治理的、会进化的(canonical §14);稳定写入唯一通道 = `EvolutionGovernor`。

## 1. 职责与边界

- **做**:`CognitiveMapStore`(JudgmentFrame/QualityModel/BlindSpotModel + MapConfidence);`EvolutionGovernor`(A3:候选进 → 闸 → 自动提升/用户确认/拒绝/降级/回滚/衰减——**所有稳定认知写入的唯一通道**);撤销/派生失效;`ReflectionProduced → CognitiveMapUpdateProposal`。
- **不做**:memory 存储(prd/06,cognition **读**它、候选存那里);UserModel/AgentSelfModel 的语义(prd/11,但其提升经本模块 `EvolutionGovernor`);proactivity/能力门(prd/13);coordination(prd/12);不执行动作。
- **crate**:`cognition`(依赖 protocol/memory/store)。**M0 范围**:候选生成 + 用户确认提升;replay/eval 自动提升 = M3。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02、prd/06(memory)。
- **下游被依赖**:prd/11(UserModel/AgentSelfModel 提升经 `EvolutionGovernor`)、prd/12(coordination 读 CognitiveMap ref)、prd/13(主动读 MapConfidence;能力门读 FailureEvidence 等)、prd/03(harness 把候选路由进 `EvolutionGovernor`、§17 纠偏喂 A3)。
- **发出事件**:H 组(`CandidateCreated`/`CandidateConflictDetected`/`CandidatePromoted`/`CandidateRejected`/`CandidateDowngraded`/`CandidateDecayed`/`RetractionEvent`/`RevocationEvent`/`ReevaluationTaskCreated`)、I 组(`ReflectionProduced`)、M 组(`CognitiveMapUpdateProposed`)。
- **实现 trait**:`CognitiveMapStore`、`EvolutionGovernor`。
- **必守不变量**:**candidate-before-promotion**(`EvolutionGovernor` 是稳定认知写入唯一入口,canonical §4)、**stable CognitiveMap 不自动变**(S9)、**撤销→派生再评估、不删历史**(canonical §8,S20)、**谨慎不对称**(更谨慎的知识易升、更自信/自治的难升,canonical §4)、**认知 = 语义 owner**(canonical §5/§14)、**纠偏是强证据喂 A3**(canonical §17)、**防投毒**(未信任 provenance 永不自动提升,canonical §15)。

## 3. 数据结构与类型

```rust
pub struct CognitiveMap { pub nodes: Vec<MapNode>, pub edges: Vec<MapEdge>,
    pub frames: Vec<JudgmentFrame>, pub quality: QualityModel, pub blindspots: BlindSpotModel }
pub struct MapConfidence { pub scope: MapScope, pub value: Confidence }   // scope 化
pub struct CognitiveMapView { /* 只读投影 */ }
pub struct CognitiveMapUpdateProposal { pub kind: MapUpdateKind, pub confidence: Confidence /* low */, pub evidence: Vec<EventId> }
pub enum MapUpdateKind { Frame, Quality, BlindSpot, ResourceRelation }

pub enum GovernanceDecision { Promote, Confirm(UserPrompt), Reject(Reason), Downgrade, Rollback, Decay }
pub struct IntakeGates {           // A3 的闸
    pub evidence: EvidenceCheck,   // 证据充分性(多证据/多时间点)
    pub conflict: ConflictCheck,   // 与既有冲突(保留冲突,不覆盖,canonical §8)
    pub impact: ImpactCheck,       // 影响面(高影响需用户确认)
    pub provenance: ProvenanceCheck, // 信任分级(未信任不自动提升,canonical §15)
}
pub struct PromotionAsymmetry {    // canonical §4:谨慎易升、自信/自治难升
    pub caution_increasing: Threshold,       // 低门槛(盲区/风险提示)
    pub confidence_autonomy_increasing: Threshold,  // 高门槛(多证据+多时点+高影响需确认)
}

// 撤销/派生(canonical §8)
pub struct Lineage { pub evidence_to_object: Vec<(EventId, ObjectRef)>, pub object_to_derived: Vec<(ObjectRef, ObjectRef)> }
pub struct ReevaluationTask { pub derived: ObjectRef, pub trigger: RetractionRef }
```

## 4. 接口与签名

```rust
pub trait CognitiveMapStore {
    fn read(&self, scope: MapScope) -> CognitiveMapView;
    fn confidence(&self, scope: MapScope) -> MapConfidence;
    fn propose_update(&self, p: CognitiveMapUpdateProposal) -> Result<CandidateId>;  // 低置信候选
}
pub trait EvolutionGovernor {                                        // 稳定写入唯一通道
    fn intake(&self, c: CandidateUpdate) -> GovernanceDecision;      // 过 IntakeGates + 不对称阈值
    fn on_retraction(&self, e: RetractionEvent) -> Vec<ReevaluationTask>;
    fn decay(&self, tick: Tick) -> Vec<CandidateId>;
}
```

## 5. 行为/算法/状态机

- **EvolutionGovernor::intake(A3)**:候选 → `IntakeGates`(证据/冲突/影响/provenance)→ 按 `PromotionAsymmetry` 定阈值:
  - **谨慎增(盲区、风险标记、更保守的判断)→ 低门槛易升**;
  - **自信/自治增(更强判断、放权、默认行为)→ 高门槛**:需多证据 + 多时间点,高影响需用户确认;
  - **未信任 provenance → 永不自动提升**(canonical §15),最多低权重候选。
- **CognitiveMapUpdateProposal**:`ReflectionProduced`(trace + 用户反馈 + failure evidence)→ 低置信 proposal → `CandidateCreated`;**stable map 不自动变**。
- **撤销/派生失效(canonical §8,S20)**:`RetractionEvent` → 经 `object→derived` 血缘找引用者 → `ReevaluationTaskCreated` → 每个派生产 invalidation/downgrade 候选;**不自动删历史事实**;停止把被撤属性当有效稳定认知用。
- **decay**:稳定慢衰减、易变快衰减;久未用候选衰减。
- **纠偏(§17)**:用户纠正 = 强证据 → 经 intake 触发 downgrade/rollback。
- **边权强化**:有用浮现后调 prd/06 `MemoryGraph::set_edge_weight`(联想越用越准,A3 驱动)。
- **主动失效自检(canonical §8/§11)**:除撤销外,`EvolutionGovernor` 定期扫稳定认知——证据老化 / 新证据矛盾 / 长期未复现 → 主动产 re-evaluation 或 downgrade 候选,记 `memory_misevolution`(prd/15);仍走候选、不自动删。防"记忆静默漂移到失效仍被当有效用"。
- **可言说 = 可携带(canonical §23)**:认知对象保持**可注入上下文的自然语言反思句式**(JudgmentFrame/Principle/Strategy 皆为可复述文本,不做不可复述的黑盒结构)。理由:①可言说表征才能装载模型工作空间并因果驱动行为(counterfactual-reflection 实证——塑形"反思时会说的话"即塑形沉默推理);②装载通道会变(今天注入上下文,明天可能是厂商 memory/微调 API),**内容资产不变**;③换模型存活。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**:`cognitive_map_failure`(用错判断框架)、`learning_failure`(候选沉淀错、过度泛化)。
- **candidate-before-promotion**:稳定只经 `EvolutionGovernor`;其余只 emit 候选。
- **防投毒(canonical §15)**:未信任内容永不直接提升稳定认知/UserModel;provenance 闸在 intake。
- **不对称谨慎**:自信/自治类变更保守(高门槛),避免慢性漂移(canonical §4/§15)。

## 7. 配置与可观测

- **config key**:`cognition.promotion.{caution_threshold,confidence_threshold}`(不对称)、`cognition.decay.{stable,volatile}`、`cognition.confirm_load.batch`(canonical §17,批处理确认避免疲劳)。
- **可观测**:H/I/M 组事件;`DecisionTrace` 引用 `CognitiveMapRef`;提升/降级可解释(过哪些闸、为何)。

## 8. 实施步骤与顺序

1. `CognitiveMapStore`(map/frames/quality/blindspot + MapConfidence)于 memory/store。
2. `EvolutionGovernor::intake`(IntakeGates + 不对称阈值 + provenance 闸)。
3. `CognitiveMapUpdateProposal`(由 `ReflectionProduced` 产低置信候选)。
4. 撤销/派生:`Lineage` + `on_retraction` → 再评估候选。
5. `decay` + 边权强化(驱动 prd/06)。
6. §17 纠偏 intake(纠正 = 强证据)。

## 9. 测试

- **S9(总纲 §7.12)**:`ReflectionProduced` → `CognitiveMapUpdateProposed`(低置信)→ `CandidateCreated`;stable map 不自动变;用户可拒绝/降级。
- **S20(总纲 §7.13)**:`RetractionEvent` → `ReevaluationTaskCreated` → 派生 downgrade 候选;不删历史;停止使用被撤属性。
- **单元**:intake 各闸;不对称(谨慎低门槛、自信/自治高门槛);未信任永不自动提升;decay(稳定慢/易变快);撤销血缘遍历找全派生。

## 10. 验收

- 认知 = 语义 owner;candidate-before-promotion(唯一通道);stable map 不自动变;撤销→派生再评估(不删);谨慎不对称提升;防投毒;S9/S20 通过。

## 11. 原创性与合规

- 认知地图、候选→提升、撤销/派生失效、谨慎不对称是本项目自有设计(archive/planning/06/11 + canonical §4/§8)。
- 常规 agent 只到记忆层,无此认知治理。**不复制**任何第三方代码/prompt/结构。
