# PRD 23：V1 核心大脑运行闭合实施计划

本文是 V1 核心大脑闭合的实施级 Program PRD。它承接 `requirements/15-16`、`architecture/canonical-contract.md` §28、`architecture/09-v1-core-brain-runtime-closure-architecture.md`，并复用 M0–M5 的全部冻结契约。它不是 M6；目标是把已有设计与模块变成默认运行时中的一个持久 forme 大脑。

## 0. 定位、基线与完成口径

- **冻结基线**：M5 合入后的 main，18 crates，97 EventKinds，S1–S99 全绿，M5 已合入 main。
- **目标**：C1–C24 全绿；仅末尾增加 2 个 R 组事件，总数 99；内部 crate 图不变。
- **施工波次**：A 默认装配、B 持续认知、C 项目/编排、D 产品闭合。每波独立 conventional commit、验收报告和永久回归。
- **不允许**：新增 M6、第四内核、crate/依赖边、独立 workspace/subagent 大脑、第二事实库、秘密落事件、隐式模型 fallback、后台稳定层直写。
- **最终声明**：只有真实 CLI/Gateway、真实 SQLite 重启、至少一个真实 external backend、真实多模型或可控 fallback、C1–C24 typed artifacts 与 release gate 全绿，才能说“V1 核心大脑闭合完成”。

## 1. 全局集成契约

### 1.1 模块 owner

| 能力 | Owner crate | 只允许的 consumer/协作 |
|---|---|---|
| R 组 DTO/EventKind | `protocol` | 全事件 consumer |
| authority store、brain metadata、CAS/projection、backup/lifecycle ledger | `store` | harness/context/cognition/eval |
| 模型注册/选择/attempt-bound provider | `models` | harness/loop |
| memory graph/activation/maintenance | `memory` | cognition/context |
| cognitive tick/AskToLearn/AgentWorkspace | `cognition` | harness/coordination/context |
| patterns/signature/custom route/outcome candidate | `coordination` | harness |
| context hydration/rendering | `context` | harness/loop |
| composition root 与统一入口语义 | `harness` | gateway |
| dashboard/value/artifact verifier | `eval` | harness/gateway/release gate |
| API/CLI surface | `gateway`/`cli` | owner client |

### 1.2 Frozen seams

- `EventStore`、`ModelProvider`、`ContextBuilder`、`MemoryGraph`、`CoordinationReasoner`、`OrchestrationLibrary`、`AgentHarness` 既有签名不破坏；需要新行为时加 companion type/trait 或 internal method。
- 前 97 `EventKind::ALL`、string、payload mapping 和 store `PayloadType` 是 exact prefix。
- stable mutation 必须事件化；projection/cache/metric 可删后重建。
- Loop 每次只调用已绑定的具体 provider。selector/fallback 不藏在 `ModelProvider::call` 内。
- 子代理、workspace、plugin、MCP、model、catalog 不能 append stable cognition 或绕过 Harness。

### 1.3 代码组织

为避免继续膨胀单个 `lib.rs`，允许在 crate 内增加下列 module，但不改变 crate 图：

```text
protocol/src/v1_closure.rs
store/src/brain.rs
store/src/backup.rs
models/src/registry.rs
memory/src/activation.rs
cognition/src/brain_tick.rs
coordination/src/patterns.rs
context/src/hydration.rs
harness/src/production.rs
eval/src/dashboard.rs
eval/src/core_brain_artifact.rs
gateway/src/core_brain.rs
```

是否拆文件不影响 public API；实现可先在现有文件落地再机械拆分，但每个 crate 必须保持 `#![forbid(unsafe_code)]`。

## 2. Protocol 与 99-kind taxonomy

### 2.1 新增类型

在 `protocol/src/v1_closure.rs` 增加并从 `lib.rs` 导出：

```rust
pub struct OwnerPrincipalRef(pub String);
pub struct ProjectionRef(pub String);

pub struct WorkspaceCharterRecord {
    pub schema_version: SchemaVersion,
    pub workspace: WorkspaceRef,
    pub version: u64,
    pub goals: Vec<GoalRef>,
    pub constraints: Vec<Constraint>,
    pub prohibitions: Vec<Constraint>,
    pub done_contract: Option<DoneContractRef>,
    pub review_cadence: Option<DurationMs>,
    pub actor: Actor,
    pub digest: SchemaDigest,
}

pub enum DataLifecycleOperation { Retain, Delete, CryptoShred }
pub enum RemoteDeletionDisposition { NotApplicable, Requested, Verified, Unknown }

pub struct DataLifecycleReceipt {
    pub schema_version: SchemaVersion,
    pub aggregate: RunId,
    pub operation: DataLifecycleOperation,
    pub scope: Scope,
    pub subject_digest: SchemaDigest,
    pub cleaned_projections: Vec<ProjectionRef>,
    pub destroyed_key_digests: Vec<SchemaDigest>,
    pub remote_disposition: RemoteDeletionDisposition,
    pub evidence: Vec<EvidenceRef>,
}
```

所有 string ref 非空；versions 单调；charter digest 以稳定字段 canonical digest 计算，排除自身 digest；`actor` 必须为 owner；`review_cadence=0` 非法。lifecycle receipt 不得包含 subject 原文、SecretRef、key id/value、私有路径或 endpoint。

### 2.2 R 组事件

在 `event.rs` 最末尾追加：

```rust
WorkspaceCharterChanged => WorkspaceCharterChangedPayload {
    charter: WorkspaceCharterRecord,
    expected_version: u64,
    committed_version: u64,
},
DataLifecycleApplied => DataLifecycleAppliedPayload {
    receipt: DataLifecycleReceipt,
    expected_version: u64,
    committed_version: u64,
},
```

`EventKind::ALL` 长度改为 99；store `PayloadType` 同步。测试必须证明：

- first 97 exact prefix；
- string/from_str/payload kind/serde roundtrip；
- unknown/zero schema/invalid version/digest/actor/secret marker 拒绝；
- legacy 97-kind DB 可打开，新增 aggregate version 为 0，不生成 synthetic event。

## 3. A 波：产品默认装配（C1–C6）

### A1 ProductionRuntimeConfig 与 builder

在 `harness` 增加：

```rust
pub struct ProductionRuntimeConfig { /* architecture/09 §2.1 */ }
pub struct BrainAggregateRefs { /* architecture/09 §3.1 */ }
pub enum RuntimeServiceStatus { Ready, Disabled, Unsupported, Blocking }
pub struct RuntimeServiceState { name: String, status: RuntimeServiceStatus, reason: Option<ReasonRef> }
pub struct CompositionManifest { /* architecture/09 §2.1 */ }
pub struct BrainServices { /* Arc services, no second authority */ }
pub struct ProductionRuntime { /* harness + services + manifest */ }
pub struct ProductionRuntimeBuilder { config: ProductionRuntimeConfig }
```

Required builder methods：

```rust
impl ProductionRuntimeBuilder {
    pub fn from_environment() -> Result<Self>;
    pub fn from_config(config: ProductionRuntimeConfig) -> Result<Self>;
    pub fn build(self) -> Result<ProductionRuntime>;
}

impl ProductionRuntime {
    pub fn harness(&self) -> Arc<ReactiveHarness>;
    pub fn manifest(&self) -> &CompositionManifest;
    pub fn doctor(&self) -> ConfigDoctorSnapshot;
    pub fn tick_once(&self) -> Result<CognitiveTickReport>;
}
```

`ReactiveHarness::from_environment(store)` 与 `from_environment_local()` 必须委托同一 internal builder path；不得保留旧单模型/空 context 拼装分支。

### A2 配置兼容与多模型注册骨架

保留现有单模型环境变量，作为只有一个 profile 的兼容配置：

```text
FORME_OWNER_ID                     required
FORME_STORE_PATH                  default .forme/forme.db
FORME_MODEL_PROVIDER
FORME_MODEL_NAME
FORME_MODEL_BASE_URL
FORME_MODEL_API_KEY
FORME_MODEL_CONTEXT_WINDOW
FORME_MODEL_TIMEOUT_MS
```

多模型配置新增 `FORME_MODEL_PROFILES_JSON`，内容是有界 JSON array，只允许：profile id/provider/model/base URL/capability/cost/rate limit/`credential_env`/fallback group。JSON 只保存 secret 所在环境变量名；真实 key 值不进入 config、manifest、doctor 或 event。若该变量存在，则 legacy 单模型变量只在明确列为 default 时使用，不能悄悄混入。

`models` 增加：

```rust
pub struct RegisteredModel { profile: ModelProfile, provider: Arc<dyn ModelProvider> }
pub struct ModelRegistry { /* deterministic BTreeMap */ }
pub struct ModelSelectionRequest { /* task/context/tool/risk/evidence */ }
pub struct ModelSelection { /* selected/policy/rationale/fallback_chain */ }
pub trait ModelSelector: Send + Sync { fn select(&self, request: &ModelSelectionRequest) -> Result<ModelSelection>; }
```

A 波只要求注册、查找、compatibility filter 与 deterministic selection 骨架；D 波完成 fallback/adaptation。duplicate profile ref、credential 缺失、invalid URL、tool incompatibility 均产生 blocking/disabled doctor row。

### A3 Owner/store binding 与 aggregate refs

`store` 增加 companion metadata table：

```sql
brain_authority(owner_digest PRIMARY KEY, store_identity, schema_version, created_at)
brain_aggregates(owner_digest, kind, aggregate_run_id, PRIMARY KEY(owner_digest, kind))
authority_writer_lease(store_identity PRIMARY KEY, process_nonce, expires_at)
```

- 首次启动以 configured owner digest 初始化；不保存 owner 原文。
- 同 store 再以不同 owner 打开 fail closed。
- aggregate IDs 确定性且持久；重复构建不追加事件。
- writer lease 通过 SQLite immediate transaction claim；进程崩溃后按 TTL/nonce 恢复，不能同时双写。

### A4 默认 memory/cognition/intention services

builder 必须用同一个 `Arc<SqliteEventStore>` 创建：

```rust
let memory = Arc::new(EventSourcedMemory::open(store.clone(), refs.memory_cognition)?);
let cognition = Arc::new(CognitiveRuntime::open(store.clone(), memory.clone(), refs.memory_cognition)?);
let intentions = event_sourced_intention_services(store.clone(), refs.intentions)?;
```

`ReactiveHarness` 增加这些 service fields 或一个 `Arc<BrainServices>`；`brain_services()` 提供只读 handle。测试专用 `new()` 可以继续薄装配，但正式 builder 不允许 NoopProactivity、coordination=None、temporary store 或 missing owner。

### A5 ContextHydrator

在 `context` 实现 `EventBackedContextHydrator`，构造时注入 projection/registry reader companion trait，不允许 context 反向依赖 harness/cognition。若为保持依赖图，认知与 AgentWorkspace 以 `ContextSliceSource` trait 由 harness 适配：

```rust
pub trait ContextSliceSource: Send + Sync {
    fn slices(&self, request: &ContextHydrationRequest) -> Result<Vec<ContextSlice>>;
    fn agent_workspace(&self, request: &ContextHydrationRequest) -> Result<Option<AgentWorkspace>>;
}
```

hydration 必须包含实际 history、memory summary/recall、charter、active cognition refs、workspace slice、skill metadata/tool schema；所有 source 先 scope/trust/limit，再进入 `LayeredContextBuilder`。正式 run 若得到完全空 sources，构建失败而不是调用模型。

### A6 统一 capability/backend 装配与 doctor

builder 从同一 config 初始化已有 registry/lifecycle：File/Shell/MCP/Skill/Plugin/Notification/Browser/Computer/PTY/AppApi。当前环境不可用的 backend 为 `Disabled|Unsupported`，不要求假装全部实跑。规则：

1. discovery 不等于 enable；
2. enable 不等于 trust/permission；
3. visible toolset 必须经 resolver；
4. driver/secret/schema 缺失时不暴露 tool；
5. Shell 不是未知能力兜底。

`ConfigDoctorReport` 扩展 checks 内容但不新增 EventKind。manifest 与 doctor 只保存安全 ref/digest。

### A7 入口改造

- `gatewayd` 启动一次 `ProductionRuntime` 并注入 routes/state。
- scheduler thread/task 从同一 runtime 调 `tick_once`，不重新 build Harness。
- CLI 默认经 Gateway；embedded command 必须调用同一 builder。
- 每个入口的 `SessionBound` manifest/model/toolset/cognition refs 等价，差异只在 Source/provenance。

### A8 A 波测试与提交

对应 C1–C6。至少添加：

- production builder clean/restart/wrong-owner/double-writer；
- manifest determinism 与 secret/path absence；
- non-empty context、scope leak、untrusted treatment、budget exclusion；
- CLI/Gateway/scheduler same manifest；
- missing model/store/credential fail closed；optional backend disabled；
- S1–S99、97-kind prefix（A 波尚未激活 R events 前允许文档提交；代码激活后直接 99）。

提交建议：`feat(harness): compose the persistent forme brain runtime`。

## 4. B 波：持续认知运行（C7–C12）

### B1 Tick claim 与三相状态机

`store` 增加：

```rust
pub struct CognitiveTickClaim { owner_digest, window, lease_nonce, expires_at }
pub trait CognitiveTickLedger {
    fn claim_tick(&self, ...) -> Result<Option<CognitiveTickClaim>>;
    fn finish_tick(&self, claim: &CognitiveTickClaim, report_digest: SchemaDigest) -> Result<()>;
    fn abandon_expired_tick(&self, ...) -> Result<()>;
}
```

`cognition` 增加：

```rust
pub enum CognitiveTickPhase { ObserveAndPropose, ReflectAndCandidate, MaintainMemory }
pub struct CognitiveTickInput { owner/event range/snapshot/foreground state/now }
pub struct CognitiveTickPhaseReport { phase, status, event_refs, model_calls, deferred_reason }
pub struct CognitiveTickReport { claim, phases, event_range, digest }
pub trait CognitiveTickEngine { fn run(&self, input: CognitiveTickInput) -> Result<CognitiveTickReport>; }
```

算法按 architecture/09 §5；固定 event range，phase 有序，foreground 优先，无 diff 不调模型。每次 append 仍经 store 单写者。

### B2 AskToLearn

新增 internal `KnowledgeGap` / `AskToLearnProposal`（可落 cognition，不必新 protocol event）：

```rust
pub struct KnowledgeGap { scope, goal, target, reason, impact, existing_confidence }
pub trait KnowledgeGapDetector { fn detect(&self, snapshot: &CognitiveSnapshot) -> Vec<KnowledgeGap>; }
```

gap 只有在会影响当前 goal/quality/risk 时生成 `ImpulseRaised{Gap}` 与 `ProactiveProposalEmitted{L1,Hitchhike}`。owner reply 通过既有 `ObservationRecorded`/`CandidateCreated`/User candidate 路径进入证据；禁止直接 stable write。

### B3 Owner AgentWorkspace

扩展 `cognition::AgentWorkspaceProjection`：

- 输入 owner authority 全 event stream，而不是当前 run 子集；
- item kinds 至少 goal/run/impulse/intention/candidate/workspace-review；
- scope filter 在 sort 前；
- sort deterministic；
- hard capacity；
- snapshot ref/digest 包含 last event cursor 与 config version。

`harness` 的 context slice adapter 把 cognition item 转成 `context::AgentWorkspaceItem`，保留 provenance，不塞 raw event。

### B4 激活扩散

在 `memory` 实现：

```rust
pub struct ActivationConfig {
    pub max_hops: u32,
    pub top_k_frontier: usize,
    pub surface_threshold: f32,
    pub per_tick_nodes: usize,
    pub per_hop_decay: f32,
    pub recency_half_life_ms: u64,
}

pub trait RelevanceBaseline {
    fn select(&self, cue: &RecallCue, candidates: &[MemoryNode], k: usize) -> Result<Vec<NodeId>>;
}
```

`ActivationBudget` 必须映射所有硬限制。实现使用稳定排序/BTreeMap，禁止 hash iteration 导致 nondeterminism。edge delta 先形成 `MemoryMaintenanceDeltaSet` 内容对象与 digest/ref，再追加 `MemoryMaintenanceApplied`；fold/replay 应用同一 delta。

### B5 Maintenance 与重评

maintenance detector 对 evidence age、conflict、long-unseen、low-value branch 生成：

- `ReevaluationTaskCreated`；
- caution/downgrade candidate；
- `MemoryMaintenanceApplied`；
- 必要的 `FailureEvidenceRecorded`。

不得直接提升 confidence/autonomy，不物理删除历史 event。所有 derived invalidation 继续走 lineage。

### B6 B 波测试与提交

对应 C7–C12：tick order/duplicate/foreground defer/no-change zero-call；AskToLearn 正反/抑制/defer；AgentWorkspace restart/scope/order/capacity；activation 五 shape/bounds/replay/untrusted/no seed；baseline report；maintenance candidate-only。

提交建议：`feat(cognition): run the governed persistent cognitive cycle`。

## 5. C 波：项目与编排闭环（C13–C18）

### C1 Charter aggregate/store

`store` 增加 `workspace_charter_state(workspace PRIMARY KEY, version, event_id, payload_json, digest)`；append helper 必须同时：

1. 验证 R event/payload kind；
2. expected version 与当前相等；
3. committed = expected + 1；
4. actor owner、workspace/digest/constraints 有效；
5. append event + projection 同一 transaction。

`harness` 提供 owner-authenticated prepare/apply/read companion；Gateway 只绑定 principal 并调用 Harness。context/coordination 只读该 projection。

### C2 八套模式

扩充 `RouteTopology` 和 `SeedOrchestrationLibrary::default()`。每个 pattern 必须有：purpose、topology、六维 signature、roles、operation guide、initial fitness、case anchors。可新增 metadata 字段但保持 serde/backward default（如对象尚未 serde，则保持 Rust constructor 兼容 helper）。

最低 topology 语义：

| pattern | route 语义 |
|---|---|
| single-run | 无 child node，主脑直接执行。 |
| retrieve-then-answer | retrieve step + main brain answer；可零 child。 |
| tool-loop | 主脑受治理 tool loop；可零 child。 |
| clarify-then-act | clarify gate 后才产生 action route。 |
| generator-evaluator | generator -> independent evaluator。 |
| parallel-research-fanout | N independent researchers -> integrator。 |
| planner-builder-evaluator-integrator | 四角色依赖 DAG。 |
| long-running-harness | plan/checkpoint/fresh-context/review 周期。 |

### C3 六维匹配与 custom route

`signature_score` 返回 typed breakdown，不再只返回 u8：

```rust
pub struct PatternMatchScore { dimensions, anchors, fitness, total, hard_compatible }
pub struct PatternSelection { pattern: Option<...>, scores, custom, confidence, reason }
```

hard incompatibility 先过滤；总分过 threshold 选已有模式；全不过阈值时 route 使用 `pattern_ref=None`、low confidence、强制 evaluator/checkpoint。固定任务字符串映射禁止进入实现。

### C4 Route runtime 与长期监管

复用既有 `RouteRuntime`、BudgetLedger、RouteNodeState。补齐：

- parent budget atomic reservation/release；
- parent cancel cascade；
- same intent retry idempotency；
- OutcomeUnknown no retry；
- child isolated working directory/scope/toolset/model；
- conflict result contract；
- checkpoint 后才允许 long-running 独立上下文/replan。

`WorkspaceSupervisor` 从 charter/goal/cadence/new evidence/failure/diff 产出 review due items；tick 生成新 GoalFrame/DecisionTrace/route。已有 run 不热换，新 route 只影响下一 segment。

### C5 Outcome -> episode/candidate/fitness

成功和失败 route 都：

1. 写 Episode memory node；
2. 写 goal/pattern/node/result/failure/verification edges；
3. 调 `sediment` 生成 strategy candidate/fitness evidence；
4. 经 M3 evaluation/promotion/activation 才影响 active policy。

`sediment` 不能仅在 Completed 返回候选；Failed/Cancelled/OutcomeUnknown 必须保留负案例，具体是否生成调整候选由 evidence sufficiency 决定。

### C6 C 波测试与提交

对应 C13–C18：charter CAS/restart/owner/scope/context enforcement；八套 library exact set；signature/custom；DAG states/budget/cancel/retry/unknown/isolation/conflict；long-term re-coordinate；success/failure episode/candidate/replay。

提交建议：`feat(coordination): close long-running workspace orchestration`。

## 6. D 波：产品闭合（C19–C24）

### D1 模型 selection/fallback/adaptation

Harness 在每次 model attempt 前：

1. 构造 `ModelSelectionRequest`；
2. selector 返回 exact profile/fallback chain；
3. registry 取得 concrete provider；
4. 写 `ModelCallStarted{actual profile}`；
5. 调用 provider；
6. 写该 attempt 的 finished/failure evidence；
7. 只有明确 pre-effect/retryable failure 才选择下一 provider。

为保持 `ModelProvider` trait 不变，Loop 增加 internal/run-level method接收 `Arc<dyn ModelProvider>`，或 Harness 为每个 attempt 创建绑定 provider 的 loop driver；不得把 selector 做成返回伪 profile 的 provider wrapper。

fallback chain 有最大 attempts；重复 profile、能力不兼容、L5/high-risk、unknown result 立即停止。DecisionTrace 记录 selection policy/snapshot/rationale。ModelProfile 决定 reasoning scaffold/checkpoint/evaluator strength，但不能改变治理。

### D2 Dashboard

`eval::CognitiveDashboardProjection` 对 event range fold，输出：

```rust
pub struct MetricRatio { numerator: u64, denominator: u64, unverifiable: u64 }
pub struct CognitiveDashboardSnapshot { owner/workspace/window/cursor/metrics/digest }
```

Gateway/CLI 提供 owner-authenticated read；支持 global/workspace/time window。删 projection 后 replay 相同。不得新增不可追溯 metrics DB；可用 cache 但 cursor/digest 必须绑定 event range。

### D3 Backup/restore

`store::BackupService`：

```rust
pub trait BackupService {
    fn create_cold_backup(&self, target: &Path) -> Result<BackupManifest>;
    fn verify_backup(&self, source: &Path) -> Result<BackupVerification>;
    fn restore_to(&self, source: &Path, target: &Path) -> Result<RestoreReport>;
}
```

使用 SQLite 官方 backup/checkpoint 能力或 rusqlite 暴露的成熟 API，不复制热 DB 文件。manifest 包含 schema/event count/last seq/owner digest/file digest；绝不含秘密/私有 endpoint。restore 到新路径，做 integrity/event continuity/replay projection 对账；不自动切换 authority。

### D4 加密 envelope

采用经过审计的 AEAD crate（具体依赖先更新 third-party/NOTICE/borrowing record）。新增 `EncryptedContentEnvelope`，并规定敏感内容在进入不可变 event/object 链之前必须被 envelope 化或外置为安全 content reference；key resolver companion 只接 `SecretRef`；AAD 绑定 owner/scope/object/schema。nonce 唯一；key/SecretRef id 不进入 Debug/Event/artifact。旧明文数据迁移必须是显式、可恢复、可验证的 owner operation；迁移未完成时不得静默混存并声称全部加密，C22/C23 必须 FAIL/UNVERIFIABLE。

### D5 Retention/delete

`store` 增加 data lifecycle aggregate version、cleanup plan 与 tombstone projection。Harness control flow：authenticate owner -> build scoped cleanup plan/digest -> expected-version recheck -> delete/crypto-shred raw content and derived indexes -> append `DataLifecycleApplied` + version/tombstone transaction -> verify no local retrieval -> report remote disposition。不可变事件中的历史密文不能改写，只能靠销毁对应内容密钥失效；若发现历史可恢复明文，必须先完成显式迁移/清理，否则不允许报告真实删除完成。

删除范围至少覆盖 content store、FTS、summary/vector/graph/candidate projection 与 per-subject keys。append-only audit metadata保留但不得含原文。无法验证的远端清理是 Unknown。

### D6 价值基线与真实 golden

`eval` 产出六类任务 artifact：简单对话冷启动、复杂工具任务、长期 workspace、多 Agent、AskToLearn/提醒、失败后重协调。每类与 direct model/最简单策略对照并报告 quality/reliability/consistency/latency/cost/interruption/hard invariant。

activation、memory recall、orchestration、model routing 分别有 baseline。若复杂机制不优，报告必须允许选择/退役简单实现；hard invariant 失败直接 FAIL。

### D7 D 波测试与提交

对应 C19–C24：actual model profile/fallback/no hidden switch；dashboard replay；backup tamper/restore；encryption wrong-key/tamper/secret scan；delete index/key/tombstone；six real goldens/baselines。

提交建议：`feat(runtime): complete the V1 core brain product loop`。

## 7. Gateway 与 CLI 表面

保持现有 run API，并 additive 提供：

```text
GET  /v1/runtime/manifest
GET  /v1/runtime/doctor
POST /v1/runtime/tick
GET  /v1/brain/workspace
GET  /v1/brain/dashboard
GET  /v1/workspaces/:id/charter
PUT  /v1/workspaces/:id/charter
POST /v1/data/backup
POST /v1/data/verify-backup
POST /v1/data/restore-plan
POST /v1/data/restore
POST /v1/data/lifecycle
```

所有 mutation 只接受 Gateway 已认证并绑定的 owner principal；payload 自报 owner 无效。高影响 data lifecycle/restore 继续 plan/approval/nonce/expiry/expected-version bound。CLI 命令只调用这些 control surface，不直接读写 SQLite。

## 8. 事件主线

默认 run：

```text
RunAccepted -> SessionBound{manifest/model/toolset snapshots}
 -> ContextBuildStarted -> ContextBuildFinished{actual refs}
 -> ModelCallStarted{actual profile} -> ... -> RunComplete
 -> observation/reflection/candidate inputs enter the owner aggregate
```

Tick：

```text
ObservationRecorded/OpportunityDetected/ImpulseRaised
 -> ValueGateEvaluated -> CompetenceGateEvaluated
 -> ProactiveProposalEmitted?
 -> ReflectionProduced -> CandidateCreated/CognitiveMapUpdateProposed?
 -> ReevaluationTaskCreated?/MemoryMaintenanceApplied?
```

Charter：

```text
owner control RunAccepted -> SessionBound
 -> WorkspaceCharterChanged{expected,committed}
 -> RunComplete
```

Data lifecycle：

```text
owner plan -> ApprovalRequested/Resolved -> final scope/version recheck
 -> local cleanup + verification
 -> DataLifecycleApplied{receipt,expected,committed}
 -> RunComplete
```

## 9. 错误与用户可见结果

| Failure | 分类 | 用户可见结果 |
|---|---|---|
| owner/store mismatch、double writer | `trust_failure` | runtime unavailable；明确 authority conflict。 |
| required service/config missing | `resource_selection_failure` | build blocked；doctor 给安全原因。 |
| context scope/trust/budget violation | `safety_policy_failure` | source excluded 或 run blocked，不泄露内容。 |
| tick duplicate/foreground conflict | `coordination_failure` | idempotent/deferred；不重复调用。 |
| activation/baseline failure | `reasoning_failure` | fallback simple baseline；保留 comparison。 |
| charter CAS/owner failure | `trust_failure` | zero event/projection mutation。 |
| route conflict/unknown effect | `verification_failure` / `execution_failure` | escalate/unknown；不盲重试。 |
| model mismatch/hidden fallback | `verification_failure` | attempt fail；事件不得造假。 |
| backup tamper/replay gap | `verification_failure` | restore blocked。 |
| decrypt/delete incomplete | `safety_policy_failure` / `verification_failure` | FAIL/UNVERIFIABLE，不报完成。 |

## 10. 测试矩阵

每个 C 场景必须至少包含一个正向、一个反向、一个重启/回放或并发断言，并以 event/projection/ground truth 为证：

| 场景 | Rust owner tests | 集成/真实证据 |
|---|---|---|
| C1–C6 | harness/store/context/models/gateway | formal CLI + Gateway manifest golden |
| C7–C12 | cognition/memory/store/harness | tick artifact + baseline comparison |
| C13–C18 | protocol/store/coordination/harness | long workspace + route golden |
| C19–C24 | models/eval/store/harness/gateway | multi-provider、backup/delete、six-task golden |

禁止只断言最终文本。测试必须检查：

- event exact subsequence 与 stream_seq；
- stable projection/CAS/restart；
- driver/model/backend ground-truth call count；
- secret/path/plaintext negative scan；
- hard invariant structural zero-call/zero-write；
- baseline 的同 case/snapshot/budget 公平性。

## 11. Artifact 与 release gate

新增：

```text
tools/verify-core-brain-closure.ps1
tools/verify-core-brain-workspace-contract.ps1
tools/core_brain_release_audit.py
docs/acceptance/core-brain-{a,b,c,d}-acceptance-report.md
docs/acceptance/core-brain-closure-acceptance-report.md
docs/acceptance/core-brain-closure-artifacts/
docs/acceptance/core-brain-closure-release-audit-artifacts/
```

最终 gate 顺序：

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
bash tools/compliance-doctor.sh
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m5.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-core-brain-closure.ps1
```

closure verifier 必须 fail closed 并验证：99-kind taxonomy + 97 exact prefix、18 crates/dependency graph、C1–C24、S1–S99、typed closed artifact set、真实 goldens、secret/private path/plaintext scan、clean tracked tree、current-tree release receipt。

## 12. 提交与验收纪律

1. 文档冻结单独提交：`docs(core-brain): freeze V1 runtime closure contracts`。
2. A 波代码/测试/报告分别提交；A gate PASS 后进入 B。
3. B、C、D 同样独立提交，不把跨波修复藏进最后一个大提交。
4. 每次提交前运行 `cargo check && cargo test && bash tools/compliance-doctor.sh`；波次门额外 strict clippy、M5 永久回归和该波 verifier。
5. acceptance report 必须记录 branch/commit、命令、测试数、EventKind 数、crate 图、artifact digest、正反证据、残余边界。
6. 历史 acceptance report 不回写当时分支/合并事实；当前状态只更新总入口与本计划报告。

## 13. 原创、依赖与安全

- production composition、activation、pattern library、fixtures、errors、dashboard 和 lifecycle schema 全部 repository-owned；不复制任何第三方项目的代码/提示词/目录/fixture/错误文案。
- 新增 AEAD/backup 等第三方依赖必须 exact pin，先更新 `docs/compliance/third-party-dependencies.md`、`compliance/borrowing-records.json`、NOTICE/license gate。
- `research/upstreams/**` 永不进入 build/import/artifact。
- secrets 只在本地 resolver/provider/backend 边界出现；Debug、panic、doctor、event、trace、backup manifest、acceptance artifact 均不得包含值或可利用的 SecretRef identifier。
- owner 级长期数据默认敏感；任何真实外部输入继续是 data，不因多次出现、签名、TLS、模型总结或 graph convergence 升格为指令。
