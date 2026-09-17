# prd/06 记忆图(Memory Graph)

模块实施级 PRD。归属 `memory` crate。**事实源**:原始记录 + 记忆图 + 激活扩散 + 记忆链候选。注意:这是**记忆层**(存/取/连),**不是认知层**(认知语义在 prd/10/11)。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 **D11**(记忆分层的 raw storage / substrate / 图部分)、结构 **6**(Context/Memory 分层的 memory 侧)、**16**(时间化记忆的**存储底座**;UserModel/AgentSelfModel 语义在 prd/11)。
- **架构**:architecture/03 §5.1(`MemoryGraph`/`MemoryProjection`/`CandidateStore`)、§5.6(签名)、§7(记忆图存储 nodes/edges)、§2.1.1 M 组。
- **口径**:canonical **§14**(记忆 vs 认知——本篇是记忆层)、**§5**(memory = 事实源 owner)、**§7**(单写者)、**§22**(激活扩散动力学)、**§19**(遗忘/巩固、scoped 检索)。
- **验收**:支撑 **S8**(UserAttributeCandidate/ImportedHistoricalEvidence 存储)、Context/Memory 矩阵行。
- **不偏离**:memory 只负责**存储与候选链**,不做认知语义/提升治理(prd/10);记忆 vs 认知界线 = canonical §14。

## 1. 职责与边界

- **做**:Raw Storage 接口;记忆图(nodes/edges + 激活操作 seed/spread/decay/surface);`MemoryProjection`(timeline/recall/summary,含 session/每日/长期切片);`CandidateStore`(记忆链候选→稳定生命周期);记忆层三类(Event/Episodic/Reflection);属性**存储**(UserModelAttribute/AgentSelfAttribute 落盘,语义在 prd/11);**前瞻性意图存储**(`IntentionStore`,canonical §22——议程对象落盘,触发消费在 prd/13)。
- **不做**:认知语义/提升治理(prd/10 `EvolutionGovernor`)、CognitiveMap(prd/10)、UserModel/AgentSelfModel 语义(prd/11)、激活扩散涌现参数的调优(build+observe+tune,不在纸上定死)。
- **crate**:`memory`(依赖 protocol/store)。**M0 范围**:session/每日/长期时间尺度切片;向量召回最小或后置;遗忘/巩固最小;热/冷分层 = M2(canonical §19)。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02(protocol/store——记忆图落在 store 上)。
- **下游被依赖**:prd/09(context 装配读 summary/recall)、prd/10(cognition 读记忆产认知候选、驱动边权强化)、prd/11(UserModel/AgentSelfModel 属性存于此)、prd/13(proactivity 在图上 seed/spread + 消费 `IntentionStore::claim_due`)。
- **发出事件**:M 组 `MemoryNodeAppended`/`MemoryEdgeAppended`/`UserAttributeCandidateCreated`/`ImportedHistoricalEvidenceRecorded`;记忆链 `CandidateCreated`。(`CognitiveMapUpdateProposed` 由 prd/10 产。)
- **消费**:事件日志(EpisodicMemory 从 events 派生)。
- **实现 trait**:`MemoryGraph`、`MemoryProjection`、`CandidateStore`(§5.1/§5.6)。
- **必守不变量**:**memory = 事实源 owner**(canonical §5);**单写者**(经 store);**candidate-before-promotion**(记忆链候选;稳定提升治理在 prd/10,本篇持有候选存储);**激活有界**(canonical §22);**记忆 ≠ 认知**(canonical §14);**防投毒**(canonical §15:未信任摄入低权重、不动稳定)。

## 3. 数据结构与类型

```rust
pub struct MemoryNode {
    pub id: NodeId, pub kind: NodeKind, pub content_ref: ContentRef,
    pub tier: StabilityTier,            // canonical §4:session/working/stable/…
    pub confidence: Confidence, pub scope: MemoryScope,
    pub resting_activation: f32,        // 静息(持久);current 为瞬时(不持久,§7 architecture)
    pub recency: Timestamp, pub provenance: Provenance,
}
pub enum NodeKind { Event, Episodic, Reflection, Fact, Attribute, Goal, CognitiveObjectRef }

pub struct MemoryEdge {
    pub from: NodeId, pub to: NodeId, pub kind: EdgeKind,
    pub weight: f32,                    // 可学(A3 强化,prd/10 驱动)
}
pub enum EdgeKind { Temporal, Causal, Association, Contradicts, PartOf, Refines }

pub enum ActivationShape {             // 五个涌现来源 = 五种形状;Commitment 不经扩散、无 shape(canonical §22)
    Association,   // 汇聚激活
    Change,        // 重新 seed
    Pressure,      // 目标静息上升
    Gap,           // 被点亮但低 confidence
    Tension,       // co-激活的 Contradicts
}
pub struct Activated { pub node: NodeId, pub level: f32 }
pub struct ActivationBudget { pub max_hops: u8, pub top_k_frontier: u16, pub per_tick: u32 }

pub struct MemoryScope { pub slice: TimeSlice, pub workspace: Option<WorkspaceRef> }  // Session/Daily/LongTerm

pub struct ProspectiveIntention {    // 前瞻性意图/议程(canonical §22):确定性触发层;存储在此,触发在 prd/13
    pub id: IntentionId,
    pub source: IntentionSource,     // UserCommitment / DeferredProposal / SelfGenerated
    pub trigger: IntentionTrigger,   // At(Timestamp) / OnEvent(EventKind) / OnCondition(ConditionRef)
    pub state: IntentionState,       // Pending / Fired / Done / Expired / Cancelled
    pub seed: ImpulseSeed, pub provenance: Provenance,
}
```

记忆链候选与属性存储(D11):`CandidateUpdate`(共享,§2.5)、`UserAttributeCandidate`、`ImportedHistoricalEvidence{ low_weight: true }`。

## 4. 接口与签名

```rust
pub trait MemoryGraph {
    fn add_node(&self, node: MemoryNode) -> Result<NodeId>;
    fn add_edge(&self, edge: MemoryEdge) -> Result<EdgeId>;
    fn seed(&self, seeds: &[NodeId], shape: ActivationShape);          // 点燃
    fn spread(&self, budget: ActivationBudget) -> Vec<Activated>;      // 扩散→衰减→浮现(有界)
    fn set_edge_weight(&self, edge: EdgeId, w: f32) -> Result<()>;     // A3 强化入口(prd/10 调);写入经 MemoryMaintenanceApplied 事件(可批量)
    fn query(&self, q: GraphQuery) -> Vec<MemoryNode>;
}
pub trait MemoryProjection {
    fn timeline(&self, scope: MemoryScope, window: TimeWindow) -> Vec<Episode>;
    fn recall(&self, cue: RecallCue, k: usize) -> Vec<MemoryRef>;      // 索引/向量 + scoped
    fn summary(&self, scope: MemoryScope) -> MemorySummary;
}
pub trait CandidateStore {
    fn create(&self, c: CandidateUpdate) -> Result<CandidateId>;
    fn list(&self, filter: CandidateFilter) -> Vec<CandidateUpdate>;
    fn transition(&self, id: CandidateId, to: CandidateState, by: Actor) -> Result<()>;  // 契约义务:必须发射对应 H 组事件(Promoted/Rejected/Downgraded/Decayed)——事件是唯一事实源
}
pub trait IntentionStore {
    fn create(&self, i: ProspectiveIntention) -> Result<IntentionId>;               // → ProspectiveIntentionCreated
    fn claim_due(&self, now: Timestamp, lease: Duration) -> Vec<ClaimedIntention>;  // 原子认领 + lease:防重复履约;崩溃后 lease 过期可再认领;resolve 幂等
    fn resolve(&self, id: IntentionId, outcome: IntentionOutcome) -> Result<()>;    // fired/done/expired/cancelled → ProspectiveIntentionResolved
}
```

## 5. 行为/算法/状态机

- **激活扩散(canonical §22)**:`seed(shape)` 置初始激活 → `spread` 沿边权 × 每跳衰减扩散(汇聚相加)→ 衰减 → 过阈值浮现 → **有界**(`max_hops` / `top_k_frontier` / `per_tick`);**无 seed 不扩散**(接 canonical §16/§18)。五来源映射到五种 `ActivationShape`。
- **边权学习**:`set_edge_weight` 供 prd/10 的 A3 在"有用浮现"后强化(联想越用越准);本篇只提供入口 + 存储,不决定强化策略。
- **遗忘/巩固(canonical §19)**:稳定节点慢衰减、易变节点快衰减;active 图有界(修剪 + 淡化)。
- **记忆链(canonical §14)**:纵向 `RawEvent→Event→Episodic→Reflection→候选→稳定`(候选存 `CandidateStore`,稳定提升由 prd/10 治理);横向 association 图。
- **EpisodicMemory 派生**:从事件日志折叠(`MemoryProjection`,可重建)。
- **recall**:索引/向量(M0 最小)+ **scoped 遍历**(只遍历相关 workspace/scope,不扫全图,canonical §19)。
- **事件覆盖划分(事件是唯一事实源,replay 必须能重建认知演化)**:**事件化**——节点/边增删(`MemoryNodeAppended/EdgeAppended`)、边权变更与巩固/修剪(批量事件 `MemoryMaintenanceApplied{deltas_ref}`,由整理 tick/A3 周期归并发射,防高频刷屏)、候选生命周期(H 组);**读时派生(不事件化)**——`current_activation`(瞬时)、懒衰减(由 `last_updated + 衰减率` 确定性计算,不物化)。replay 重建 = 事件流 + 确定性懒衰减函数。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**:`learning_failure`(来源不可靠、候选沉淀错、过度泛化)。
- **防投毒(canonical §15)**:摄入内容按 `Provenance.trust_tier`;**未信任内容最多低权重候选,永不直接动稳定认知或 UserModel**;`ImportedHistoricalEvidence` 低权重、不驱动高影响。
- **图结构投毒(canonical §15)**:攻击面不止节点内容,还含**恶意边 / 权重 / relation 冲突**(伪造 `Contradicts`、劫持关联召回)。`add_edge`/`set_edge_weight` 只采信过程证据;未信任来源不得改图结构与边权(与节点内容同等 quarantine)。
- **candidate ≠ stable**:`CandidateStore` 只到候选;稳定提升治理在 prd/10(本篇不自动提升)。
- **审计**:节点/边/候选带 provenance;撤销经 `RetractionEvent`(prd/10 触发再评估)。

## 7. 配置与可观测

- **config key**:`memory.scopes`、`memory.forgetting.{stable_rate,volatile_rate}`、`memory.activation.{max_hops,top_k,per_tick}`(涌现参数,build+observe+tune)、`memory.recall.k`。
- **可观测**:M 组事件;激活浮现结果可解释(哪些 seed、经哪些边浮现)。

## 8. 实施步骤与顺序

1. `nodes`/`edges` 存储(architecture §7 表)+ `MemoryGraph::add_*`。
2. `query` + `MemoryProjection`(timeline/recall/summary;EpisodicMemory 从 events)。
3. 激活操作 `seed/spread/decay/surface`(有界)。
4. `set_edge_weight` 强化入口 + 遗忘/巩固(慢/快衰减)。
5. `CandidateStore`(记忆链候选)+ `UserAttributeCandidate`/`ImportedHistoricalEvidence` 存储。

## 9. 测试

- **S8 存储侧(总纲 §7.11 支撑)**:`UserAttributeCandidate` 带 evidence/confidence/时间戳/stability/scope 落盘;`ImportedHistoricalEvidence{low_weight}`。
- **Context/Memory 矩阵行**:session/每日/长期切片可取;candidate≠stable。
- **单元**:seed/spread 有界(max_hops/top_k 生效)、无 seed 不扩散;边权更新;衰减(稳定慢/易变快);recall scoped(不扫全图);EpisodicMemory 从 events 重建一致;防投毒——未信任摄入落低权重候选、不动稳定。

## 10. 验收

- memory = 事实源;图 + 激活(有界);记忆链候选;candidate≠stable;scoped recall + 遗忘;防投毒降权;S8 存储侧 + Context/Memory 行通过。

## 11. 原创性与合规

- 记忆图、激活扩散、episodic/semantic 区分是通用认知科学/CS 模式。
- 常规 agent 的 session/每日/长期 memory 只到**记忆层**(存+取),forme 在其上有认知层(prd/10/11)。
- 引入向量/图存储第三方库时,记 source/license/copy_risk/implementation_rule/notice。
