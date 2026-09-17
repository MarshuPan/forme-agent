# prd/11 时间化记忆 + UserModel + AgentSelfModel

模块实施级 PRD。跨 `memory`(属性存储)+ `cognition`(语义)。用户理解与 Agent 自我理解是时间化、过程化、候选化的。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 **D11**(时间化记忆/UserModel 候选边界)、结构 **16**(UserAttributeCandidate/UserModelAttribute/ImportedHistoricalEvidence、stability/confidence/时间尺度/过程证据优先)。
- **架构**:architecture/03 §5.2(UserModel/AgentSelfModel 语义);设计底稿 archive/planning/13(时间化记忆)、archive/planning/14(AgentSelfModel)。
- **口径**:canonical **§4**(稳定性层级、时间尺度→tier 映射)、**§5**(owner + 对象职责:UserModel=用户是谁 / AgentSelfModel=会与不会 / PartnershipModel=如何互补)、**§14**(认知)、**§21**(ImportedHistoricalEvidence 当引导种子)、**§15**(未信任不塑造稳定画像)。
- **验收**:主证 **S8**(UserModel 候选 + 历史证据降权)。
- **不偏离**:属性**存储**在 memory(prd/06),**语义/提升治理**经 cognition(prd/10 `EvolutionGovernor`);过程证据优先于历史导入。

## 1. 职责与边界

- **做**:`UserModel`(目标/偏好/风险/工作方式)、`AgentSelfModel`(会/不会、可靠性、能力缺口)、`PartnershipModel`(互补/协作/纠偏/放权)、**`TrustProfile`(per-scope 信任/放权状态,本模块 owner)**;时间化:`UserAttributeCandidate → UserModelAttribute` 生命周期 + 时间尺度→tier 映射;`ImportedHistoricalEvidence` 降权;过程证据优先。
- **不做**:raw storage(prd/06)、CognitiveMap(prd/10)、提升机制本身(prd/10 `EvolutionGovernor`——本篇**产候选 + 定语义**)、proactivity(prd/13)。
- **crate**:`memory`(属性存储)+ `cognition`(语义)。**M0 范围**:候选链 + 元数据;多时间尺度稳定化 = M2。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/06(memory)、prd/10(cognition/`EvolutionGovernor`)。
- **下游被依赖**:prd/12(coordination 读 AgentSelfModel/UserModel/`TrustProfile`)、prd/13(能力门读 AgentSelfModel/`TrustProfile`;主动读 UserModel)、prd/15(digest 引用 `TrustProfileRef`)、prd/03(§17 纠偏)。
- **发出事件**:M 组 `UserAttributeCandidateCreated`/`ImportedHistoricalEvidenceRecorded`;H 组 `CandidatePromoted`/`CandidateDowngraded`(经 prd/10)。
- **实现**:UserModel/AgentSelfModel/PartnershipModel 语义访问。
- **必守不变量**:**candidate-before-promotion**(经 prd/10)、**时间尺度→tier 映射**(canonical §4)、**过程证据 > 历史导入**(D11)、**ImportedHistoricalEvidence 永不驱动稳定/trust/高影响**(D11/§15/§21)、**对象职责单一**(canonical §5)、**每聚合单写者**(§7)。

## 3. 数据结构与类型

```rust
pub struct UserModelAttribute {
    pub attribute: AttrKey, pub value: AttrValue, pub evidence: Vec<EventId>,
    pub confidence: Confidence, pub stability: StabilityTier, pub scope: Scope,
    pub first_observed_at: Timestamp, pub last_updated_at: Timestamp,
    pub conflicts: Vec<AttrRef>, pub feedback: Vec<FeedbackRef>,
}
pub struct UserAttributeCandidate { /* 同上,status=candidate */ }
pub struct AgentSelfAttribute { pub capability: CapabilityKey, pub reliability: Reliability, pub gap: Option<CapabilityGap>, pub evidence: Vec<EventId> }
pub struct PartnershipState { pub complement: Vec<ComplementFact>, pub delegation_state: DelegationState }
pub struct TrustProfile {   // per-scope 信任/放权状态;经 EvolutionGovernor(prd/10)提升,撤销触发再评估(canonical §8)
    pub scope: Scope, pub trust_level: TrustLevel, pub delegation_state: DelegationState,
    pub evidence: Vec<EventId>, pub last_updated_at: Timestamp,
}
pub struct ImportedHistoricalEvidence { pub source: SourceRef, pub low_weight: bool /* =true */, pub bootstrap_only: bool /* =true */ }

pub struct TimeScaleTierMap { /* session→session tier / recent→working / repeated→stable(canonical §4) */ }
pub enum EvidencePriority { Process, Imported }   // Process > Imported
```

## 4. 接口与签名

```rust
pub trait UserModel {
    fn query(&self, key: AttrKey, scope: Scope) -> Option<UserModelAttribute>;
    fn observe(&self, obs: UserObservation) -> Result<CandidateId>;    // → UserAttributeCandidate,经 prd/10 提升
    fn import_historical(&self, ev: ImportedHistoricalEvidence) -> Result<()>;  // 低权重、不提升
}
pub trait AgentSelfModel { fn capability(&self, k: CapabilityKey) -> Option<AgentSelfAttribute>; fn observe(&self, o: SelfObservation) -> Result<CandidateId>; }
pub trait PartnershipModel { fn state(&self, scope: Scope) -> PartnershipState; }
```

## 5. 行为/算法/状态机

- **UserAttributeCandidate**:用户明确表达偏好/工作方式 → 候选(evidence/confidence/first·last_at/stability/scope);多次**过程证据**或用户确认 → 置信升 → 经 prd/10 不对称提升。
- **ImportedHistoricalEvidence**:低权重、**仅作引导种子**(canonical §21)——给起步地图、让 forme 问更好的问题;**永不**直接生成稳定 UserModelAttribute、trust 提升、delegation grant 或高影响主动行为。
- **时间尺度→tier**:session/每日/长期 → session/working/stable(canonical §4)。
- **AgentSelfModel**:由任务/验证/失败/反馈形成,**不由初始 profile 直接稳定化**(canonical §5)。
- **PartnershipModel**:长期互补/放权状态。
- **过程证据优先**:冲突时过程证据 > 历史导入证据。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**:`learning_failure`(过度泛化)、`trust_failure`(据薄证据放权)。
- **防投毒(canonical §15/§21)**:历史导入/未信任永不稳定;provenance 闸在 prd/10 intake。
- **S8 反向**:单次对话/一次失败/历史导入不直接生成稳定属性/trust/高影响;用户可拒绝/降级/确认关键候选。

## 7. 配置与可观测

- **config key**:`user_model.time_scale.*`、`user_model.imported_weight_cap`、`self_model.reliability_window`。
- **可观测**:M/H 组事件;属性可查(evidence/confidence/时间戳/provenance,canonical §17)。

## 8. 实施步骤与顺序

1. `UserModelAttribute`/`AgentSelfAttribute`/`PartnershipState` schema + 属性存储(prd/06)。
2. `UserAttributeCandidate` 创建(evidence/confidence/时间戳/stability/scope)。
3. `ImportedHistoricalEvidence` 低权重 + 仅引导。
4. 时间尺度→tier 映射。
5. 语义访问 + 经 prd/10 `EvolutionGovernor` 提升。

## 9. 测试

- **S8(总纲 §7.11)**:`UserAttributeCandidate` 全元数据;`ImportedHistoricalEvidence{low_weight}`;多次过程证据可升;历史导入不驱动稳定/trust/高影响。
- **单元**:时间尺度→tier;单次对话不成稳定;历史导入永不驱动 delegation;过程证据 > 历史导入。

## 10. 验收

- UserModel/AgentSelfModel/PartnershipModel 语义;候选链 + 时间尺度→tier;过程 > 导入;导入永不稳定/trust/高影响;S8 通过。

## 11. 原创性与合规

- UserModel/AgentSelfModel/PartnershipModel 及过程证据优先是本项目自有设计(archive/planning/13/14)。
- **不复制**第三方 user-profile/记忆代码或 prompt;历史导入降权是 forme 特有治理。
