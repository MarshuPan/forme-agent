# prd/02 协议 + 事件存储(Protocol & Event Store)

模块实施级 PRD。归属 `protocol` + `store` crate。它是所有模块咬合的**脊柱**——协议对象、事件日志、schema 版本与 replay。照 `prd/01 §10` 模板写。

## 0. 定位与承上

- **实现**:requirements/03 **D2**(SQLite/FTS + append-only event log)、**D19**(schema 版本 + replay);结构 **3**(Run/Session/Turn/Event 协议)、**5**(SQLite/FTS + event log)。
- **架构**:architecture/03 §2(协议对象)、§2.1.1(完整事件分类)、§4.3 + §7(store)、§4.7 `EventStore` 签名。
- **口径**:canonical **§7**(并发/单写者)、**§9**(schema/replay)、§1(事件审计)。
- **验收**:主证 **S21**(schema/replay);支撑 **S1**(同构事件序列)、**S19**(单写者/投影派生)的事件面。
- **不偏离**:协议对象与事件名以 architecture/03 §2 / §2.1.1 为准;同名对象口径以 canonical 为准;不新增未在架构中的协议类型。

## 1. 职责与边界

- **做**:
  - `protocol` crate:定义**共享协议类型**——事件信封、生命周期对象、共享原语(Ids/Provenance/schema_version)、以及所有域对象的**编译期宿主**。
  - `store` crate:append-only event log(单写者)+ 投影(从事件派生、可重建)+ SQLite/FTS + schema 版本/迁移 + replay。
- **不做**:业务逻辑(harness/loop 在 prd/03);记忆图存储语义(prd/06,虽复用 store 的底层);policy/cognition 语义。
- **crate**:`protocol`、`store`。
- **对象字段权属**:`protocol` 是所有协议 struct 的**编译期宿主**,但**域对象的字段以其所属模块 PRD 为准**(如 `ActionIntent`→prd/08、`ApprovalRequest`→prd/04、协调约束→prd/12、候选→prd/10)。本篇定死**信封 + 生命周期对象 + 共享原语 + 事件枚举**;域对象在此声明占位、字段在其模块 PRD 定稿(不各自为战:字段一旦定稿即回填 protocol,全局唯一)。
- **M0 范围**:SQLite 单文件;向量召回后置;热/冷分层、多设备同步 = M2(canonical §19)。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:`protocol` 无依赖;`store` 仅依赖 `protocol`。
- **下游被依赖**:**所有模块**(它是脊柱)。
- **发出事件**:store **不产生业务事件**;它是事件的**唯一写入路径**与事实源。
- **消费事件**:全部(它存所有 `EventKind`)。
- **实现 trait**:`EventStore`(architecture/03 §4.7)、`Projection`。
- **owns 协议对象**:`Event` 信封、`EventKind`、`Run/Session/Turn`、`RunRequest/RunResult`、`Provenance`、`SchemaVersion`、各 `*Id`、`Workspace`/`WorkspaceCharter`(存储侧;charter 语义在 prd/12);并**宿主**(非字段权属)其余域对象(§1)。
- **必守不变量**:单写者(canonical §7);append-only 不可变;每对象带 `schema_version`(§9);事件是模块间唯一事实交换与审计面(§1);跨模块只经协议对象传递。

## 3. 数据结构与类型

### 3.1 共享原语

```rust
// 单调可排序 id(建议 ULID:含时间前缀,便于 append 顺序与游标)
pub struct EventId(Ulid);
pub struct RunId(Ulid);
pub struct SessionId(Ulid);
pub struct TurnId(Ulid);

pub struct SchemaVersion(pub u32);          // 每类 payload 独立版本线

pub struct Provenance {
    pub source: Source,                     // 事实来源(接 canonical §15 信任分级)
    pub actor: Actor,                       // owner / agent / subagent / external / system
    pub trust_tier: TrustTier,              // owner_input > verified_process > approved_source > untrusted
    pub caused_by: Option<EventId>,         // 因果链(可选)
}

pub enum Source { UserTurn, ProactiveJob, Schedule, Subagent, Communication, Internal }
pub enum Actor  { Owner, Agent, Subagent(RunId), External(ParticipantId), System }
pub enum TrustTier { OwnerInput, VerifiedProcess, ApprovedSource, Untrusted }
```

### 3.2 事件信封(事实单位)

```rust
pub struct Event {
    pub event_id: EventId,
    pub run_id: RunId,
    pub stream_seq: u64,                    // run 内单调序,append 时由单写者赋值——排序权威(ULID/ts 仅信息性,不赌时钟)
    pub turn_id: Option<TurnId>,            // 生命周期外的事件可无 turn
    pub kind: EventKind,                    // 判别式:决定 payload 变体
    pub payload: EventPayload,              // 与 kind 对应的强类型 payload
    pub schema_version: SchemaVersion,      // 本事件 payload 的版本
    pub ts: Timestamp,                      // 单调 + 墙钟
    pub provenance: Provenance,
}
```

`EventKind` = architecture/03 §2.1.1 的 ~60 类判别式,分 14 组:

```rust
pub enum EventKind {
    // A Run/Session:RunAccepted, SessionBound, RunComplete, RunAborted, RunFailed, RunLimited, RunWaiting, RunResumed
    // B Turn/Context:TurnStarted, TurnComplete, ContextBuildStarted, ContextBuildFinished, CompactionStarted, CompactionFinished
    // C Model:ModelCallStarted, ModelCallDelta, ModelCallFinished, OutputClassified
    // D Tool/Policy/Approval:ToolCallProposed, ToolPolicyEvaluated, ApprovalRequested, ApprovalResolved, HandoffRequested, HandoffResolved
    // E Action:ActionPlanned, ActionStarted, ActionOutputDelta, ActionCompleted, ActionFailed, ActionDenied, ActionCancelled, ActionOutcomeUnknown
    // F Verification:VerificationStarted, VerificationFinished
    // G Failure:FailureEvidenceRecorded, FailureDigestUpdated
    // H Candidate/撤销:CandidateCreated, CandidateConflictDetected, CandidatePromoted, CandidateRejected, CandidateDowngraded, CandidateDecayed, RetractionEvent, RevocationEvent, ReevaluationTaskCreated
    // I Cognitive/Proactive:ObservationRecorded, OpportunityDetected, ValueGateEvaluated, CompetenceGateEvaluated, ImpulseRaised, ReflectionProduced, ProactiveProposalEmitted, ProactiveProposalResolved, ProspectiveIntentionCreated, ProspectiveIntentionResolved
    // J Coordination:GoalFramed, ResourcePlanned, DoneContractSet, AutonomyEnvelopeSet, DecisionTraceRecorded, OrchestrationRouteCreated, SubagentSpawned, SubagentResultReturned
    // K Capability:CapabilityIndexed, ToolsetResolved, McpDiscovered, McpCallEvent, SkillMetadataExposed, SkillBodyLoaded, PluginContributionRegistered, PluginToggled, CapabilityEvidenceRecorded
    // L Communication:CommunicationEventReceived, CommunicationSessionOpened, CommunicationSessionTerminated, ExternalCommunicationGranted, DisclosurePolicyApplied, CommunicationProposalEmitted
    // M Memory:MemoryNodeAppended, MemoryEdgeAppended, MemoryMaintenanceApplied, UserAttributeCandidateCreated, ImportedHistoricalEvidenceRecorded, CognitiveMapUpdateProposed
    // N Config/Compliance:ConfigDoctorReport, ComplianceCheckResult
    RunAccepted, SessionBound, /* … 全量见 §2.1.1 … */ ComplianceCheckResult,
}
```

`EventPayload` 与 `EventKind` 一一对应(每变体一个强类型 struct,payload struct 字段以其**所属模块 PRD** 定稿,本篇给信封契约):

```rust
pub enum EventPayload {
    RunAccepted { source: Source, session_ref: SessionId, input_ref: InputRef },
    SessionBound { policy_profile: PolicyProfileRef, model_profile: ModelProfileRef, toolset_ref: ToolsetRef, workspace: WorkspaceRef },
    /* … 每个 EventKind 一个变体;payload 字段见 §2.1.1「关键 payload」+ 各模块 PRD … */
}
```

### 3.3 生命周期对象

```rust
pub struct Run     { pub run_id: RunId, pub source: Source, pub status: RunStatus, pub budget: Budget, pub stop_reason: Option<StopReason>, pub result_ref: Option<EventId> }
pub struct Session { pub session_id: SessionId, pub workspace: WorkspaceRef, pub agent_profile: AgentProfileRef, pub policy_profile: PolicyProfileRef, pub model_profile: ModelProfileRef, pub memory_scope: MemoryScope }
pub struct Turn    { pub turn_id: TurnId, pub run_id: RunId, pub index: u32 }
pub struct RunRequest { pub source: Source, pub session: SessionRef, pub agent_profile: AgentProfileRef, pub input: RunInput, pub budget: Option<Budget>, pub idempotency_key: Option<IdemKey> }  // 同键重复提交返回原 run_id,不开新 run
pub struct RunResult  { pub status: RunStatus, pub stop_reason: StopReason, pub outputs: Vec<OutputRef>, pub evidence_refs: Vec<EventId> }
pub enum RunStatus { Accepted, Running, Waiting, Complete, Aborted, Failed, Limited }

pub struct Workspace { pub workspace_id: WorkspaceId, pub charter: Option<WorkspaceCharter> }  // 隔离 scope(canonical §13);store 持有,WorkspaceRef 指向它
pub struct WorkspaceCharter { pub constraints: Vec<Constraint> }  // 声明式 charter(≠学习式 CognitiveMap);store 存、prd/12 语义引用
```

### 3.4 投影与 replay 对象

```rust
pub struct ReplayReport {
    pub run: RunId,
    pub schema_snapshot: SchemaSnapshot,     // 所用 schema/policy/loop/model/tool-schema version
    pub diff_vs_current: Vec<ProjectionDiff>,// replay 与当前 projection 的差异(不覆盖历史)
}
pub struct SchemaSnapshot { pub schema: BTreeMap<PayloadType, SchemaVersion>, pub policy_version: Version, pub loop_version: Version, pub model_profile: ModelProfileRef, pub tool_schema: Version }
```

### 3.5 命令、聚合版本与幂等边界

`EventStore::append` 仍是冻结的底层事件接口；V1 生产 mutation 必须通过 store-owned 的 typed command facade，不能让各模块自行拼 SQL 或直接改 projection。这里的 command facade 是实现边界，不是要新增一套与既有 `VersionedEventStore` 平行的 public trait。

生产 command 至少携带以下语义字段（字段名是概念 DTO，具体 command 可按聚合拆分）：

```rust
pub struct MutationCommand {
    pub aggregate: RunId,
    pub expected: AggregateVersion, // 复用 protocol::AggregateVersion
    pub idempotency_key: IdempotencyKey,
    pub event: Event,
}

pub struct MutationReceipt {
    pub result: ExpectedAppend, // 复用 protocol::ExpectedAppend
    pub replay_digest: SchemaDigest,
}
```

实现必须把该边界落到既有 `VersionedEventStore::append_expected(event, expected)` 与对应的领域幂等 ledger；不得引入 `ExpectedVersion`、`CommandId` 或 `VersionedMutationStore` 作为第二套协议。`AggregateVersion` 是带 `schema_version + aggregate + value` 的 protocol 对象，`ExpectedAppend` 是既有的提交结果。

`idempotency_key` 用于重复 command 归并，`event_id` 用于事件重放去重，动作自身使用 `ActionIntent.intent_id`，聚合写入使用 `AggregateVersion` 的 expected value/CAS。四者不能互相替代。首次写入允许 aggregate version 为 `0`；陈旧 expected version、同键异语义和 event 与 aggregate 不匹配都必须零事件、零 projection 推进并返回可分类 conflict。

### 3.6 ApprovalRequested additive 载荷与持久消费账本

`ApprovalRequestedPayload` 在保留历史事件可读的前提下 additive 增加：`plan_digest`、`normalized_action_digest`、`policy_version`、`tool_schema_version`。旧事件缺少这些字段时只能用于历史展示/replay，**不得**被恢复路径解释成仍可执行的批准依据；新产生的审批事件必须四项齐全，并与 `affected_resources` 一起通过 payload 校验。

生产 one-shot approval 的消费事实属于 authority companion metadata，不新增 `EventKind`，也不由 `approval` crate 的进程内集合充当跨重启事实源：

```rust
pub struct ApprovalConsumptionClaim {
    pub schema_version: SchemaVersion,
    pub approval_id: ApprovalId,
    pub approver: VerifiedPrincipal,
    pub nonce: Nonce,
    pub plan_digest: PlanDigest,
    pub normalized_action_digest: SchemaDigest,
    pub affected_resources: Vec<ResourceRef>,
    pub policy_version: Version,
    pub tool_schema_version: Version,
    pub now: Timestamp,
}

pub trait ApprovalConsumptionLedger {
    /// true = 本次首次 claim；false = 完全同义的已消费重放；
    /// 同 nonce 异义复用、authority fence 失效或 binding 不完整均返回 Err。
    fn claim_approval(&self, claim: &ApprovalConsumptionClaim) -> Result<bool>;
}
```

该 trait 由持有有效 `AuthorityLeaseGuard` 的 store companion 实现。调用时在一个 SQLite `IMMEDIATE` transaction 内验证 configured owner/store/process nonce 的活动 writer lease，再按 canonical tuple 计算 `semantic_digest` 并写入：

```sql
CREATE TABLE approval_consumptions (
  nonce                    TEXT PRIMARY KEY,
  owner_digest             TEXT NOT NULL,
  store_identity           TEXT NOT NULL,
  approval_id              TEXT NOT NULL,
  plan_digest              TEXT NOT NULL,
  semantic_digest          TEXT NOT NULL,
  claimed_at               INTEGER NOT NULL
);
```

`semantic_digest` 至少覆盖 approver、normalized action digest、排序去重后的 affected resources、policy version 与 tool-schema version。首次合法 claim 返回 `true`；同 nonce + 同 approval/plan/semantic digest 重放返回 `false`；同 nonce 的任一异义字段必须 conflict。claim 提交后即使 driver 失败、进程崩溃或结果 Unknown，也不得删除或释放该行。

## 4. 接口与签名

```rust
pub trait EventStore {
    /// 唯一写入路径。单写者:实现内部串行化 append。
    /// 前置:event.schema_version 已是当前版本;event_id 唯一。
    /// 后置:事件持久化 + 索引;返回 EventId。append 不可变、不覆盖。
    /// 顺序:append 由 authority 赋值 aggregate/run 内单调 stream_seq(排序权威);幂等:event_id 去重 + command/action 由各自幂等键处理。
    fn append(&self, event: Event) -> Result<EventId>;

    /// 按 authority stream_seq 读事件；ULID/ts 只作信息字段，不能排序裁决。
    fn read_run(&self, run: RunId) -> EventCursor;

    /// 从事件派生投影(可重建)。P: Projection。
    fn project<P: Projection>(&self, scope: ProjectionScope) -> Result<P::State>;

    /// replay:按 SchemaSnapshot 读事件、读时 upcast 旧版本,产出 ReplayReport(含与当前 projection 的 diff)。不覆盖历史事件。
    fn replay(&self, run: RunId, at: SchemaSnapshot) -> Result<ReplayReport>;
}

pub trait Projection {
    type State;
    fn empty() -> Self::State;
    fn apply(state: &mut Self::State, event: &Event);   // fold:纯函数、可重放
}

pub struct EventCursor { /* 惰性、有序、可分页 */ }
```

- **upcast 注册**:`fn register_upcaster(payload_type, from: SchemaVersion, to: SchemaVersion, f: Upcaster)`;读时若 `event.schema_version < current` 链式 upcast。

## 5. 行为/算法/状态机

- **append(单写者)**:校验 schema_version 为当前 → 校验 event_id 唯一(去重) → 序列化 payload → **单写者串行化**(store 级写锁 / 单写线程)→ 事务写 `events` 表 + 更新 FTS/索引 → 返回 EventId。整条原子:失败回滚,不留半条。
- **typed mutation**:认证后的 command → 规范化语义字节 → 查幂等键 → expected-version/CAS → authority 分配 `stream_seq` → append event → 同一事务更新 aggregate version 与 projection/ledger → 返回 `MutationReceipt`。重复同义 command 返回原 receipt；同键异义、版本陈旧或 payload aggregate 不匹配时 conflict，不能先写后补偿。
- **投影派生**:`project` = `read_run` 折叠 `Projection::apply`;投影是缓存,权威永远是事件;可丢弃重建。
- **replay**:读事件(按 `SchemaSnapshot`)→ 旧版本读时 upcast → 折叠出 replay 投影 → 与当前 projection 比对产 `diff_vs_current`。**只报差异、不改历史事件**。
- **schema upcast**:读路径遇低版本 payload,链式套用注册的 upcaster 到当前版本;breaking change 必须有 migration note,否则读路径报错(不静默改语义)。
- **单写者与并发**:store 保证**单一 append 串行**;per-session run 串行由 harness(prd/03)排队,但稳定层写入的串行性由本模块的单写者保证(canonical §7)。M0 单进程 + 单写者是实现下界；V1 production aggregate mutation 必须使用既有 `VersionedEventStore` 的 expected-version/幂等语义，即使当前部署仍是单进程，也不能把进程锁当作协议。
- **事件唯一事实源**:所有稳定状态、候选 transition、可影响 replay 的 graph edge/weight/maintenance 变化都只能由 typed mutation 产生事件并在同一事务 fold；projection/cache/metric 删除后必须能从事件重建。瞬时 activation 可由 snapshot + 确定性函数读时派生。
- **审批一次性消费**:最终 policy/capability/envelope/competence/owner/schema/plan 重查全部通过后，harness 才调用 `ApprovalConsumptionLedger::claim_approval`；该 claim 与 active authority writer fence 在同一 SQLite `IMMEDIATE` transaction 校验。claim 失败、同 nonce 异义冲突或同义重放均必须发生在 `ActionStarted`/driver 之前。

## 6. 错误/安全/权限/审批

- **错误分层**:store 的 IO/序列化失败是**系统错(`StoreError`),不进 FailureTaxonomy**——taxonomy 只分类 Agent 认知/执行失败,基础设施故障表现为 run abort/retry(harness 处理)。仅 **schema 不匹配且无 migration → `verification_failure`**(读路径拒绝,属 Agent 可见的正确性失败);疑似历史语义被篡改 → `safety_policy_failure`(canonical §11)。
- **append 幂等**:同 `event_id` 重复 append 被去重(幂等),不产生重复事实。
- **封装即安全**:除 `append` 外无写路径;`events` 表对上层只读(投影/游标),杜绝旁路改历史。
- **审计**:每事件带 `provenance`;删除隐私数据是独立机制(canonical §8),M0 不做真实删除,只 append `RetractionEvent`。
- **数据保护姿态(canonical §19)**:secrets 永不入事件(仅 `SecretRef`);payload 可标敏感度 + 脱敏视图(导出/展示);静态加密 M0 依赖 OS 盘加密(应用层加密 M2);损坏检测 = 事务 + checksum;备份恢复 = 单文件冷备(文档化流程)。
- 无 policy 判定(store 在 policy 之下);但**不得**被 policy 之外的模块直接写。

## 7. 配置与可观测

- **config key**:`store.db_path`、`store.fts.enabled`、`store.retention`(M0 = keep-all)。
- **可观测**:store 不发业务事件,但暴露 `read_run/project/replay` 供可观测层(canonical §17)读;migration note 落 `ComplianceCheckResult`/日志。

## 8. 实施步骤与顺序

1. `protocol`:`*Id`、`SchemaVersion`、`Provenance`、`Event` 信封、`EventKind`、`EventPayload`(先信封 + 生命周期变体,域变体占位)、生命周期对象、`RunRequest/RunResult`。
2. `store`:SQLite schema(见 §8.1)、`append`(单写者 + 事务 + 去重)、`read_run`/`EventCursor`。
3. 投影:`Projection` trait + `session_state`、`transcript` 两个内置投影 + 重建。
4. schema 版本:`register_upcaster` + 读时 upcast + migration note。
5. `replay` + `ReplayReport`(diff)。
6. authority companion:`approval_consumptions` + `ApprovalConsumptionLedger`；不得新增事件或第二事实库。

### 8.1 SQLite schema(M0)

```sql
CREATE TABLE events (
  event_id       TEXT PRIMARY KEY,          -- ULID
  run_id         TEXT NOT NULL,
  stream_seq     INTEGER NOT NULL,          -- run 内单调序(排序权威)
  turn_id        TEXT,
  kind           TEXT NOT NULL,             -- EventKind 判别式
  payload        BLOB NOT NULL,             -- 序列化 EventPayload
  schema_version INTEGER NOT NULL,
  ts             INTEGER NOT NULL,          -- 单调 + 墙钟
  provenance     BLOB NOT NULL
);
CREATE UNIQUE INDEX idx_events_run ON events(run_id, stream_seq);   -- run 内有序读(排序权威)
CREATE INDEX idx_events_kind  ON events(kind);
CREATE VIRTUAL TABLE events_fts USING fts5(text, content='');  -- 可检索文本投影
-- 投影表(可重建,非事实源):session_state / transcript 由 apply 折叠维护

-- V1 生产 mutation companion ledger（均可由事件/命令事实重建或核对）
-- aggregate_versions(aggregate PRIMARY KEY, version)
-- command_idempotency(namespace, idempotency_key PRIMARY KEY, semantic_digest, receipt_json)
```

- append-only:**无 UPDATE/DELETE** 于 `events`;撤销经 `RetractionEvent` 新事件表达。

## 9. 测试

- **S21 Schema/Replay**(总纲 §7.4 全规格):v1 事件经 upcast 被 v2 reader 解释;`ReplayReport` 记 schema/policy/loop/model/tool version;replay 出 diff 不覆盖;无 migration note 的 breaking change → 读路径报错。
- **S1 事件面**(本模块部分):两入口产生**同构**事件序列(仅 run_id/ts/source 不同);均可从 `read_run` 得到相同 `EventKind` 序列。
- **S19 单写者**(本模块部分):并发 append 被串行化、无交错半写;投影可从事件重建,与在线投影一致。
- **单元**:append/read round-trip;event_id 去重幂等;command 同键同义返回原 receipt、同键异义冲突;expected-version stale 零写;动作 intent 幂等键不重复副作用;v1→v2 upcast 链;`Projection::apply` 折叠确定性;FTS 查询;事务失败回滚不留半条。
- **审批消费账本**:首次 claim=`true`；完全同义重放=`false`；同 nonce 改 approval/plan/action/resource/policy/tool schema/approver 任一字段均 conflict；SQLite 关闭重开仍拒绝重放；并发 claim 仅一个成功；错误 owner、过期/被替换 writer lease 与无 authority guard 全部零写。
- **ApprovalRequested 兼容**:legacy payload 缺新增 binding 字段仍可反序列化和 replay，但不能恢复成可执行批准；当前 payload 缺任一 binding 校验失败；当前事件 round-trip 后字段完整。

## 10. 验收

- append-only 不可变、单写者、每对象带 `schema_version`;`stream_seq` 是排序权威;command/action/aggregate 各自幂等与 CAS 语义闭合;投影可从事件完全重建;replay 复现且带版本元数据 + diff(不覆盖历史);`events` 无旁路写路径;S21 通过、S1/S19 事件面通过。

## 11. 原创性与合规

- Event sourcing / CQRS / append-only log 是通用模式,非特定项目专有——无 copy risk。SQLite/FTS5 为标准库能力。
- 不复制任何第三方的事件命名体系、schema、序列化格式或表结构;`EventKind` 命名源自本项目 architecture/03 §2.1.1。
- 若未来引入第三方序列化/迁移库,记 source/license/copy_risk/implementation_rule/notice。
