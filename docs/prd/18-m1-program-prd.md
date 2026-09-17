# M1 真实使用体验 Program PRD

M1 实施总纲。本文把 `requirements/05-06` 与 `architecture/04` 收敛成三个有序交付波次，固定跨 crate 集成契约、实施步骤、测试和验收。M1 不补 M0 内核，不允许为了控制台或后台任务建立第二条运行链。

**实施状态（2026-07-15）**：M1-A 已完成并通过 S23-S28；M1-B 已完成并通过 S29-S33；M1-C 已完成并通过 S34-S37、M1-A/B/M0 回归与合规门。真实配置模型 golden task、18-event trace/eval report 与最终全回归已通过；证据见 `docs/acceptance/m1-acceptance-report.md`。M1 final PASS。

## 0. 定位与承上

- **需求**：`requirements/05-m1-scope-decisions.md` D20–D35。
- **验收**：`requirements/06-m1-verification-strategy.md` S23–S37。
- **架构**：`architecture/04-m1-experience-architecture.md`。
- **既有硬约束**：`canonical-contract.md` §1–24、`architecture/03` crate 图/86-event taxonomy/治理 enforce、`prd/01` 集成契约。
- **回归基线**：M0 S1–S22 + compliance doctor，任何 M1 波次不得使其退化。

## 1. 目标与边界

### 1.1 目标

1. 用 Local Gateway daemon + Web console 让用户真实控制同一套 Harness runtime。
2. 用 trace/review/manual eval 让运行结果可解释、可纠正、可评价。
3. 用 intention-backed scheduler 和 local notification 跑通可靠后台/主动工作流。
4. 用 compaction、scope search、skill/MCP/plugin progressive loading 提升长任务效率。

### 1.2 不做

- Browser/Computer Use/AppApi/remote executor。
- 真实外部消息平台、多租户、公网 Gateway。
- LLM judge、自动 promotion、自动策略/loop 进化。
- plugin marketplace、远端动态 ABI、managed policy。
- 新内部 crate 或未在 architecture/03/04 声明的依赖边。

## 2. 全局位置与集成契约

```text
CLI / Local Web / Local Notification
                |
             Gateway
                |
         Harness façade (唯一入口)
        /       |        \
   Store     Brain      Execution
 projection  owners     backends
```

### 2.1 必守不变量

- Gateway handler 不直读 SQLite，不直调 loop/eval/memory/execution。
- Scheduler 不拥有独立 job truth，只 claim event-backed intention。
- UI review 不直接修改 stable projection，只提交治理 command。
- Event stream 以 `stream_seq` 排序，cursor reconnect 不 append event。
- Notification 是 `ActionIntent`，不是绕过 policy 的 UI side effect。
- Manual eval report 不自动改变 trust、LoopSpec、CoordinationSpec 或 capability policy。
- Context compression 不得移除 unresolved approval、unknown outcome、DecisionTrace、FailureEvidence、candidate lineage 或 DoneContract。
- dynamic discovery/search/rank 只影响候选可见性，执行前仍重查 capability/policy/envelope/schema。

### 2.2 Crate ownership

| 能力 | owner | 通过 Harness 暴露给 Gateway | 禁止 |
|---|---|---|---|
| HTTP/SSE/static console/local auth | gateway | 是 | gateway -> store/eval/memory 新边 |
| run/control/session serialization | harness | 直接 owner | handler 直接 loop |
| event cursor/projection | store | read façade | timestamp 排序 |
| trace/manual eval | eval | trace/eval façade | viewer 修改 event |
| intention/lease/topic/UserModel | memory | schedule/review façade | scheduler 私有真相 |
| proactive follow-up/AttentionBudget | cognition | tick/proposal façade | cognition 执行 action |
| compaction/search | context | context hooks | 跨 scope raw dump |
| skills/MCP/plugins | capabilities | resolve façade | search 命中即授权 |
| notification delivery | execution | normal action path | 未规划直接推送 |

## 3. 协议对象与接口

所有新增稳定对象带 `SchemaVersion`，并在 `forme-protocol` 中有 round-trip/invalid-boundary tests。M1 默认不增加 EventKind。

### 3.1 Control DTO

```rust
pub struct SurfaceProfile {
    pub schema_version: SchemaVersion,
    pub surface: SurfaceRef,
    pub kind: SurfaceKind,
    pub trust: TrustTier,
    pub scope: Scope,
}

pub struct GatewayProfile {
    pub schema_version: SchemaVersion,
    pub surface: SurfaceRef,
    pub policy: PolicyProfileRef,
    pub model: ModelProfileRef,
    pub toolset: ToolsetRef,
    pub workspace: WorkspaceRef,
}

pub struct EventCursor {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub after_stream_seq: u64,
}

pub struct EventPage {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub after_stream_seq: u64,
    pub snapshot_upper_bound: u64,
    pub events: Vec<Event>,
}

pub struct ApprovalDecision {
    pub schema_version: SchemaVersion,
    pub approval_id: ApprovalId,
    pub outcome: ApprovalOutcome,
    pub approver: VerifiedPrincipal,
    pub bound_plan_digest: PlanDigest,
    pub policy_version: Version,
    pub tool_schema_version: Version,
    pub nonce: Nonce,
    pub use_by: Timestamp,
}

pub enum RunControl {
    ResolveApproval(ApprovalDecision),
    Cancel(CancelRequest),
}
```

### 3.2 Review/Eval DTO

```rust
pub struct CandidateReviewCommand {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub candidate: CandidateId,
    pub expected_state: CandidateReviewState,
    pub decision: CandidateReviewDecision,
    pub actor: Actor,
    pub evidence: Vec<EvidenceRef>,
    pub retraction: Option<CandidateRetraction>,
}

pub struct CandidateRetraction {
    pub schema_version: SchemaVersion,
    pub target: ObjectRef,
    pub lineage: LineageRef,
    pub derived_refs: Vec<ObjectRef>,
}

pub struct ManualEvalCase {
    pub schema_version: SchemaVersion,
    pub case_ref: EvalCaseRef,
    pub kind: GoldenTaskKind,
    pub request: RunRequest,
    pub workspace: WorkspaceRef,
    pub done_contract: DoneContractRef,
    pub allowed_capabilities: Vec<CapabilityRef>,
    pub policy: PolicyProfileRef,
    pub rubric: RubricRef,
    pub required_events: Vec<EventKind>,
    pub forbidden_events: Vec<EventKind>,
}

pub struct EvalProfile {
    pub schema_version: SchemaVersion,
    pub eval_ref: EvalRef,
    pub model: ModelProfileRef,
    pub policy: PolicyProfileRef,
    pub toolset: ToolsetRef,
    pub workspace: WorkspaceRef,
    pub event_schema: SchemaVersion,
    pub replay_snapshot: ReplaySnapshotRef,
}

pub struct ManualEvalReport {
    pub schema_version: SchemaVersion,
    pub eval_ref: EvalRef,
    pub case_ref: EvalCaseRef,
    pub run: RunId,
    pub trace_refs: Vec<EventId>,
    pub outcome: VerificationOutcome,
    pub rubric: RubricRef,
    pub snapshot: ReplaySnapshotRef,
    pub profile: EvalProfile,
}
```

### 3.3 Scheduler/Notification DTO

```rust
pub struct ScheduleCommand {
    pub schema_version: SchemaVersion,
    pub intention: ProspectiveIntention,
    pub session: SessionId,
    pub envelope: AutonomyEnvelope,
    pub budget: Budget,
}

pub struct ScheduleBinding {
    pub schema_version: SchemaVersion,
    pub session: SessionId,
    pub envelope: AutonomyEnvelope,
    pub budget: Budget,
}

pub struct SchedulerConfig {
    pub schema_version: SchemaVersion,
    pub tick: DurationMs,
    pub lease: DurationMs,
    pub max_claims_per_tick: u32,
}
```

`BackendKind` 增加 `Notification`；`ActionParameters` 增加结构化 notification 参数。M1-B 追加 versioned intention/claim/job/tick/recovery DTO，并为既有 proactive/intention payload 增加可回放字段。所有 enum/payload 变更必须带 protocol snapshot/legacy decode test 和 compatibility note，但不新增 EventKind。

### 3.4 Trait 契约

```rust
pub trait GatewayControl: AgentHarness {
    fn start_run(self: Arc<Self>, request: RunRequest) -> Result<RunId>;
    fn list_runs(&self) -> Result<Vec<RunSummary>>;
    fn stream_event_page(&self, cursor: EventCursor) -> Result<EventPage>;
    fn run_summary(&self, run: RunId) -> Result<RunSummary>;
    fn pending_approvals(&self, session: SessionId) -> Result<Vec<PendingApproval>>;
    fn control(&self, run: RunId, control: RunControl) -> Result<()>;
    fn trace_view(&self, run: RunId) -> Result<TraceView>;
    fn review_candidate(&self, command: CandidateReviewCommand) -> Result<()>;
}

pub trait SchedulerGatewayControl: GatewayControl {
    fn schedule(&self, command: ScheduleCommand) -> Result<IntentionId>;
    fn list_jobs(&self) -> Result<Vec<ScheduledJob>>;
}

pub trait ManualEvaluator: GatewayControl {
    fn run_case(&self, case: ManualEvalCase, profile: EvalProfile) -> Result<ManualEvalReport>;
    fn export_report(&self, eval: EvalRef) -> Result<ManualEvalReport>;
}

pub trait SchedulerService {
    fn tick(&self, now: Timestamp) -> Result<SchedulerTickReport>;
    fn cancel(&self, intention: IntentionId, actor: Actor) -> Result<()>;
    fn recover(&self, now: Timestamp) -> Result<RecoveryReport>;
}
```

`GatewayControl`/`ManualEvaluator` 的实现位于 harness façade；Gateway crate 只持有 trait object/handle。`start_run` 必须先完成校验、幂等归并和 `RunAccepted` 持久化，再返回 run id 并由 Harness 后台推进；后台启动或推进失败必须把已接受 run 收敛到可审计 terminal 状态。`SchedulerGatewayControl` 在 M1-B 激活。SchedulerService 可以由 daemon 驱动，但 claim/submit/recover 的 mutation 必须委托 harness/memory owner。

Control/review 使用 protocol 自有边界 DTO，不能把 approval/memory/cognition crate 的内部类型下压进 protocol。M1-A Web approval 只发 one-shot `ApprovalDecision`；Harness 映射为 `ApprovalGrant { granted_scope: OneShot }`，resolve 成功即恢复 approval-waiting run。通用 tool-outcome/handoff `ResumeInput` 不从 Web 暴露。owner confirm 复用 `CandidatePromoted { by=user }`，不新增 `CandidateConfirmed`；retract 复用 `RetractionEvent -> ReevaluationTaskCreated`。

## 4. M1-A：控制面与评价面

### A0 Protocol 与 contract tests

1. 添加 control/surface/cursor/review/eval primitives 和 schema versions。
2. 添加 `Notification` planned enum delta 但不注册 backend。
3. 测试 serde round-trip、unknown enum、zero/overflow cursor、scope/actor validation。
4. 确认 EventKind::ALL 仍为 86。

### A1 Harness query/control façade

1. `GatewayControl` 委托现有 submit/resume/cancel/pending approval；HTTP submit 使用 accept-then-drive 的 `start_run`，同步 CLI 语义不变。
2. 增加 run summary、cursor page、trace view 和 candidate review。
3. mutation 前比较 run/session/actor/current state/digest/scope。
4. 并发测试证明 query snapshot 与 append 不产生 torn view。

### A2 Store/trace projections

1. 增加 run/session summary projection。
2. cursor page 捕获 snapshot upper bound。
3. TraceView 折叠 M0 事件并解析 refs，不展示模型隐推理。
4. viewer/export 前后 checksum、row count 与 cursor 不变。

### A3 Gateway daemon

1. `forme-gatewayd` binary 组装与 CLI 相同 Harness environment。
2. 采用 `axum`/`tokio` 提供 loopback HTTP/SSE；blocking runtime 调用隔离。
3. 实现 local bearer secret、same-origin/CSRF、body/timeout/rate limits。
4. 实现 `/v1/runs`、events、approvals、control、trace、candidate review 与 eval endpoints；`/v1/jobs` 随 M1-B scheduler 激活。
5. 非 loopback bind 启动失败；secret 不进入 Debug/event。

### A4 Local Web Console

第一版以静态 HTML/CSS/ES modules 随 gateway 提供，不建立第二个 server/build graph。必须提供：

- run list/status 与稳定尺寸 timeline；
- live event stream/reconnect state；
- pending approvals 与 cancel/resume；
- trace/failure/verification/candidate tabs；
- candidate review modal；
- eval set/run/report view；
- loading/empty/error/disconnected/permission-denied 状态。

UI 只调用 Gateway API；模型/tool/external 文本按 untrusted data 转义，不能用 `innerHTML` 注入。

### A5 Manual Eval

1. 建立 repository-owned structured golden task format。
2. 首批覆盖 final-only、tool+approval、long-context、background/proactive 四组。
3. runner 保存 model/profile/policy/tool/schema snapshot、run id、trace refs、rubric outcome。
4. report 为派生 artifact；反馈进入 candidate/failure 时必须显式 review。

### M1-A gate

S23–S28 + S1–S22 + compliance doctor 全绿。

## 5. M1-B：后台与主动工作流

### B0 Intention scheduler

1. daemon 按配置 tick，但 `claim_due` 由 event-backed IntentionStore 完成。
2. claim 后先校验 trigger、TTL、scope、envelope、budget、foreground session。
3. 用 intention id 生成 deterministic run idempotency/intent key。
4. schedule run 仍产生 `RunAccepted{source=schedule}` 与 SessionBound。
5. terminal 后 resolve intention；defer 创建新 intention；unknown outcome 进入 manual review。
6. BackgroundProactive golden case 必须调用本 façade并评价真实 schedule run；不得直接 submit 一个 `source=Schedule` request。

### B1 Recovery 与并发

1. startup 扫描 claimed lease 与非 terminal actions。
2. 未开始且安全的任务允许 re-claim。
3. `ActionStarted` 无 terminal 的 external effect 标记 unknown，绝不自动重试。
4. 同 session 前台 run 优先，后台进入有界队列。
5. cancel/revoke/budget exhaustion 在下一 action 前重查。
6. lease 以 prior intention event generation 作为 store domain idempotency key；两个 live handle 并发 claim 时总 winner 数必须为一。

### B2 Proactive follow-up

第一批只实现：

- due Commitment reminder；
- verification fail/unverifiable 或 high-impact failure follow-up。

每次仍走 Observation → ValueGate → CompetenceGate → DecisionTrace → Proposal → Feedback。quiet hours 默认 defer/hitchhike；reject 写 suppression，不执行 action。

`CompetenceGateEvaluated.reads` 必须区分 verification result ref 与 FailureEvidenceRef；fail/unverifiable 由结果事件主导，high-impact failure 不伪装成 verification evidence。

### B3 Notification backend

1. 只支持 local notification surface。
2. proposal accept 后生成 Deliver intent 与 immutable plan。
3. policy/approval/scope/target 在执行前重查。
4. notification 内容只存 safe summary/ref；失败进入 FailureEvidence。

### M1-B gate

S29–S33 + M1-A/M0 全回归。

**验收状态（2026-07-14）**：PASS。M1-B 实现已完成；逐场景证据见 `docs/acceptance/m1-b-acceptance-report.md`。

## 6. M1-C：Context 与 Capability 效率

### C0 Automatic compaction

1. 只在 threshold/budget pressure 触发。
2. `CompactionStarted/Finished` 记录 lineage 和 preserved refs。
3. deterministic test 比较压缩前后 DoneContract/governance 语义。
4. 统计 token/context reduction，不用压缩率替代正确性。

### C1 Topic memory 与 scoped review

1. session/project search 必须明确 scope/provenance/trust。
2. topic summary 是派生 candidate，不覆盖 raw event/history。
3. UserModel conflict 进入 review；无 owner confirmation 不 promotion。

### C2 Skill 按需加载

1. 默认 context 只有 metadata。
2. bounded search/rank 返回候选及解释。
3. Coordination 选择后才 `SkillBodyLoaded`。
4. untrusted/disabled/unselected body 不可进入 context。

### C3 MCP dynamic discovery

1. 支持 refresh metadata 与 schema 延迟加载。
2. tool search 返回摘要，不暴露未授权 schema/body。
3. 选中后解析 schema 并绑定 digest。
4. call 前重查 enabled/allowlist/schema/policy/envelope。

### C4 Plugin runtime lifecycle

1. load/configure/enable/trust/activate/disable/fail/reload 状态可观察。
2. reload 先构建并验证新 contribution snapshot，再原子切换。
3. failure isolation 不终止 daemon，不留下 ghost tool/skill/MCP/hook。

### M1-C gate

S34–S37 + M1-A/B/M0 全回归。

**验收状态（2026-07-14）**：PASS。M1-C 实现已完成；逐场景证据见 `docs/acceptance/m1-c-acceptance-report.md`。此波次通过不替代 §10 的真实配置模型 M1 final 条件。

## 7. 错误、安全、权限与审批

| 失败 | 分类/结果 |
|---|---|
| HTTP schema/auth/body/rate violation | policy/config failure；请求不进入 Harness mutation。 |
| stale cursor | 显式 cursor error，可从安全 lower bound 重取，不改历史。 |
| stale candidate state | governance conflict；要求 refresh，不覆盖新状态。 |
| scheduler lease conflict | concurrency conflict；coalesce，不重复 run/action。 |
| unknown action outcome | `ActionOutcomeUnknown` + waiting/manual review。 |
| notification target/scope changed | plan digest mismatch / policy deny。 |
| compaction lineage incomplete | ContextFailure；保留原 context 或 clean abort。 |
| MCP schema 延迟加载 mismatch | tool failure + FailureEvidence；不 fallback shell。 |
| plugin reload failure | 保留旧安全 snapshot 或 disable；不部分激活。 |

## 8. 配置与可观测

实现 architecture/04 §12 的 gateway/scheduler/notification/context/capability keys。ConfigDoctor 增加：

- loopback bind 与 local auth secret；
- scheduler tick/lease/max claims；
- notification availability/permission；
- compaction threshold 与 context reserve；
- plugin/MCP runtime source 与 allowlist。

所有 finding 给出阻断性、原因和修复建议。hot reload 不得改变已绑定 run/approval/action plan 的 snapshot。

## 9. 测试策略

### 9.1 单元

- protocol schema/cursor/control/review/eval DTO。
- SSE cursor/page snapshot。
- local auth/CSRF/redaction。
- candidate compare-state review。
- intention lease/recovery/idempotency。
- notification immutable plan。
- compaction lineage。
- skill/MCP/plugin lifecycle。

### 9.2 集成

- Gateway HTTP → real Harness → SQLite event stream。
- browser-level console workflow：submit、stream、approve、cancel、trace、review。
- daemon restart with persistent store/intention lease。
- real model golden tasks；secret 不进 artifacts。

### 9.3 场景门

严格实现 `requirements/06` S23–S37 的 fixture、事件顺序、store/projection 与反向断言。新增 `tools/verify-m1.ps1` 组合 M0 gate、S23–S37、golden report schema 和 compliance doctor。

## 10. 验收

M1 完成条件：

1. M1-A/B/C 各自 gate 全绿。
2. S1–S37 全绿，无 ignored/placeholder 场景。
3. 真实模型 golden tasks 报告可追溯到 run/event snapshot。
4. harness bypass、重复后台副作用、unknown-outcome blind retry 均为 0。
5. event stream reconnect 无丢失/乱序；trace read strictly read-only。
6. 合规 fixture 与真实 upstream tree doctor PASS。
7. 生成 `docs/acceptance/m1-acceptance-report.md`。

**最终验收状态（2026-07-15）**：PASS。七项条件全部满足；真实模型 typed report 与 portable trace manifest 位于 `docs/acceptance/`，最终矩阵与事件断言见 `docs/acceptance/m1-acceptance-report.md`，离线终门由 `tools/verify-m1-final-artifacts.ps1` 固定。

## 11. 原创性与合规

- HTTP/SSE、scheduler、trace/eval、按需加载 和 plugin lifecycle 按公开协议/API 自主实现，不复制第三方源码、prompt、UI 文案、目录或 fixture。
- 引入 `tokio`/`axum` 或其他依赖前更新 dependency/borrowing records，检查 license、MSRV、Windows GNU 支持和 transitive risk。
- Web console 资产为项目原创，不复刻第三方产品布局。
- 每个波次提交前运行 compliance doctor；失败即不得进入下一波。
