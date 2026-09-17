# M3-B Domain 策略与长期任务验收报告

- 验收日期：2026-07-18
- 验收范围：M3-B only；M3-C 未激活
- 工程规模：18 crates；89 EventKinds；292 个 listed Rust tests（291 个常规 + 1 个真实浏览器 ignored test 单独实跑）；9 compliance fixtures
- 权威规格：`docs/requirements/10-m3-verification-strategy.md` S58-S62；`docs/prd/20-m3-program-prd.md` §4
- 最终结果：**PASS - 等待 owner review**

## 1. S58-S62 逐场景证据

| 场景 | 可执行证据 | 事件序列断言 | 反向断言 | 结果 |
|---|---|---|---|---|
| S58 Loop strategy | `forme-loop::s58_loop_registry_is_immutable_seeded_and_only_narrows_runtime_budget`、`s58_run_binds_one_loop_version_and_cannot_hot_swap`；M3-B Harness integration | live run 先 `RunAccepted -> SessionBound{evolution_snapshot}`，再进入 `ToolsetResolved -> GoalFramed -> ... -> VerificationFinished -> RunComplete`；Loop budget 在 SessionBound 前解析并实际收紧 `RunCtx` | seed 永久可解析；same version/different digest、未知/重复 phase、零/溢出/unbounded budget 拒绝；strategy 不能扩大 runtime hard budget，已绑定 run 不能热换 | PASS |
| S59 Coordination strategy | `forme-coordination::s59_coordination_strategy_selects_existing_roles_without_expanding_child_authority`、`s59_lower_cost_never_outvotes_incomplete_quality_or_over_delegation`；S62 child route | `SessionBound -> GoalFramed -> OrchestrationRouteCreated -> SubagentSpawned -> SubagentResultReturned -> MemoryNodeAppended -> VerificationFinished -> RunComplete`；child 由 Harness 建 run | strategy 只能选择/重排现有 route node；permission/toolset/model/budget 不变；低成本不能覆盖 correctness、verification、handoff failure 或 over-delegation | PASS |
| S60 Selection policy | `forme-capabilities::s60_filters_authority_and_managed_state_before_strategy_ranking`、`s60_provider_declaration_never_raises_the_result_evidence_ceiling`、`s60_selection_registry_keeps_version_content_immutable`；M3-B Harness integration | `SessionBound -> ToolsetResolved -> GoalFramed -> ResourcePlanned -> DecisionTraceRecorded{evolution_snapshot,selected refs}` | lifecycle/managed policy/permission/scope/authorized set 在 strategy ranking 前过滤；provider `declared` 自报不增加结果证据分数；ranking 不注册新 capability 或扩大 scope | PASS |
| S61 Model adaptation | `forme-models::s61_weak_and_strong_profiles_change_scaffolding_but_not_governance`、`s61_provider_strength_cannot_replace_measured_outcome_evidence`、`s61_model_adaptation_registry_rejects_same_version_different_digest`；M3-B Harness integration | bound scaffold 进入固定模型输入；该 golden 按 active spec 产生两轮 `VerificationStarted -> VerificationFinished`，DecisionTrace 保存同一 snapshot | effective capability=`min(provider profile, measured outcome)`；无 verified outcome 不绑定 adaptation；强模型不能移除高影响 trace/approval/verification；不兼容 active content 为 `RunAccepted -> FailureEvidenceRecorded -> RunFailed`，且无 `SessionBound` | PASS |
| S62 Bounded long horizon | `forme-harness::s62_long_horizon_checkpoints_yield_to_foreground_pin_versions_and_stop_cleanly`；`forme-eval` 两个 artifact contracts | 三个普通 Harness checkpoint 分别绑定 Loop `v1 -> v2 -> v1`；每段含 `SessionBound -> GoalFramed -> OrchestrationRouteCreated -> SubagentSpawned/Returned -> MemoryNodeAppended -> VerificationFinished -> RunComplete`。checkpoint 0 额外 `RunWaiting -> RunResumed`；唯一 outward run 含 `ApprovalRequested -> ApprovalResolved -> CompetenceGateEvaluated -> ActionPlanned -> ActionStarted -> ActionCompleted -> VerificationFinished` | foreground defer 不建 run；approval 前 backend calls=0；one-shot plan-bound approval 后恰执行一次；active 变化只影响下一 checkpoint；rollback 后恢复 v1；cancel 后 run/action 计数不增长；tamper、额外目录项、secret/private path 均拒绝 | PASS |

S1-S57 不是抽样运行：`tools/verify-m3.ps1` 首先完整调用 `tools/verify-m2.ps1`，再执行 M3-A 精确场景。因此 M0-M2、M3-A、真实 Browser/API/communication、M1/M2/M3-A typed artifacts、MCP race regression、CAS、rollback 与合规门均为本波永久回归证据。

## 2. 冻结契约与治理边界

- `EventKind::ALL.len() == 89`；M3-B 不新增、重排或改名任何事件。
- workspace 保持 18 crates，内部依赖边无新增、无反向；冻结的 `LoopDriver`、`CoordinationReasoner`、`ActionBackend`、`EventStore` trait 未改签名。
- 四类 domain spec 均带 schema/version/scope/content ref/digest/runtime compatibility，使用 strict `deny_unknown_fields`；permission、approval、L5、audit disable、fixed identity 等治理字段 fail closed。
- Loop/Coordination/Selection/Model registries 保存 immutable content；同一 version 不能绑定不同 digest。active pointer 仍由 M3-A event-derived projection 持有，domain crate 不拥有 writer。
- Harness 在 `SessionBound` 前一次性解析 version/content ref/digest/compatibility，随后把完整 evolution snapshot 固定到 run 与 DecisionTrace。schema/model/tool/backend 漂移在 bind 前失败。
- subagent 只经 Harness spawn，继承父 run 已固定的 snapshot，不重新读取 active pointer；child workspace、permission、toolset、model 与 budget 保持隔离和原边界。
- Loop strategy 只能收紧 hard budget；Coordination strategy 不改变 child authority；Selection 只排序预过滤集合；Model adaptation 不删除高影响治理步骤。
- outward Browser action 仍经过 policy、plan-bound approval、执行前重查、CompetenceGate、ActionBackend 与 verification；M3 strategy event 不生成 permission、grant、trust 或 L5 standing autonomy。
- M3-B 未引入第三方依赖，`Cargo.lock` 与 borrowing records 无需变化。

## 3. Long-horizon Typed Artifact

仓库工件：`m3-b-artifacts/long-horizon-1325de753c05441a.json`

| 字段 | 证据 |
|---|---|
| content digest | `fnv64:1325de753c05441a` |
| SHA-256 文件校验 | `5bc4d20ee6f64731ee98c83f5d9270676928b64ba21757ada8d86d46b02a6e67` |
| checkpoint 0 | snapshot `fnv64:c4cbeb72e3140386`；Loop v1；26 events；foreground wait/resume |
| checkpoint 1 | snapshot `fnv64:1cf5e21933f570f4`；Loop v2；24 parent events；唯一 Browser outward run 20 events |
| checkpoint 2 | snapshot `fnv64:9bf2631a3e18d07e`；owner rollback 后 Loop v1；24 events |
| terminal | project cancelled；cancel 后零新 run、零新 action |

`forme-m3-b-artifact-verify` 要求目录恰有一个 regular artifact，并独立验证 schema/content address、checkpoint index/run 唯一性、`v1 -> v2 -> v1`、foreground yield、唯一 outward lineage 及 approval/CompetenceGate/action/verification 子序列。仓库工件和全门临时重生成工件均 PASS。

敏感扫描覆盖 `SecretRef`/`ResolvedSecret`、API key/credential、Authorization/Bearer/private key，以及 Windows、Unix、UNC 绝对路径；工件只保存 typed refs、snapshot/version 和 EventKind，不保存 raw event payload、tool args、外部正文、凭据或本机路径。

M3-A 五件套 verifier 同时修复了 Windows `core.autocrlf` 回归：digest 绑定解析后的 canonical JSON，而不是平台相关的原始换行字节；LF/CRLF 回归测试、语义篡改测试与仓库五件套独立复核均 PASS。

## 4. 真实 Browser 回归

统一门单独启动本机已安装的 Chrome/Edge，复跑 repository-owned loopback mutation golden。真实 Browser 进程仍完整经过 Harness approval、plan digest、ActionBackend、untrusted typed receipt、server mutation observation、verification，以及 M3-A candidate/eval/activation/regression/rollback 五件套生成。

S62 的新增 outward step 使用 repository-owned recording Browser backend，目的是隔离证明长期 checkpoint 不会绕过审批与 CompetenceGate；它不冒充第二个真实浏览器任务。真实跨系统地面真值由上述 M2/M3-A Chrome golden 持续覆盖。

## 5. 最终门

统一入口：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m3.ps1
```

该入口依次执行：

```text
S1-S52 permanent regression via tools/verify-m2.ps1
S53-S57 M3-A exact scenario and negative contracts
S58-S62 M3-B protocol/domain/Harness/artifact contracts
89-kind protocol snapshot and frozen 18-crate graph
real installed Chrome/Edge golden and M3-A temporary five-artifact verification
M3-B temporary long-horizon artifact generation and independent verification
repository-owned M3-A and M3-B artifact verification
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
py -3 -m unittest discover -s tools/tests -v
bash tools/compliance-doctor.sh
git diff --check
```

最终输出：

```text
M3-B FINAL ACCEPTANCE: PASS (S1-S62 + 89 EventKinds + 18 crates + long-horizon artifact + compliance)
```

## 6. 残余边界

- M3-B registry 首版是进程内 immutable content registry；active truth 已事件化，但通用 strategy package distribution/remote registry 不在 M3。
- Long-horizon 首版提供 bounded project/checkpoint primitive，由上层 scheduler 驱动；不做多主任务编排、公网控制面或无限自治 daemon。
- M3-B content addressing 沿用仓库 deterministic FNV64 体系，便于稳定离线复核但不宣称密码学碰撞抗性；报告额外固定 SHA-256 文件校验。
- S62 新增 outward fixture 不是真实浏览器 driver；真实 Chrome/Edge 行为由永久 M2/M3-A golden 覆盖，M3-C final 会继续复用真实 trace 做完整 candidate-to-rollback golden。
- StrategyMemory conflict/decay、AgentSelf/Partnership、Trust/Delegation、Proactivity/Communication 策略与 release audit 属于 M3-C，未提前实现或宣称。

## 7. 结论

M3-B 已把 Loop、Coordination、Selection、ModelAdaptation 和 bounded long-horizon checkpoint 接入 M3-A 的 candidate/stable/active/run-pinning 控制面。每个 live run 都由 Harness 在 `SessionBound` 前固定兼容 snapshot，subagent 继承父 snapshot，真实 outward 行为继续经过 M0-M2 的审批与能力门。S1-S62、89 EventKinds、18-crate 冻结图、292 个 listed Rust tests、真实 Browser 回归、strict Clippy、typed artifacts 与原创合规门均满足。**M3-B 验收 PASS，停止在 owner review 边界；M3-C 未进入实现。**
