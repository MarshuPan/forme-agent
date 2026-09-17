# M2-C 验收报告

- 验收日期：2026-07-17
- 工程规模：18 crates；252 个非忽略 Rust tests；1 个真实浏览器 test 由验收脚本单独实跑；9 compliance fixtures
- 权威规格：`docs/requirements/08-m2-verification-strategy.md` S48-S52；`docs/prd/19-m2-program-prd.md` §6
- 兼容契约：`docs/architecture/m2-c-protocol-compatibility.md`
- 统一入口：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m2.ps1`
- 最终结果：**M2-C 验收 PASS - S48-S52、S1-S47、真实浏览器/API golden、artifact 与原创合规门全绿**

本报告只按权威事件、可重建 projection、SQLite 事务结果和反向断言计分。资源分数、模型自评、plugin 声明、peer 成功或最终文案均不能单独证明授权、成长或同步正确。

## 1. 场景证据

| 场景 | 执行证据 | 事件/事务序列断言 | 权威状态与反向断言 | 结果 |
|---|---|---|---|---|
| S48 ResourceGraph | `forme-coordination::s48_resource_graph_is_event_derived_deterministic_and_never_authorizes_by_score`；M2-C protocol contract | projector 只折叠 store 已赋 `stream_seq` 的 `CapabilityIndexed/CapabilityEvidenceRecorded` 等结构化事件；`DecisionTraceRecorded.resource_graph_snapshot` 绑定本次 snapshot | 同一乱序事件集两次重建完全相同，aggregate version=`7`；Untrusted `ActionOutputDelta` 不进入 graph evidence、边或分数。未授权资源即使分数更高，也先被 inventory/trust 过滤，不进入 `ResourcePlan` | PASS |
| S49 长期目标 | `forme-memory::s49_long_term_goal_lineage_rebuilds_and_yields_replans_or_stops_before_action` | 权威序列为 `GoalFramed -> ProspectiveIntentionCreated -> OrchestrationRouteCreated -> ProspectiveIntentionResolved{Cancelled}`；checkpoint 绑定 goal/intention/route/artifact/situation/evidence | reopen 后 lineage、route、checkpoint 与 aggregate version 一致；前台活动先 `YieldToForeground`，situation digest 变化为 `Replan`，budget/expiry/cancel/revoke 为 `Stop`。取消后无 `ActionPlanned/ActionStarted`，每次 continuation 仍只返回普通 scheduled Harness run 决策 | PASS |
| S50 能力成长 | `forme-memory::s50_capability_growth_is_result_led_candidate_only_and_owner_grants_narrowly`；M2-C protocol contract | review 前只有 `CandidateCreated{capability_update}`；owner promote 后为 `CandidateCreated -> CandidatePromoted` | pass/fail/unverifiable 与 owner feedback 决定 ceiling，自评只可降低；一次成功最高 L2。空/非 owner principal 无事件；批准只返回 proposal 内 low-risk、有限 scope/timebox/action 的 envelope。无 `AutonomyEnvelopeSet/PluginToggled/ToolsetResolved`，不改 LoopSpec、CoordinationPolicy、TrustProfile 或默认 permission | PASS |
| S51 Managed Plugin Policy | `forme-capabilities::s51_managed_plugin_policy_verifies_then_switches_atomically_and_revokes_without_ghosts` | 成功 activation 产生带 `managed_snapshot` 的 `PluginContributionRegistered`；revoke 产生 `PluginToggled{enabled=false,managed_policy,managed_snapshot}` | source/manifest digest/signature/allow/deny/revoke 全验证后才切 generation。坏签名或不匹配 replacement 保留旧 generation；合法 reload 原子从 v1 切 v2，无半激活；managed deny 不可被 user/workspace enable 放宽。revoke 后 tool 与 hook 均不可见，普通 `activate/reload` 旁路被拒 | PASS |
| S52 热冷记忆 | `forme-memory::s52_hot_cold_projection_is_rebuildable_scoped_retained_and_secret_free` | source 序列含 `MemoryNodeAppended* -> RetractionEvent` 与 Untrusted `ActionOutputDelta`；投影只保留 event identity/ref，不嵌入 raw delta | 正反序输入重建相同 snapshot；hot/cold、TTL `Expired`、retraction `Tombstoned` 与 `Redacted` 可断言。selective recall 只返回请求 scope 内 `Active` refs；secret-like content ref、expired/tombstoned ref 与 raw external marker 均不出 snapshot/recall | PASS |
| S52 CAS/同步 | `forme-store::s52_expected_append_uses_atomic_compare_and_zero_write_conflicts`、`s52_sync_batch_is_atomic_idempotent_single_peer_and_cursor_bound`、`s52_sync_peer_must_be_preconfigured_and_cannot_be_claimed_by_first_request`、`s52_export_redacts_secrets_rejects_redacted_import_and_resumes_from_cursor` | `append_expected` 在同一 immediate transaction 比较 version 后 append；peer 只在 store open/schema migration 事务中配置；batch 在同一事务完成 compare -> event/projection append -> batch ledger -> cursor。export 按源 `stream_seq` 输出 `Full|Redacted` | 未配置实例的 apply/export/cursor 均拒绝且不能抢占 peer；不同 peer reopen 失败。stale CAS 为 `Conflict` 且 event 数不变；合法 batch 从 version 0 到 2，重复同语义为 `Duplicate` 且无新 event，同 ID 改语义拒绝；第二 event 碰撞使首 event 一并回滚。cursor 跨 reopen 从 2 续到 5。RunAccepted/model delta/tool args/action output、SecretRef/credential marker和敏感 envelope identity不出 transfer；含任一 `Redacted` 的 transfer 不能转 authoritative batch | PASS |

## 2. 冻结契约与治理边界

- `EventKind::ALL.len()` 保持 86，旧值不重排、不改义。M2-C 只给既有 payload 增加 serde-safe optional 字段，legacy 缺字段读为 `None`，不能凭空构造新事实。
- workspace 保持 18 crates，内部依赖边未新增、未反向。`ResourceGraphProjector` 在 coordination，长期目标/能力候选/热冷投影在 memory，CAS/sync ledger 在 store，managed policy 在 capabilities。
- 冻结 `EventStore` trait 未修改；新增 `VersionedEventStore: EventStore`。SQLite database schema 从 1 迁移到 2，只新增 `sync_peers/sync_batches/sync_cursors`，历史 event 不重写；schema migration/replay 回归全绿。
- ResourceGraph score 只在可信 inventory 限定后排序，不改变 trust、permission 或 capability lifecycle。未信任网页/API/参与者正文永远不产生 graph 结构。
- CapabilityUpdateProposal 经 `CandidateCreated` 持久化；owner review 只返回窄 envelope，不自动写 policy/trust/spec。M3 的 replay/eval/promotion/rollback 和规格自动提升没有进入本波。
- managed plugin source/signature/digest verifier 是显式 trait 边界；所有 contribution 先完整构造再原子切换，失败保留旧 generation，revoke 后无 ghost contribution。
- hot/cold 是同一 event log 的可重建视图，不是第二事实源。过期和撤销通过 retention state/tombstone 表达，不删除或改写历史事件。
- sync 只支持一个在 `StoreOptions` 中预配置的 owner-bound peer；默认实例禁用 sync，首次请求没有 TOFU 注册权，既有数据库与新 peer 语义不一致时 open fail closed。无 expected-version 的第二写者在 API 形状上不可表达。CAS mismatch 写入零 event；批次 ledger 比较完整 canonical 语义字节，不以弱摘要代替相等判断。
- ConfigDoctor 从 14 行扩为 15 行。Sync row 要求 owner-bound peer、expected-version/CAS、batch idempotency、persistent cursor 与 `redact_raw_content + forbid_secret_refs`；disabled 且未配置 peer/policy 为显式 PASS。

## 3. 真实世界与离线工件

- 最终门重新实跑真实 Chrome/Edge：repository-owned loopback 页面经 Harness one-shot approval 后只产生一次 server mutation，测试 PASS。
- M2-A typed eval `docs/acceptance/m2-a-browser-golden-report.json` 与 portable trace `docs/acceptance/m2-a-browser-golden-trace.json` 继续作为 M2 real-world golden；35 条 trace refs、连续 `stream_seq`、Browser receipt 与 server mutation ground truth 可离线复核。
- `tools/verify-m2-a-artifacts.ps1` 再次验证 report/trace identity、事件顺序、mutation count=`1`、Untrusted provenance 和敏感字段/值扫描。
- M2-B 的真实 loopback API read/mutation 与真实 bounded communication delivery 在 workspace tests 和 S43-S47 精确门中再次通过；unknown outcome 仍不重试。

## 4. 最终门结果

通过：

```text
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets             # 252 passed; 1 browser golden ignored here
cargo test ... m2_a_browser_golden -- --ignored  # 1 real Chrome/Edge golden passed
py -3 -m unittest discover -s tools/tests -v      # 9 passed
powershell ... tools/verify-m2-workspace-contract.ps1
powershell ... tools/verify-m2-a-artifacts.ps1
powershell ... tools/verify-m2.ps1                # S1-S52 + real Chrome PASS
bash tools/compliance-doctor.sh
git diff --check
```

统一入口最终输出：

```text
M2 FINAL ACCEPTANCE: PASS (S1-S52 + real browser/API golden + artifacts + strict clippy)
```

结论：**M2-C 验收 PASS。M2 三波实现已闭合，等待 owner 对本波与最终报告审查。**
