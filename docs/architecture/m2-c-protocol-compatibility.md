# M2-C Protocol Compatibility Note

本文固定 M2-C 对 `forme-protocol`、事件 payload、store 扩展 trait 与 replay 的 additive 变更。它承接 canonical §5-9、§15、§18-19、§24、`05-m2-cross-system-collaboration-architecture.md` 和 `prd/19`。M2-C 不引入第二事实源，也不把资源评分、能力提案、managed policy 或同步成功解释为执行授权。

## 1. 增量对象

所有下列对象带 `SchemaVersion`，旧 enum 值不重排、不改名：

- ResourceGraph：`ResourceKind`、`ResourceEvidenceOutcome`、`ResourceScore`、`ResourceNode`、`ResourceEdge`、`ResourceGraphSnapshot`。snapshot 记录每个 aggregate 的 `AggregateVersion` 与全部 evidence event refs；score 只供 Coordination 排序，不能改变 permission、trust 或 capability lifecycle。
- 长期目标：`LongTermGoal`、`GoalCheckpoint`、`GoalLineageSnapshot`。GoalFrame、ProspectiveIntention、ExecutionRoute 与 checkpoint artifact 继续使用既有事件种类，lineage 只从事件重建。
- 能力成长：`CapabilityGap`、`CapabilityResultEvidence`、`CapabilityUpdateProposal`、`NarrowCapabilityGrant`。proposal 是 candidate；只有 owner review 后才能把 proposal 中已验证的窄 envelope 投影为 grant。
- managed plugin：`ManagedPluginBinding`、`ManagedPluginPolicy`、`ManagedPluginSnapshot`。source、manifest digest 与 signature 由外部 verifier 验证；managed deny/revoke 先于 lower layer，不能被 enable/trust/grant 放宽。
- hot/cold/sync：`MemoryTemperature`、`MemoryRetentionState`、`MemoryTierEntry`、`HotColdMemorySnapshot`、`SyncPeer`、`SyncRedactionPolicy`、`SyncWriteBatch`、`SyncTransferBatch`、`SyncApplyReport`。冷层与 peer 都只持有 event-derived projection；中心 event log 仍是唯一事实源。
- `ConfigCheck::Sync` 在 C 波 additive 激活。`EventKind::ALL.len()` 仍为 86。

## 2. 既有事件的 additive payload

以下字段全部使用 serde default，旧事件缺字段时读为 `None`，不得凭空补造 M2-C 事实：

- `GoalFramedPayload.long_term: Option<LongTermGoal>`。
- `ProspectiveIntentionCreatedPayload.goal_frame: Option<GoalFrameRef>`。
- `DecisionTraceRecordedPayload.resource_graph_snapshot: Option<ResourceGraphSnapshotRef>`。
- `OrchestrationRouteCreatedPayload.goal_frame: Option<GoalFrameRef>` 与 `checkpoint: Option<GoalCheckpoint>`。
- `CandidateCreatedPayload.capability_update: Option<CapabilityUpdateProposal>`。
- `PluginContributionRegisteredPayload.managed_snapshot: Option<ManagedPluginSnapshotRef>`。
- `PluginToggledPayload.managed_policy: Option<ManagedPluginPolicyRef>` 与 `managed_snapshot: Option<ManagedPluginSnapshotRef>`。

M2-C 不新增 EventKind。ResourceGraph 复用 capability/action/verification/failure/revocation 事件；长期 lineage 复用 goal/intention/route/memory 事件；能力成长复用 candidate/autonomy-envelope 事件；managed plugin 复用 plugin/capability 事件；hot/cold/sync 只投影和转移已有 event。

## 3. ResourceGraph 与长期路线

- projector 只消费 store 已赋 `stream_seq` 的权威事件；聚合运算必须确定、可重建，snapshot 必须列出 aggregate version 和 evidence refs。
- 未信任 raw content 不产生图结构、关系或评分。Harness 产生的结构化 CapabilityEvidence/Verification/Failure 可作为结果证据，但其原始外部正文仍留在 quarantine。
- Coordination 先以既有 inventory/trust/capability lifecycle 限定候选，再允许 ResourceGraph score 排序；高分的未授权资源永不进入 ResourcePlan。
- 长期 goal 的每次推进仍是普通 `ScheduleCommand -> Harness run`。前台活动、budget exhausted、cancel/revoke 先于后台 continuation；situation digest 变化只产生 replan/review，不沿用旧路线静默执行。

## 4. 能力成长与 Managed Plugin

- 结果证据权重大于自评：pass/fail/unverifiable 与 owner feedback 决定 proposal 上限；self confidence 只能降低该上限。
- 一次成功、plugin 声明或自评高分不能生成 grant。proposal 经 `CandidateCreated` 持久化，owner promote 后只返回 proposal 内已经验证的低风险、有限 scope/timebox、显式 capability/action type envelope。
- 任何 CapabilityUpdateProposal 都不得携带或改写 LoopSpec、CoordinationPolicy、TrustProfile、managed policy 或默认 tool selection。
- managed plugin activation 先验证 policy version、source allowlist、deny/revoke、manifest digest 和 signature，再构造完整 contribution snapshot；所有校验通过后才切 generation。验证失败保留旧 generation，revoke 后整个 source 的 tool/skill/MCP/hook/config contribution 一次性不可见。

## 5. Expected-Version、CAS 与单 Peer

冻结的 `EventStore` trait 不改。store additive 实现：

```rust
pub trait VersionedEventStore: EventStore {
    fn aggregate_version(&self, aggregate: RunId) -> Result<AggregateVersion>;
    fn append_expected(&self, event: Event, expected: AggregateVersion)
        -> Result<ExpectedAppend>;
    fn apply_sync_batch(&self, batch: SyncWriteBatch) -> Result<SyncApplyReport>;
    fn export_sync_batch(&self, request: SyncExportRequest)
        -> Result<SyncTransferBatch>;
}
```

- `append_expected/apply_sync_batch` 在同一个 SQLite immediate transaction 内比较 aggregate version、写 event、更新 projection 与 batch/cursor ledger；不匹配返回显式 conflict，写入数为零。
- batch 绑定唯一 peer、唯一 aggregate、expected version、非空连续事件和 idempotency id。相同 batch id + 相同语义返回 Duplicate；相同 id + 不同语义拒绝。
- M2-C 只允许配置一个 owner-bound peer；`StoreOptions.sync_peer` 默认 `None`，只有 store open 时显式提供的 peer 才在 schema migration 的 immediate transaction 中绑定或与既有配置做完整语义核对。未配置实例的 apply/export/cursor 全部 fail closed，首次 sync 请求不能 TOFU 注册 peer；不同 peer 或同 ref 不同语义的 reopen 失败。
- peer 不获得 stable projection 写接口。无 expected-version 的 peer write 在 API 形状上不可表达。
- export 可携带安全 payload 或 `Redacted` marker。raw input/model delta/tool args/action output 与命中 SecretRef/credential/secret marker 的 payload 必须 redacted；redacted transfer 不能反向导入成权威事件。

## 6. Legacy Decode 与 Replay

- M0/M1/M2-A/B payload 继续按原 wire 语义读取；M2-C optional 字段缺失只表示历史时没有该事实。
- `ConfigCheck::Sync` 只从 C 波 reader 起可解码；A/B compatibility tests 改为断言旧值仍可解码和 EventKind 仍为 86，不再断言 Sync 未激活。
- 历史事件不重写。ResourceGraph、goal lineage、capability proposal、hot/cold index 和 sync cursor 都能从 authoritative events + additive ledger 重建；冷归档、plugin registry 或 peer 私有状态不能覆盖历史语义。
- sync transfer 的 redaction 不修改中心 event；它只改变跨设备视图。SecretRef id 和 resolved value 都不得出现在 transfer、trace 或验收 artifact。

## 7. Contract Tests

- S48：同一事件集两次重建相同 graph；DecisionTrace 绑定 snapshot；高分未授权资源不被选择；untrusted raw content 不增边/分数。
- S49：goal/intention/route/checkpoint lineage 可在重启后重建；前台优先；situation drift 进入 replan；cancel/revoke/budget 后无新 action。
- S50：多次结果证据形成 candidate proposal；自评只能压低；owner approve 只产生窄 grant；一次成功/reject 不改 trust/permission/spec。
- S51：source/signature/allow/deny/revoke 全覆盖；验证失败保留旧 generation；成功 reload 原子切换；revoke 无 ghost contribution。
- S52：hot/cold 同源重建、selective recall、retention/redaction；显式 peer 预配置/TOFU 拒绝、单 peer CAS conflict、原子 batch、duplicate idempotency、断线 cursor 与 SecretRef scan。
