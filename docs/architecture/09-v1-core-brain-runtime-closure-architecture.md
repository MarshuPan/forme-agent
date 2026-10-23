# V1 核心大脑运行闭合架构

本文实现 `requirements/15-v1-core-brain-closure-scope-decisions.md` 与 `16-v1-core-brain-closure-verification-strategy.md`。它不是 M6，也不增加第四内核；它把 M0–M5 已经存在的 18 个 crate、三内核、治理面、执行面、联邦面和能力生态装进**同一个默认、持久、可恢复的 forme 大脑运行时**。

## 0. 状态与冻结基线

- 冻结基线：M5 合入后的 main，18 crates，97 EventKinds，S1–S99 全绿，M5 已合入 main。
- 新增事实只允许在 97-kind taxonomy 末尾追加 `WorkspaceCharterChanged` 与 `DataLifecycleApplied`，闭合后共 99 EventKinds；前 97 项同序同义。
- crate 图、既有 public trait、Harness-first、single authority、owner 身份、执行前重查、能力门出口、candidate/stable/active/permission 分离全部冻结。
- 本文只定义现有模块的组合、持久运行与产品闭环。任何实现若需要新增 crate、依赖边、第三个事件或改变冻结 trait，必须先回 requirements/architecture 经 owner 确认。

### 0.1 历史 taxonomy 与当前 taxonomy 的验证边界

- `EventKind::ALL` 表示**当前运行时**的完整 taxonomy；V1 闭合后为 99 项。M5 历史验收使用不可变的 97 项 strict prefix，两者不是同一个验证输入。
- M5 acceptance artifact、release receipt 和历史报告必须按其声明的 schema/taxonomy snapshot 验证；验证器不得把当前 99 项列表直接拿来与历史 97 项工件比较。
- V1 closure artifact 验证当前完整 99 项，并另行断言前 97 项与 M5 exact prefix 同序同名。历史报告保留当时事实，不通过回写历史文件来“刷新” taxonomy。
- 任何 additive event 都必须同时更新：当前 protocol taxonomy、对应 closure contract，以及读取历史 artifact 的兼容边界；不得让历史工件依赖未来运行时的完整枚举。

## 1. 总体拓扑：一个产品运行时，一个大脑

```text
CLI / Local Gateway / owner control / scheduler signal
                         |
                         v
              ProductionRuntimeBuilder
                         |
          +--------------+---------------+
          |                              |
   ProductionRuntime                CompositionManifest
          |
          +-- authority SqliteEventStore (single writer)
          +-- owner identity + persistent aggregate ids
          +-- ModelRegistry + run-bound model selection
          +-- Capability/Skill/Plugin/MCP/Backend registries
          +-- EventSourcedMemory + CognitiveRuntime
          +-- CoordinationReasoner + SeedOrchestrationLibrary
          +-- Proactivity + scheduler + three-phase cognitive tick
          +-- ContextHydrator + LayeredContextBuilder
          +-- Eval/Verifier + Federation + Ecosystem
          +-- Dashboard + backup/data-lifecycle companion services
                         |
                         v
                   ReactiveHarness
```

`ProductionRuntime` 是产品 composition root，不是新内核。它持有一个权威 `ReactiveHarness` 及其同源 companion services。CLI、Gateway 和 scheduler 不得自行拼装轻量 Harness，也不得创建入口私有 memory、model loop、approval 或 store。

## 2. Composition root 与运行时清单

### 2.1 正式对象

以下对象落在现有 `harness` crate；不新增 crate：

```rust
pub struct ProductionRuntimeConfig {
    pub schema_version: SchemaVersion,
    pub owner: OwnerPrincipalRef,
    pub store_path: PathBuf,
    pub workspace: WorkspaceRef,
    pub models: Vec<ModelRuntimeConfig>,
    pub capabilities: CapabilityRuntimeConfig,
    pub scheduler: SchedulerConfig,
    pub cognition: CognitiveRuntimeConfig,
    pub data: DataProtectionConfig,
}

pub struct CompositionManifest {
    pub schema_version: SchemaVersion,
    pub owner_digest: SchemaDigest,
    pub store_identity: SchemaDigest,
    pub aggregate_refs: BrainAggregateRefs,
    pub model_profiles: Vec<ModelProfileRef>,
    pub capability_snapshot: CapabilitySnapshotRef,
    pub service_states: Vec<RuntimeServiceState>,
    pub digest: SchemaDigest,
}

pub struct ProductionRuntime {
    harness: Arc<ReactiveHarness>,
    services: Arc<BrainServices>,
    manifest: CompositionManifest,
}

pub struct ProductionRuntimeBuilder;

impl ProductionRuntimeBuilder {
    pub fn from_environment() -> Result<Self>;
    pub fn build(self) -> Result<ProductionRuntime>;
}
```

实际字段可以使用现有强类型，但必须保持上述职责。`ReactiveHarness::from_environment*` 退化为对 builder 的兼容委托，不再保留第二套装配逻辑。

### 2.2 构建顺序

构建严格按下列顺序，任一步失败不得返回半启用 runtime：

1. 解析 typed config，只保留 `SecretRef`，解析 secret value 发生在对应 provider/backend 边界。
2. 打开 SQLite，验证 schema、event prefix、integrity、owner binding、single-writer lease 和未决 unknown recovery。
3. 解析稳定 `BrainAggregateRefs`，打开同一 owner 的 memory/cognition/intention/charter/strategy projections。
4. 构建 `ModelRegistry`，校验每个 profile/credential/capability/fallback policy。
5. 构建 capability、MCP、skill、plugin、backend registries；缺配置项必须形成 disabled/unsupported doctor row，不能半暴露。
6. 构建 memory、`CognitiveRuntime`、AgentWorkspace projection、coordination、orchestration、proactivity 和 scheduler。
7. 构建 context hydrator、verifier、federation、ecosystem、dashboard、backup/data lifecycle services。
8. 生成 `CompositionManifest`，运行 blocking doctor；必需项失败则 fail closed。
9. 以同一 services 创建唯一 Harness，执行 startup recovery scan 后才接受入口流量。

### 2.3 单实例与入口

- 同一进程对同一 store 只能存在一个 authority writer；重复 builder 必须被 store lease 拒绝。
- Gateway daemon 持有 `Arc<ProductionRuntime>`；scheduler task 使用同一实例。
- CLI 默认调用 Local Gateway。仅显式 embedded/test 模式可在进程内构建 runtime，但仍走同一 builder 与 manifest。
- `CompositionManifest.digest` 对秘密值、绝对私有路径和易变时间不敏感；相同配置语义应稳定。

### 2.4 入口等价的事件证据

- 正式 runtime 的 Harness 为入口 run（CLI/Gateway/scheduler 提交并由 Loop 驱动的 run）在 `SessionBound` 中写入 `composition_manifest`（`CompositionManifest.digest`）。该字段是 `SessionBound` 的 serde optional additive 字段，排在 `federation_snapshot` 之后；测试专用 Harness、既有控制面专用 run（exact replay、federation owner control、ecosystem control）与历史事件解码为 `None`，序列化时省略，因此前 97 项 payload 字节与历史工件不变。`None` 不能被入口用来证明等价。
- CLI、Gateway、scheduler 对同一 runtime 产生的 `SessionBound` 必须在 `composition_manifest`、`policy_profile`、`model_profile`、`toolset_ref`、`workspace` 上完全相同；差异只允许出现在 `RunAccepted.source` 与 provenance。
- scheduler 只通过 runtime 的共享 scheduler pass 同时驱动到期 intention 与大脑 tick，不另建 Harness。
- run 命令幂等以 authority store 的领域幂等账本为准：同键同义返回原 run，同键异义冲突且零写入，重启后同样成立；进程内 map 只是缓存。

## 3. Owner 大脑身份与持久 aggregate

### 3.1 稳定引用

```rust
pub struct BrainAggregateRefs {
    pub schema_version: SchemaVersion,
    pub owner: OwnerPrincipalRef,
    pub memory_cognition: RunId,
    pub intentions: RunId,
    pub charter: RunId,
    pub dashboard: RunId,
    pub data_lifecycle: RunId,
}
```

引用由 configured owner identity 与 store identity 确定性派生，并写入 authority companion metadata。重启复用，不能按进程随机生成。一个 store 与一个 owner authority 绑定；owner 不匹配时只允许显式离线迁移，不得直接继承。

### 3.2 事实归属

- memory/cognition 共用一个 aggregate，继续满足 `CognitiveRuntime` 的同流要求。
- intentions 使用已有 event-sourced intention services；claim/lease 状态由 store companion ledger 持久化。
- WorkspaceCharter 是 workspace-scoped stable fact，写入 charter aggregate；每个变更同时带 workspace 与 committed version。
- AgentWorkspace 和 dashboard 是可重建投影，不是第二事实库。
- 初始化只创建 companion metadata，不伪造认知/记忆事件。重复启动不产生“假学习”。

## 4. 默认 Context Hydration

### 4.1 Hydrator

`context` crate 保持装配 owner；`harness` 只提供同源服务引用并在每个 run 创建请求：

```rust
pub struct ContextHydrationRequest {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub session: SessionId,
    pub workspace: WorkspaceRef,
    pub scope: Scope,
    pub brain_call: bool,
    pub selected_skills: Vec<SkillRef>,
    pub model: ModelProfileRef,
    pub budget: ContextBudget,
}

pub trait ContextHydrator: Send + Sync {
    fn hydrate(&self, request: &ContextHydrationRequest) -> Result<RunCtx>;
}
```

正式 `default_context` 不得调用 `ContextSources::empty()`。hydrator 从同一 store/registry/projection 生成：

1. fixed/constitutional rules；
2. 当前 session history window；
3. scope-bound memory summary 与 bounded recall；
4. owner 级 AgentWorkspace 的 scope slice；
5. active CognitiveMap/UserModel/AgentSelf/Partnership/Trust/strategy refs；
6. WorkspaceCharter；
7. skill metadata，且只有选中 skill 才加载正文；
8. 当前 tool schema/capability snapshot。

### 4.2 信任与预算

- owner/system rule 才可作为 `Instruction`；外部内容、memory 中的外部原文、模型输出与工具结果保持 `Data/UntrustedData`。
- 大脑级调用可装入 AgentWorkspace、认知 refs、charter 和 recall；普通执行节点只装必要切片。
- 每一 source 有独立硬上限与 provenance；超限必须记录 excluded reason，不能静默 raw truncate。
- `ContextBuildFinished` 必须记录实际 source/slice refs，而不是仅写声明配置。

## 5. 三相 Cognitive Tick

### 5.1 唯一状态机

```text
Claim tick
 -> Phase 1 ObserveAndPropose
 -> Phase 2 ReflectAndCandidate
 -> Phase 3 MaintainMemory
 -> Tick report / release claim
```

三相是一个有序 tick，不是三个并行 loop：

1. **ObserveAndPropose**：读取新事件、到期 intention、workspace diff、failure/change/commitment，形成 opportunity/impulse/proposal；不做稳定写。
2. **ReflectAndCandidate**：对本 tick 的新 outcome/failure/feedback 生成 Reflection、evidence 与 candidate；候选进入 A3。
3. **MaintainMemory**：计算 consolidation/decay/prune/edge deltas，追加 `MemoryMaintenanceApplied`；不删除历史事件。

### 5.2 调度与并发

- store companion ledger 对 `(owner, tick_window)` 原子 claim/lease；重复 signal 只得到同一 claim 结果。
- foreground session 存在时，重模型 reflection/maintenance defer；确定性的 intention 与安全检查仍可运行。
- 无新 evidence、到期 intention、显著 diff 或 maintenance due 时不调用模型。
- phase 读取固定 event range/snapshot；后台只追加 proposal/candidate/maintenance event，不能直接改前台 run、stable cognition 或执行真实动作。
- claim 同时固定 memory/cognition aggregate head；维护读取该 head 的对象版本，晚到变更不混入当前窗口。重试沿用原 window 时间，defer 保留未处理输入；完成时校验落盘 claim 的 nonce/attempt/expiry/range 后再原子推进游标。
- 维护链使用窗口与对象版本确定性身份，允许逐事实重试补齐；部分追加后崩溃不能造成重复降级或缺失 candidate。对象已经有更新版本时不使用旧快照使它失效。
- 每相输出 typed report；前相失败时后相按 fail policy 跳过或只运行安全维护，不能假报成功。

## 6. AskToLearn、AgentWorkspace 与激活扩散

### 6.1 AskToLearn

AskToLearn 由**会影响当前判断的认知缺口**触发：缺口必须绑定 scope、目标、缺失属性、为什么影响质量以及期望候选 target。其结果固定为 L1、默认 Hitchhike，仍经过 AttentionBudget、ValueGate、CompetenceGate 和 policy。

owner 回答只追加 evidence/candidate；拒绝抑制同 origin，defer 转为 intention。无关缺口、已有高置信答案或 quiet policy 下不得重复询问。

### 6.2 Owner 级 AgentWorkspace

`cognition::AgentWorkspaceProjection` 从 owner authority 的全局事件范围投影：active goals、active runs、raised impulses、due intentions、pending candidates、long-running workspace supervision。排序键固定为 `(value * urgency, urgency, stream_seq)` 的确定性降序；容量有界，scope query 必须先过滤再排序。

`context::AgentWorkspace` 是给模型的裁剪视图；`cognition::AgentWorkspaceProjection` 是权威可重建投影。两者不可混为项目 `Workspace`。

### 6.3 激活扩散

`EventSourcedMemory::spread` 激活为可替换实现：

```text
validated seeds
 -> initialize by resting activation + tier + recency + shape boost
 -> bounded frontier expansion
 -> contribution = source_activation * edge_weight * per_hop_decay
 -> convergent contributions sum
 -> deterministic top-k / threshold surface
```

硬边界：

- 无 seed 返回空，不扫全图。
- `max_hops`、`top_k_frontier`、`surface_threshold`、`per_tick_nodes` 全部强制。
- 同 snapshot/config 结果确定；tie 使用 node id。
- edge weight、resting activation、consolidation、decay 与 prune 只通过 `MemoryMaintenanceApplied` delta 事件改变。
- Untrusted 内容不能创建结构边或调整权重。
- 同一候选集必须保留“直接交模型选相关项”的基线；运行策略可以按可信评测选择扩散或基线。

## 7. WorkspaceCharter 与长期项目监管

### 7.1 协议对象

```rust
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

pub struct WorkspaceCharterChangedPayload {
    pub charter: WorkspaceCharterRecord,
    pub expected_version: u64,
    pub committed_version: u64,
}
```

`WorkspaceCharterChanged` 是 R 组第 98 个事件。store 在同一事务校验 owner、workspace、digest、expected version、committed version，并更新投影。旧 `WorkspaceCharter` DTO 仅作兼容输入；正式运行使用 record/projection。

### 7.2 长期监管

每个具有长期 goal 或 cadence 的 workspace 进入 owner 大脑 supervision projection。到期/新 evidence/失败/重大 diff 触发新的 frame/plan/route；不得 mid-run 热换。可从 direct 变为 orchestration，也可减少、增加或替换模式。workspace 不拥有独立 cognition runtime，项目认知仍在 owner 大脑中按 workspace scope 存放。

## 8. 八套种子编排与经验回流

### 8.1 库结构

`coordination::SeedOrchestrationLibrary` 扩充为八套 repository-owned 模式：

1. `single-run`
2. `retrieve-then-answer`
3. `tool-loop`
4. `clarify-then-act`
5. `generator-evaluator`
6. `parallel-research-fanout`
7. `planner-builder-evaluator-integrator`
8. `long-running-harness`

前四套可以零 subagent；后四套可生成受限 child runs。Tool Use 与 Memory 是能力层，不是平级模式；13 种主流模式继续按 planning/15 的三层归位。

`RouteTopology` 可 additive 扩充以表达 retrieve/clarify/generate-evaluate/planner-integrator/long-running，但 topology 只描述 DAG 形状，不成为主脑。

### 8.2 选择与兜底

- 六维 signature：decomposability、dependency、verifiability、clarity、scale、reversibility。
- score 同时记录字段匹配、case anchors 与 active fitness；hard incompatibility 先过滤。
- 最高分过 threshold 时使用种子模式；否则生成 `custom` low-confidence route，强制 evaluator/checkpoint。
- `sediment` 对成功和失败都生成 episode/evidence/strategy candidate；不得直接激活或只奖励成功。

### 8.3 Route 经验

route outcome 连接 goal、pattern、node、result、failure、verification、conflict 与 workspace。事实/设计/质量冲突由 integrator 保留并上抛，不得静默多数票。child workspace 隔离、父预算预留/回收、取消传播、幂等 retry 和 unknown no-retry 继续沿 M2/M4 治理。

## 9. 多模型注册、路由与 fallback

### 9.1 不造假的绑定方式

```rust
pub struct ModelRegistry { /* profile -> provider */ }
pub struct ModelSelection {
    pub schema_version: SchemaVersion,
    pub selected: ModelProfileRef,
    pub policy: ModelRoutingPolicyRef,
    pub rationale: ReasonRef,
    pub fallback_chain: Vec<ModelProfileRef>,
}

pub trait ModelSelector: Send + Sync {
    fn select(&self, request: &ModelSelectionRequest) -> Result<ModelSelection>;
}
```

Harness 在每次模型调用开始前完成选择，并把**选中的具体 provider**绑定到该 attempt。Loop 对该 provider 调用 `profile()` 与 `call()`，因此 `ModelCallStarted/Finished` 写入的 profile 必须与真实网络调用一致。

禁止实现一个在 `call()` 内偷偷 fallback、但 `profile()` 永远返回 primary 的 wrapper。若 primary 失败，Harness 结束该 attempt，记录真实失败，再按有界 policy 选择下一个 provider，创建新的 attempt/event。`SessionBound.model_profile` 是首选/默认 profile；每个 ModelCall 事件才是实际调用事实。

### 9.2 选择输入与边界

选择至少读取 context window、tool compatibility、任务质量/风险、model capability evidence、failure evidence、延迟/成本和 active adaptation strategy。成本用于排序，不得抵消 hard requirement。fallback 只能在可重试 transport/provider failure 且 outcome 明确未产生时发生；高风险/L5、不兼容 schema、结果不明或链耗尽时停止/询问。

fallback 不改变 scope、toolset、permission、approval、DoneContract 或 capability envelope。模型更强只允许减少认知脚手架，不允许减少治理与 ground truth。

## 10. Dashboard 与可观测

`eval` 提供 event-derived `CognitiveDashboardProjection`，`gateway/cli` 只读展示。指标至少包括：

- proposal adopt/reject/defer 与 interrupt regret；
- AskToLearn accepted/useful/rejected/duplicate-suppressed；
- candidate backlog、age、conflict、promotion/downgrade；
- CognitiveMap/UserModel/AgentSelf confidence 与 unverifiable；
- workspace supervision overdue/replanned/outcome；
- pattern/model selection、fallback、quality、failure 与 verification；
- activation implementation 与基线对照。

每个比率必须返回 numerator/denominator/unverifiable count；删除 projection 后可从 event replay 重建。dashboard 不成为新事实源，不存模型隐藏思维链。

## 11. 备份、数据保护与真实删除

### 11.1 备份

`store` 提供 SQLite online backup/checkpoint companion：冻结 writer 窗口、生成一致性快照、manifest、last stream seq、schema/event prefix、owner digest 与文件 checksum。restore 只能到新路径，先离线校验 byte checksum、`integrity_check`、事件 checksum/continuity、projection replay，再由 owner 显式选择；不得自动替换当前 authority。

### 11.2 应用层加密

敏感 payload 在进入不可变 event/object 链之前使用成熟 AEAD crate 外置为 versioned envelope，或只在事件中保存安全 content reference。协议只保存 envelope 的 algorithm/version、safe key digest ref、nonce、ciphertext、AAD digest 与 replay 所需元数据；真实 key 由本地 resolver 提供，不进 config/event/log/trace/artifact。错误 key、缺 key、tamper 全部 fail closed。

加密采用 field/content envelope，不加密 EventKind、stream_seq、schema/version、scope safe digest 等 replay 所需元数据。导出默认脱敏。

不可变事件链中的历史密文不能物理改写；删除通过清理可寻址原文/派生索引并销毁对应内容密钥，使密文不可恢复，同时保留不含原文的 tombstone。若历史事件已经保存可恢复明文，删除 projection 或新增 tombstone 不足以满足 C22/C23：必须完成显式迁移/清理，未完成时 closure gate 必须报告 FAIL/UNVERIFIABLE。

内容保护的运行落点是 store 写边界与真实 artifact writer，不能只提供一个未被调用的加密 helper。AAD 进入 AEAD authentication，绑定 owner/scope/object/schema；外置 key resolver 不进入备份。历史明文迁移到新目标并保留原源的回滚能力，原源仍属未保护数据，必须在结果中披露；迁移不自动接管 authority。SQLite 本地 schema 的 additive companion 表与具体签名以 store 实现及 `prd/23` D 波补充为准，不增加业务事件或 crate 依赖边。

### 11.3 数据生命周期

```rust
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

pub struct DataLifecycleAppliedPayload {
    pub receipt: DataLifecycleReceipt,
    pub expected_version: u64,
    pub committed_version: u64,
}
```

`DataLifecycleApplied` 是 R 组第 99 个事件。删除/retention 清理原始敏感内容、FTS、summary/vector/graph/candidate 派生数据并 crypto-shred 对应 key；append-only 审计链保留不含原文的 tombstone。对历史可恢复明文必须先迁移/清理，不能以删除 projection 冒充完成。远端未验证删除必须标 `Requested/Unknown`，不得宣称完成。

事务边界为“pending job + 写入/检索 fence → 外部 key 销毁验证 → success event/version/tombstone”。中断后 job 保留，禁止脑运行时使用旧认知/cache；恢复严格复用获批 cleanup digest。成功后共享 memory/cognition 重建、run/output cache 失效。保留的密文、外部副本、物理磁盘与远端删除有不同证明边界，receipt 只声明本次已实际验证的本地范围。

## 12. 失败、恢复与一致性

| 条件 | 结果 |
|---|---|
| owner/store binding 不一致 | runtime 构建失败，零服务暴露。 |
| 必需 service 缺配置 | fail closed；可选 service 为 disabled doctor row。 |
| context source 越 scope/信任/预算 | 排除并记录；不能 raw dump。 |
| tick duplicate/lease 过期 | 不重复运行；由 ledger 恢复或重新 claim。 |
| background 与 foreground 冲突 | foreground 优先，重 phase defer。 |
| activation 无 seed/超预算 | 空或确定性截断；不扫全图。 |
| charter stale CAS/非 owner | 零事件、零投影改变。 |
| route true conflict/unknown effect | 上抛主脑/保持 unknown，不盲重试。 |
| model fallback profile 不匹配 | hard failure；不得写伪 profile。 |
| backup tamper/replay gap | restore fail closed。 |
| decrypt/tamper/delete 不完整 | FAIL/UNVERIFIABLE；不得返回部分成功。 |

启动恢复顺序固定为：store integrity/owner binding -> unknown action/remote/ecosystem recovery -> aggregate replay -> scheduler/tick lease recovery -> registry reconciliation -> dashboard rebuild check -> 接受入口。

### 12.1 运行状态恢复契约

运行状态必须把“暂时不能继续”和“已经结束”分开表达：

| 类别 | 状态 | 是否可继续 | 必须保留的事实 |
|---|---|---:|---|
| 可运行 | `Accepted` / `Running` | 是 | 当前 run、turn、snapshot 与已消费的幂等键 |
| 可恢复挂起 | `Waiting` / `Suspended` | 是 | `PendingKind`、resume state、等待原因、过期时间 |
| 终态 | `Complete` / `Aborted` / `Failed` / `Limited` | 否 | stop reason、result/evidence refs、最终 stream cursor |
| 副作用未知 | `Suspended(ToolInterrupt)` + `ActionOutcomeUnknown` | 仅裁决后 | intent、plan digest、attempt、probe/裁决记录；不得自动重试 |

`ResumeInput` 必须与 `PendingKind` 一一匹配：审批等待只能接受绑定当前 plan 的 `ApprovalGrant`，工具中断只能接受原 `intent_id` 的可信 receipt/probe 结果或 owner 裁决，handoff 只能接受当前 checkpoint 的 resolution。恢复前重新校验 owner、scope、policy、grant、envelope、plan digest 和 schema snapshot；校验失败时零动作、零状态推进。

`ResolveToolOutcome` / `ResolveHandoff` 的完整 owner command 是恢复同一裁决所必需的输入，不得只持久化其摘要。command 及其 resolution 以现有 protected-content envelope 加密，绑定 owner、run、command digest、resolution digest、target digest；该伴随记录与 `RunResumed` claim 通过 store 单写者在同一 SQLite 事务提交，并使用与原 run 一致的 scope/subject，避免任一半单独落盘。明文不进入 event、索引、日志或错误文本。owner 认证后的 pending 投影只在 claim 仍未决且绑定校验通过时附带原 command；刷新后由 owner 明确选择核实，服务不自动重放。读取缺失、损坏或绑定不符时 fail closed，保留 Unknown，不生成新 command，也不允许换号裁决。

启动恢复扫描不是“把未完成 run 重新执行一遍”，而是：扫描已持久化的中间事实，补记未知结果或恢复记录，重建挂起状态，随后等待明确裁决。只有已证明没有副作用、且符合 retry policy 的 attempt 才能建立新 attempt；同一 `intent_id` 不得盲重放。

若某个 `ModelCallStarted` 没有其后同 `call_id` 的 `ModelCallFinished`，扫描不得重发该请求或切换 fallback；必须用现有 `RunFailed` 明确终止恢复，stop reason 标记模型调用结果未知，不生成 `ModelCallFinished`、token/cost 估算或最终回答。owner 可另起新 run；原 run 保留可审计的未完成模型调用事实。

### 12.2 事件写入与状态投影契约

所有会改变稳定事实的操作采用统一事务边界：

```text
typed command
 -> authenticate / authorize / expected-version check
 -> assign authority stream_seq
 -> append immutable event
 -> update the derived projection in the same transaction
 -> return the committed result
```

`event_id`、command `idempotency_key`、动作 `intent_id` 和聚合 `expected_version` 分别解决不同问题，不能互相替代。`stream_seq` 由 authority store 分配，是唯一排序权威；时间戳和 ULID 只作信息字段。重复同义 command 必须返回原提交结果，重复键的异义 payload 必须以 conflict 拒绝且不推进版本。

任何 `set_*`、`transition`、`decay`、edge-weight 更新或 lifecycle mutation 都只能是上述 command 的内部实现，不得直接写 projection/cache。瞬时 `current_activation` 可以由 event range + snapshot + 确定性衰减函数按读时派生；会影响 replay、召回或治理判断的边权、巩固、修剪和候选状态变化必须进入结构化事件/maintenance delta。在线 fold 与 replay fold 必须共享同一验证和应用语义。

#### 12.2.1 稳定认知晋升的事件化

- `CandidatePromoted.stable` 记录被激活的 `StableCognitionRecord`：对象引用、scope、种类、陈述、稳定层级、置信度、证据引用（含观察时间与是否经验证过程）、最近复现时间。
- 晋升目标必须与被晋升候选的已记录事实一致：CognitiveMap 提案按其 frame/quality/blindspot/resource 引用，用户属性、AgentSelf、Partnership 按其已编码的候选目标，`stable:{object}` 规格候选按其对象；不一致时零写入拒绝。
- 在线路径先追加事件，再以与重放相同的 fold 更新状态；`CognitiveRuntime` 不保留只在内存里的稳定对象。
- 历史 `CandidatePromoted`（无记录）按原推导重放，仅作兼容；新的晋升一律携带记录。重放与在线状态不一致视为 R-FOLD 失败。
- 记录不含秘密、凭据或私有路径；陈述长度有界。

### 12.3 最终计划、审批和验证契约

执行路径至少有两次治理检查：

1. `ActionIntent` 产生候选计划时，做一次初始 policy/competence/approval 判断；
2. 形成不可变 `ExecutionPlan`、批准返回、driver 调用前，分别重新检查最终 plan digest、规范化 action/resource binding、参数、backend、scope、timeout、rollback、capability、policy snapshot、grant/envelope、owner epoch 和 schema；最终重查通过后再原子 claim one-shot nonce，claim 失败不得产生 `ActionStarted` 或 driver call。

审批绑定的是最终计划，而不是“某类动作”的泛化许可。绑定 tuple 还必须包含 approver、`normalized_action_digest`、排序去重的 `affected_resources`、policy/tool-schema snapshot、nonce 和 expiry。任何 plan 字段、资源集合、审批身份或 backend-specific 参数变化都必须形成新 digest、新 approval ticket/nonce 和新审计链；one-shot 消费状态必须在 authority companion ledger 中跨重启保留。`Verifier` 不得以模型文本或动作自报为唯一证据；它必须按 `DoneContract` 的 selector/predicate/tolerance 读取事件段、动作结果、文件/外部 ground truth，并显式返回 `Pass`、`Fail` 或 `Unverifiable`。

authority companion ledger 以 nonce 为唯一消费键，并保存 owner/store、approval id、plan digest 与其余审批语义的 canonical digest。claim 必须和当前 process nonce 的 writer fence 在同一个 SQLite `IMMEDIATE` transaction 内验证；首次同义 claim 成功，完全同义重放返回已消费，同 nonce 异义复用返回 conflict。该记录不是新业务事件，但属于跨重启执行授权事实，不能由进程内 `Mutex`、Gateway 会话或 `ApprovalResolved` 事件单独替代。

### 12.4 文档与实现状态契约

`main` 是用户可复用的基线，功能分支是施工现场。架构/PRD 可以记录当前分支的实现进度，但只有对应 commit 已通过该波独立验收、永久回归、合规门和 clean-tree release audit 并合入 `main`，才允许在总入口写“完成”“闭合”或“永久基线”。这条规则防止文档把草案、局部测试和正式交付混为一谈。

## 13. Crate 增量职责

| crate | V1 闭合职责 |
|---|---|
| `protocol` | 两个 R 事件及 charter/data-lifecycle DTO；已有对象 additive 兼容。 |
| `store` | owner/brain metadata、charter/data lifecycle CAS projection、tick claim、backup/restore/encrypted content companion。 |
| `models` | ModelRegistry、selector、run/attempt-bound provider 与有界 fallback。 |
| `memory` | 激活扩散、maintenance delta replay、baseline candidate adapter。 |
| `cognition` | 三相 tick、AskToLearn、owner AgentWorkspace、maintenance/reevaluation。 |
| `coordination` | 八套模式、签名评分、custom route、episode/fitness candidate。 |
| `context` | ContextHydrator 与正式非空 sources。 |
| `capabilities` / `execution` | typed config 统一注册、backend doctor rows；不新增边。 |
| `harness` | ProductionRuntimeBuilder、single composition、tick/route/model companion orchestration。 |
| `eval` | dashboard、价值 baseline、closure artifacts/verifier。 |
| `gateway` / `cli` | 同一 runtime 的 control/read surface。 |
| 其余 crate | 复用既有 policy/approval/communication/federation/ecosystem 不变量。 |

## 14. 实施波次与永久门

1. **A：默认装配**——composition、owner aggregates、context、registries、入口统一和多模型骨架。
2. **B：持续认知**——三相 tick、AskToLearn、AgentWorkspace、activation、maintenance。
3. **C：项目与编排**——charter、八套模式、动态匹配、长期监管、经验回流。
4. **D：产品闭合**——多模型完整 routing/fallback、dashboard、backup/restore、encryption/delete、价值 golden。

每波执行 S1–S99 永久回归和对应 C 场景。最终只有 C1–C24、99-kind taxonomy（前 97 项 exact prefix）、18-crate graph、strict clippy、合规、真实 golden、typed artifact 与 clean-tree release audit 全绿，才能声明 V1 核心大脑运行闭合。
