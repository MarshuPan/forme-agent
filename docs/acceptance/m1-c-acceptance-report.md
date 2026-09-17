# M1-C 验收报告

- 验收日期：2026-07-14
- 工程规模：18 crates；203 Rust tests；9 compliance fixtures
- 权威规格：`docs/requirements/06-m1-verification-strategy.md` S34-S37；`docs/prd/18-m1-program-prd.md` §6
- 统一入口：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m1.ps1`
- 最终结果：**PASS - S34-S37、M1-A/B/M0 回归与原创合规门全绿**

本报告封版 M1-C context/capability efficiency。至此 M1-A、M1-B、M1-C 三个波次 gate 均完成；但 M1 final 仍要求一次真实配置模型的 golden-task trace/eval report。本次 LongContext golden 使用 repository-owned `ScriptedModelProvider` 验证真实 Harness 路径，不能替代该最终条件，因此尚不生成 `docs/acceptance/m1-acceptance-report.md`。

## 1. 场景证据

下表记录测试直接断言的事件顺序、权威状态和反向条件。token 下降、搜索命中或最终回答都不能单独替代 lineage、store/projection 与副作用前阻断证据。

| 场景 | 执行证据 | 事件序列断言 | 权威状态与反向断言 | 结果 |
|---|---|---|---|---|
| S34 Automatic Compaction | `forme-harness::s34_automatic_compaction_preserves_lineage_and_done_contract`；`forme-context::tests::s34_automatic_compaction_is_threshold_bound_and_keeps_governance_lineage` | 严格顺序 `CompactionStarted -> CompactionFinished -> ContextBuildFinished` | 只在 threshold pressure 触发；在 clone 上验证后才切换；`after_tokens < before_tokens` 且降到 trigger threshold 以下；同 run 后续再次超阈值可再次压缩；preserved refs 覆盖 approval、action/tool outcome、unknown outcome、verification/failure/capability evidence、candidate/retraction、Goal/Resource/DoneContract/Envelope/DecisionTrace 与既有 compaction；无 pressure 时零 compaction event；压缩后 verifier 仍引用原 DoneContract | PASS |
| S35 Skill Progressive Disclosure | `forme-capabilities::s35_skill_search_is_bounded_explained_and_loads_only_the_selection`；`forme-harness::m1c_long_context_golden_task_passes_the_real_harness_path` | `SkillMetadataExposed(selected hit) -> SkillBodyLoaded(selected)`，随后才进入 context build/compaction | 搜索按 registry cap 与 query limit 双重限界，结果带 score/matched terms/explanation；disabled/untrusted 不可搜索或加载；unselected body 无 event 且不进 context；Harness 只装载排名后明确选择的一份正文 | PASS |
| S36 MCP Dynamic Discovery | `forme-harness::s36_mcp_refresh_search_schema_digest_and_execution_recheck_are_governed`；`forme-protocol::m1_c_mcp_schema_digest_is_additive_and_old_parameters_remain_replayable`；execution MCP contracts | registry/capability/run 三段共同断言 `McpDiscovered -> CapabilityIndexed -> ToolsetResolved -> ToolCallProposed -> ToolPolicyEvaluated -> ActionPlanned -> ActionStarted -> McpCallEvent -> ActionCompleted` | discovery 只暴露 allowlisted metadata；搜索结果不含 schema；选中后才解析 object schema 并生成 SHA-256 `SchemaDigest`，digest 进入 immutable plan；无 capability rechecker、provider grant 收窄、schema refresh 失配均在 `ActionStarted` 前产生 deny/failure，且无 `McpCallEvent`；disabled server 搜索为空；旧 `schema_digest=None` 可 replay 但 backend 不执行 | PASS |
| S37 Plugin Lifecycle / Scoped Review | `forme-capabilities::s37_plugin_reload_is_atomic_and_runtime_failure_is_isolated`；`forme-memory::tests::s37_scoped_memory_and_candidate_review_never_pollute_broader_stable_state` | plugin lifecycle 产生 `PluginContributionRegistered`、`PluginToggled` 与 capability/toolset events；review 只产生 `CandidateCreated/UserAttributeCandidateCreated -> CandidatePromoted{owner}` | reload 先验证全量 contribution snapshot；无效 manifest 或跨来源重复 hook 保留旧 generation、tool 与 hook，不部分切换；runtime failure 只移除失败 plugin，其他 plugin 仍 Active；scoped search 拒绝 session 向 project 反向越界；topic summary 保持 candidate；system promotion 被拒，owner promotion 也只写原 session scope，不污染 project/global stable | PASS |

## 2. Context 与 Golden Path

- Harness 在每次 loop drive 前按当前 context pressure 判断是否压缩，不把“一次已压缩”当作整个 run 的永久豁免。
- 压缩若不能同时降低 token、落入 usable budget 且回到 threshold 以下，会返回显式 context error，不切换到一个仍会立即重压缩的 context。
- `m1c_long_context_golden_task_passes_the_real_harness_path` 从 `evals/m1/golden-tasks.json` 读取 LongContext case，经真实 `ReactiveHarness -> selected skill load -> context compaction -> model loop -> verifier -> ManualEvalReport`，结果为 `Pass`。
- golden trace 同时包含 `SkillMetadataExposed`、唯一一条 selected `SkillBodyLoaded`、`CompactionStarted/Finished`、`VerificationFinished`，且没有 `CandidatePromoted`。
- 该 case 使用 scripted provider 以保证回归确定性；它证明 golden/manual-eval 管道和 Harness 集成，不计作 M1 final 的真实配置模型证据。

## 3. MCP、Plugin 与 Memory 治理

- MCP metadata refresh 会清除旧 selected schema/digest；stale intent 即使 policy 仍允许，也会因 current schema digest 不一致而在执行前停止。
- execution-time recheck 同时检查 provider lifecycle/current grant、allowlist、current schema、policy visibility/permission/scope 与 AutonomyEnvelope；缺少 rechecker 本身即为 deny。
- plugin capability 与 hook replacement 在 mutation 前完成全量验证；失败 reload 保留上一代安全 snapshot，runtime failure 则 clean disable 该 source，避免 ghost capability。
- scoped memory query 同时绑定 requester、target、workspace、time slice 和 scope containment；跨 workspace、窄 scope 读取宽 scope 默认拒绝。
- topic summary 通过既有 candidate lifecycle 表达，不覆盖 raw event/history；UserModel conflict 仍需 owner promotion，且 stable 写入不扩大 scope。

## 4. Config、协议与架构边界

- ConfigDoctor 增加 `ConfigCheck::Context`，检查 1-100 compaction threshold、非零 max tokens 与小于上限的 reserve；enabled MCP 还必须有非空 allowlist。每个 finding 保留原因和修复说明。
- M1-C 只增加 `SchemaDigest`、optional `ActionParameters::Mcp.schema_digest` 与 additive `ConfigCheck::Context`；兼容边界记录在 `docs/architecture/m1-c-protocol-compatibility.md`。
- `EventKind::ALL` 与 contract snapshot 均保持 86；旧 MCP payload 无 digest 仍可解码和 trace/replay，但不能进入新执行路径。
- `forme-capabilities` 复用已登记的 `sha2 0.10.9` 公共 digest API；dependency/license 记录已存在于 `docs/compliance/third-party-dependencies.md`。
- `cargo metadata --no-deps` 审计确认内部 crate 依赖边未增加；store 仍为事件单写者，Harness 仍为执行咽喉。

## 5. 最终门结果

统一脚本最终输出：

```text
[S23] PASS
...
[S37-memory] PASS
[M1-C-golden] PASS
[S1] PASS
...
[S22] PASS
[PASS] upstream
[PASS] license
[PASS] copy
compliance-doctor: PASS
M0 ACCEPTANCE: PASS (S1-S22 + compliance)
M1-C ACCEPTANCE: PASS (S23-S37 + S1-S22 + compliance)
```

同时通过：

```text
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace --all-targets   # 203 passed
py -3 -m unittest discover -s tools/tests -v  # 9 passed
bash tools/compliance-doctor.sh
git diff --check
```

当前 GNU Rust toolchain 未安装 `cargo-clippy`，仓库正式门也不包含 clippy；本次尝试返回 component missing，而非 lint finding。

## 6. M1 Final 待办

当前进程没有配置 `FORME_MODEL_BASE_URL`、`FORME_MODEL_NAME`、`FORME_MODEL_API_KEY`，所以本轮不能生成真实模型 golden report。M1 final 还需：

1. 用真实配置模型运行至少一个 repository-owned golden case。
2. 导出可追溯到 run/event snapshot 的 `ManualEvalReport`，且 artifact 不含 secret。
3. 复跑 S1-S37 与合规门后生成 `docs/acceptance/m1-acceptance-report.md`。

结论：**M1-C 完成；M1-A/B/C 三个波次 gate 全部完成。M1 final 仅剩真实配置模型 golden 证据与最终汇总报告，尚不宣称 M1 整体完成。**
