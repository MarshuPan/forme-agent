# M1-B 验收报告

- 验收日期：2026-07-14
- 工程规模：18 crates；194 Rust tests；9 compliance fixtures
- 权威规格：`docs/requirements/06-m1-verification-strategy.md` S29-S33；`docs/prd/18-m1-program-prd.md` §5
- 统一入口：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m1.ps1`
- 最终结果：**PASS - S29-S33、M1-A/M0 回归与原创合规门全绿**

本报告只封版 M1-B 后台与主动工作流。M1-C context/capability efficiency 尚未实现，M1 整体仍未完成。

## 1. 场景证据

下表记录测试直接断言的事件顺序、权威状态和反向条件。页面状态只作为进程/浏览器补充证据，不替代事件与 store 断言。

| 场景 | 执行证据 | 事件序列断言 | 权威状态与反向断言 | 结果 |
|---|---|---|---|---|
| S29 到期 Background Intention | `forme-harness::s29_due_background_intention_creates_one_governed_schedule_run`；Gateway E2E `jobs_api_persists_across_daemon_restart_and_cancel_prevents_a_run` | intention aggregate：`ProspectiveIntentionCreated -> MemoryMaintenanceApplied{claim} -> ProspectiveIntentionResolved{fired} -> ProspectiveIntentionResolved{done}`；schedule run 以 `RunAccepted{source=schedule} -> SessionBound` 开始，以 `RunComplete` 结束；Commitment delivery 还直接断言 `ProactiveProposalEmitted -> ToolCallProposed -> ToolPolicyEvaluated -> ActionPlanned -> ActionStarted -> ActionCompleted -> RunComplete` 均存在 | due intention 只生成一个 deterministic run，job 投影为 `Done/Complete`；not-due tick 无 run；缺失授权 capability 的 command 在 schedule 边界被拒；HTTP mutation 缺 CSRF 为 403 | PASS |
| S30 重启、Lease 与重复 Intent | `forme-harness::s30_restart_reclaims_safe_lease_and_duplicate_intent_never_repeats_delivery`、`s30_unknown_schedule_outcome_enters_manual_review_without_backend_retry`；`forme-memory::scheduled_intention_binding_and_lease_survive_rebuild` | 安全 crash：`ProspectiveIntentionCreated -> MemoryMaintenanceApplied{lease}`，lease 到期后由重建投影 re-claim；unknown fixture：`RunAccepted -> ActionStarted -> ActionOutcomeUnknown -> RunWaiting` | 两个 live memory handle 并发 claim 的 winner 总数严格为 1；重建保留完整 binding/seed/expiry/lease；重复 intent coalesce，created event 和 notification side effect 都只有 1 次；已有 `ActionStarted` 无 terminal 时进入 manual review，sink delivery 为 0，不盲重试 | PASS |
| S31 Foreground Priority | `forme-harness::s31_foreground_precedes_schedule_and_budget_or_cancel_blocks_new_actions` | 前台活跃时 tick 只把 intention 记入 `deferred`，无 schedule run；前台 `RunComplete` 后下一 tick 才启动后台；零预算后台产生 `RunLimited` 且无 `ActionStarted` | 同 session 后台不抢占前台；budget=0 不触发 notification；owner cancel 后 intention 投影为 `Cancelled`，到期 tick 仍无 run；后台未写前台稳定态或 approval channel | PASS |
| S32 主动 Follow-up | `forme-harness::s32_failure_followup_uses_three_gates_attention_budget_and_reject_suppression`、`s32_verification_fail_and_unverifiable_followups_read_result_evidence` | high-impact failure 精确序列：`ObservationRecorded -> OpportunityDetected -> ValueGateEvaluated -> ImpulseRaised -> CompetenceGateEvaluated -> DecisionTraceRecorded -> ProactiveProposalEmitted -> ProactiveProposalResolved`；verification fail/unverifiable 各产生同构 follow-up | quiet hours 将 delivery 降为 `Hitchhike` 且 `attention_cost=0`；三门 guard 有明确结果；high-impact failure 只进入 `failure_evidence`，不伪装成 verification ref；fail/unverifiable 由各自 `VerificationFinished` ref 主导，pass 不触发；reject 后无 action，下一 tick 被 suppression 挡住 | PASS |
| S33 Local Notification / Approval | `forme-harness::s33_local_notification_is_plan_bound_and_high_risk_waits_for_approval`；`forme-execution::notification_target_scope_and_body_ref_are_bound_by_the_plan_digest` | 低风险严格偏序：`ToolCallProposed -> ToolPolicyEvaluated -> ActionPlanned -> ActionStarted -> ActionCompleted`；高风险：`ApprovalRequested -> RunWaiting -> ApprovalResolved -> RunResumed -> ActionStarted -> ActionCompleted -> RunComplete`，并直接断言 `ApprovalResolved` 早于 `ActionStarted` | notification 只保存 safe `body_ref`；plan digest 绑定 backend、surface、scope、title、body ref 与 permissions；篡改 target/scope/digest 的 approval 被拒，事件流零变化、delivery 计数不变；合法 one-shot approval 后只投递一次 | PASS |

## 2. Scheduler 与协议证据

- `forme-protocol` 增加 versioned schedule command/binding/config、claim/job/tick/recovery DTO；所有边界有 round-trip、空值、零 tick/lease/max claims 与 expiry/timebox 校验。
- M1-B 没有新增 EventKind，`EventKind::ALL` 仍为 86；旧 M0 proactive/intention payload 通过 `m1_b_additive_payload_fields_keep_m0_events_readable` 继续可解码。
- `ProspectiveIntentionCreated.schedule` 保存 session/envelope/budget；claim generation 使用 store domain idempotency key，两个独立 handle 不能同时成为 winner。
- scheduler daemon 只负责 tick；schedule/list/cancel/claim/recover mutation 均委托 Harness façade 与 memory owner，Gateway 不直读 SQLite、不拥有第二份 job truth。
- BackgroundProactive golden case 先走 `SchedulerGatewayControl::schedule`，再 tick 真实 schedule run；不再直接提交伪造的 `source=Schedule` request。

## 3. 治理与恢复证据

- due claim 在启动 run 前重查 trigger、expiry、scope、AutonomyEnvelope、budget 与 foreground session；action 前再次重查 capability/policy/envelope。
- Commitment reminder 和 verification/high-impact failure follow-up 都经过 Observation、ValueGate、CompetenceGate、DecisionTrace 与 proposal feedback 链。
- `AttentionBudget` 区分 interrupt 与 hitchhike；quiet hours、rate limit 和 reject suppression 均不会产生 action side effect。
- Notification backend 是普通 `ActionBackend`；immutable plan digest 与 one-shot approval 复用 M0 执行咽喉，target/scope 变化不能复用旧批准。
- startup recovery 将安全的未开始 lease 归为 reclaimable；任何已开始但结果未知的外部效果都追加 `ActionOutcomeUnknown` 并进入 `RunWaiting`/manual review。

## 4. Gateway 与浏览器实弹

- 进程级 Gateway E2E 使用真实 `forme-gatewayd`、文件 SQLite 和 daemon restart：due job 完成后可读完整 run events；pending job 重启后继续完成；取消 job 保持 `run=None/run_status=None`。
- Web Console Jobs 页真实连接 `http://127.0.0.1:8153/`：一个等于输入 `min`、但提交时已过期的 due 值被 `Date.now()` 重查拦截，显示 `Due time must be in the future.`，job 列表仍为空。
- 未来任务先显示 `Pending | Not started`；取消后显示 `Cancelled | Not started`，Cancel command 消失且无 run。
- `390 x 844` 移动视口下 document `scrollWidth == clientWidth == 375`，无页面横向溢出；job form/list 宽度约 331px，完整落在视口内；浏览器 console warning/error 为 0。

## 5. ConfigDoctor、兼容与合规

- ConfigDoctor 增加 scheduler enabled/tick/lease/max claims 与 notification enabled/availability/permission 检查；每个 finding 仍带阻断性、原因和修复建议。
- M1-B protocol migration/replay 边界记录在 `docs/architecture/m1-b-protocol-compatibility.md`；本波没有新增第三方 runtime dependency。
- compliance doctor 的 9 个自建 fixture 全绿；真实树 upstream、license、copy 三项均 PASS。
- `node --check crates/gateway/assets/app.js` PASS；Jobs UI 使用 DOM `textContent`/`replaceChildren`，没有 `innerHTML` 注入路径。

## 6. 最终门结果

统一脚本最终输出：

```text
[S23] PASS
...
[S33] PASS
[S1] PASS
...
[S22] PASS
[PASS] upstream
[PASS] license
[PASS] copy
compliance-doctor: PASS
M0 ACCEPTANCE: PASS (S1-S22 + compliance)
M1-B ACCEPTANCE: PASS (S23-S33 + S1-S22 + compliance)
```

同时通过：

```text
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace --all-targets   # 194 passed
py -3 -m unittest discover -s tools/tests -v  # 9 passed
node --check crates/gateway/assets/app.js
bash tools/compliance-doctor.sh
```

当前 GNU Rust toolchain 未安装 `cargo-clippy`，且仓库正式门不包含 clippy，因此本次未把它列为验收条件。

结论：**M1-B 完成；下一波为 M1-C（S34-S37）。**
