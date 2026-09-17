# prd/04 policy + approval + delegation

模块实施级 PRD。归属 `policy` + `approval` crate。运行时治理:权限判定(执行前重查)、审批(Gateway-compatible)、放权(DelegationGrant + AutonomyEnvelope)。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 **D10**(Delegation/AutonomyEnvelope 运行时 enforcement)、**D12**(Approval Gateway-compatible);结构 **12**(Policy/Approval/Audit)、**13**(DelegationGrant/AutonomyEnvelope)。
- **架构**:architecture/03 §4.4(policy/approval/delegation enforce)、§4.7(`PolicyEngine`/`ApprovalBroker` 签名)。
- **口径**:canonical **§1**(Policy-at-execution、trust 不覆盖 permission)、**§2**(L0–L5 语义)、**§3**(AutonomyEnvelope 维度)。
- **验收**:主证 **S2**(审批拒绝);支撑 **S6**(AutonomyEnvelope 产出/enforce)、**S11**(subagent scope 受限)。
- **不偏离**:合并序与 deny>ask>allow = §4.4;AutonomyEnvelope 维度 = canonical §3。

## 1. 职责与边界

- **做**:`PolicyEngine`(层合并、allow/ask/deny、执行前重查)、`ApprovalBroker`(结构化审批、多入口投递、恢复)、`DelegationGrant` + `AutonomyEnvelope` 运行时 enforce。
- **不做**:不执行动作(prd/08);不决定"用什么资源"(prd/12 产 AutonomyEnvelope,本模块 enforce 它);不驱动流程(prd/03 在咽喉点**调用**本模块)。
- **crate**:`policy`、`approval`。**M0 范围**:本地 profile;managed policy(企业策略中心)= M2。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02(protocol/store)。
- **下游被依赖**:prd/03(咽喉点调 `evaluate`/`enforce_envelope`/审批)、prd/05(接入 resolve 过 policy)、prd/07(toolset 解析参考 policy 可见性)、prd/08(执行前重查)、prd/12(产 AutonomyEnvelope)、prd/14(gateway 审批投递)、subagent(prd/12)。
- **发出事件**:`ToolPolicyEvaluated`、`ApprovalRequested`、`ApprovalResolved`(经 harness 触发 `RunWaiting`/`RunResumed`、`ActionDenied`)。
- **消费**:`ToolCallProposed` / `ActionIntent`(由 harness 传入)。
- **实现 trait**:`PolicyEngine`(merge/evaluate/enforce_envelope)、`ApprovalBroker`(request/resolve/pending)。
- **必守不变量**:**Policy-at-execution**(执行前重查,模型可见≠许可);**deny>ask>allow**;**trust 不覆盖 permission**(permission 是硬边界);Delegation/AutonomyEnvelope 运行时 enforce;审批 **Gateway-compatible**(CLI/Gateway 同事件)。

## 3. 数据结构与类型

```rust
pub enum PolicyDecision { Allow, Ask(AskReason), Deny(DenyReason) }

pub struct PolicyLayer { pub source: PolicyLayerSource, pub rules: Vec<PolicyRule> }
pub enum PolicyLayerSource {                 // 合并优先序(前者不可被后者放宽)
    Managed, User, Workspace, Agent, Surface, Session, SkillToolPlugin, Hook, Final
}
pub struct PolicyRule { pub matcher: ActionMatcher, pub effect: PolicyDecision, pub scope: Scope }
pub struct EffectivePolicy { /* 合并后的判定函数 */ }
pub struct PolicyContext { pub session: SessionId, pub toolset: ToolsetRef, pub delegation: Option<DelegationGrant>, pub envelope: Option<AutonomyEnvelope> }

// 审批(Gateway-compatible;字段 = architecture §2.3)
pub struct ApprovalRequest {
    pub approval_id: ApprovalId, pub action_summary: String, pub risk_level: RiskLevel,
    pub scope: Scope, pub requested_permissions: Vec<Permission>, pub affected_resources: Vec<ResourceRef>,
    pub rollback_boundary: RollbackBoundary, pub expires_at: Timestamp, pub choices: Vec<ApprovalChoice>,
}
pub struct ApprovalGrant {
    pub approval_id: ApprovalId, pub outcome: ApprovalOutcome, pub granted_scope: GrantScope,
    pub approver: VerifiedPrincipal,       // 谁批的(AuthContext,canonical §10)
    pub bound_plan_digest: PlanDigest,     // 绑定最终不可变 ExecutionPlan 的规范化摘要——批的是"这个动作",不是"这类动作"
    pub policy_version: Version, pub tool_schema_version: Version,   // 批时所见的判定环境
    pub nonce: Nonce, pub use_by: Timestamp,                         // 一次性使用 + 过期
}
pub enum ApprovalOutcome { Granted, Denied, Expired }
pub enum GrantScope { OneShot, Session, ParamPattern(ArgMatcher) }   // 受限预授权
pub struct ApprovalTicket(ApprovalId);

// 放权(canonical §3 全维度)
pub struct DelegationGrant { pub subject: DelegationSubject, pub envelope: AutonomyEnvelope, pub granted_by: Actor, pub audit_ref: EventId }
pub struct AutonomyEnvelope {
    pub scope: Scope, pub capability: CapabilitySet, pub action_type: Vec<ActionType>,
    pub risk_limit: RiskLimit, pub approval_rule: ApprovalRule, pub budget: Budget,
    pub timebox: Timebox, pub rollback: RollbackReq,
}
pub enum EnvelopeDecision { Within, OutOfScope(Reason), NeedsApproval }
```

## 4. 接口与签名

```rust
pub trait PolicyEngine {
    fn merge(&self, layers: &[PolicyLayer]) -> EffectivePolicy;                       // deny>ask>allow
    fn evaluate(&self, ctx: &PolicyContext, intent: &ActionIntent) -> PolicyDecision; // 执行前重查
    fn enforce_envelope(&self, grant: &DelegationGrant, env: &AutonomyEnvelope, intent: &ActionIntent) -> EnvelopeDecision;
}
pub trait ApprovalBroker {
    fn request(&self, req: ApprovalRequest) -> Result<ApprovalTicket>;   // → RunWaiting
    fn resolve(&self, ticket: ApprovalTicket, grant: ApprovalGrant) -> Result<()>;
    fn pending(&self, scope: ApprovalScope) -> Vec<ApprovalRequest>;     // 多入口拉取
}
```

## 5. 行为/算法/状态机

- **merge(合并序)**:`Managed → User → Workspace → Agent → Surface → Session → SkillToolPlugin → Hook → Final`;**deny>ask>allow**;后层**只能收紧**;`hook` 可收紧、**不可绕过** managed/user 的 deny。
- **evaluate(执行前重查)**:对每个待执行 `ActionIntent` 重查 toolset scope + 参数 + 路径/网络/命令 + sandbox → `Allow/Ask/Deny`。模型构造的不可见工具名/越权参数 → `Deny`。
- **enforce_envelope**:动作是否落在 `DelegationGrant ∩ AutonomyEnvelope` 内 → `Within/OutOfScope/NeedsApproval`;**trust 高不越 permission deny**(permission 判定先行且硬)。
- **审批流**:`request` → `ApprovalRequested` + `RunWaiting`;`resolve` → `ApprovalResolved{granted|denied|expired}`;`GrantScope` 支持一次性/会话级/参数模式预授权;过期 → `Expired`(等同不批)。**Gateway-compatible**:CLI 与 Local Gateway 共享同一 `ApprovalRequest/Resolved/RunWaiting/RunResumed` 事件,`pending` 供任一入口拉取。
- **DelegationProposal 承接(放权成长闭环)**:认知层(prd/13)以 CapabilityEvidence 为据申请扩展 envelope → 作为**高影响 ApprovalRequest** 走审批 → 用户确认后落 `DelegationGrant`(带 scope/期限/预算/撤销);**申请本身零副作用**,拒绝不改变现状;trust 不覆盖 permission 不变。
- **审批-计划不可变绑定(防 TOCTOU)**:审批绑定最终 `ExecutionPlan` 的 `PlanDigest`(规范化参数/backend/scope/timeout/rollback);执行前重验 digest 一致,**任何批后变更 = 新 plan = 作废重批**;nonce 一次性、`use_by` 过期即废;`approver` 必须为 `VerifiedPrincipal`(canonical §10)。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**(canonical §11):`safety_policy_failure`(策略/审批/sandbox 边界缺失或误判)、`trust_failure`(放权过高/审批边界不清)。
- **deny/拒绝/过期 → 一律不执行**(harness 收到后走 `ActionDenied`/停止)。
- **trust 不覆盖 permission**:显式实现——permission 判定在 envelope/trust 之前,且 deny 不可被任何 trust 状态放宽。
- **审计**:每次判定/审批落事件 + provenance;DelegationGrant 记 `granted_by` + `audit_ref`。

## 7. 配置与可观测

- **config key**:`policy.profiles.{managed,user,workspace,agent,surface,session}`、`approval.default_expiry`、`approval.high_risk_always_ask`。
- **可观测**:`ToolPolicyEvaluated{decision,rule_source,reason}`、`ApprovalRequested/Resolved`;判定可解释(命中哪层哪条规则)。

## 8. 实施步骤与顺序

1. `PermissionProfile` + `PolicyRule` + `PolicyLayer` 合并(deny>ask>allow、后层只收紧)。
2. `PolicyEngine::evaluate`(执行前重查 toolset/参数/路径/网络/命令/sandbox)。
3. `ApprovalBroker`(request/resolve/pending + Gateway-compatible 事件 + GrantScope 预授权 + 过期)。
4. `DelegationGrant` + `AutonomyEnvelope` + `enforce_envelope`(trust 不覆盖 permission)。
5. 接入 harness 咽喉点(prd/03 §5.1)。

## 9. 测试

- **S2(总纲 §7.2)**:高风险动作 → `ToolPolicyEvaluated{ask}` → `ApprovalRequested` → 拒绝 → 不执行;`deny` 路径直接拒、不进审批;不可见工具名/越权参数被 `deny`。
- **S6 / S11 支撑**:AutonomyEnvelope 产出后被 `enforce_envelope` 约束;subagent 越出 envelope 的工具被拒。
- **单元**:合并优先序(managed deny 不被下层放宽);`hook` 不能绕过 user/managed deny;trust 高不越 permission deny;审批过期=不批;GrantScope 三种范围各自生效。

## 10. 验收

- 合并 deny>ask>allow 且后层只收紧;执行前重查生效;审批 Gateway-compatible 且拒/过期不执行;Delegation/AutonomyEnvelope 运行时 enforce;trust≠permission;S2 通过、S6/S11 相关断言通过。

## 11. 原创性与合规

- 权限层级、审批、能力边界是通用治理模式,非特定项目专有。
- **不复制**第三方权限 DSL、规则命名、审批 prompt 或提示文案;层级命名(managed/user/workspace/…)源自本项目 architecture/03 §4.4。
- 若引入第三方策略/校验库,记 source/license/copy_risk/implementation_rule/notice。
