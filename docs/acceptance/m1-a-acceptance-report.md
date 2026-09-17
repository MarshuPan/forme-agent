# M1-A 验收报告

- 验收日期：2026-07-14
- 工程规模：18 crates；182 Rust tests；9 compliance fixtures
- 权威规格：`docs/requirements/06-m1-verification-strategy.md` S23-S28；`docs/prd/18-m1-program-prd.md` §4
- 统一入口：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m1.ps1`
- 最终结果：**PASS - S23-S28、S1-S22 与原创合规门全绿**

本报告只封版 M1-A 控制面与评价面。M1-B background/proactive workflow 和 M1-C context/capability efficiency 尚未实现，不计入本次完成范围。

## 1. 场景证据

下表只记录测试直接断言的事件顺序、权威投影和反向条件，不以页面文案或最终回答代替运行证据。

| 场景 | 执行证据 | 事件 / 游标断言 | 权威状态与反向断言 | 结果 |
|---|---|---|---|---|
| S23 CLI/Web 同协议 | `forme-gateway::s23_http_web_surface_uses_real_gateway_harness_and_event_stream`、`http_submit_accepts_before_the_background_model_call_completes`，并回归 M0 CLI/Harness contract | Web 真实链：`RunAccepted -> SessionBound -> GoalFramed -> ResourcePlanned -> DoneContractSet -> AutonomyEnvelopeSet -> DecisionTraceRecorded -> TurnStarted -> ContextBuildStarted -> ContextBuildFinished -> ModelCallStarted -> ModelCallDelta -> ModelCallFinished -> OutputClassified -> VerificationStarted -> VerificationFinished -> TurnComplete -> RunComplete` | HTTP `202` 在被阻塞的模型调用完成前返回；随后 summary 从非终态收敛到 `Complete`。请求真实经过 Gateway -> Harness -> model -> SQLite；无 Web 私有 loop 路径 | PASS |
| S24 Event stream 重连 | `forme-harness::s24_event_page_reconnect_is_contiguous_deterministic_and_read_only` + Gateway E2E `after`/`Last-Event-ID` | cursor 7 后严格返回 `8..snapshot_upper_bound`；同 cursor 两次响应完全相同；`stream_seq` 连续递增 | run 至少 20 事件；读取前后权威 event 流完全相同；summary cursor 等于 snapshot upper bound；不按 timestamp 排序 | PASS |
| S25 控制操作往返 | `forme-harness::s25_gateway_approval_is_one_shot_plan_bound_and_resumes_the_run`、`cancel_interrupts_an_active_action_and_finishes_without_losing_state` | approval 关键顺序：`ApprovalResolved -> RunResumed -> ActionStarted`；cancel 流包含 `ActionStarted`、`ActionCancelled`、`RunAborted` | 错误 plan digest 不追加事件、backend 计数为 0；合法 one-shot grant 只执行一次；HTTP 错误 owner 被 403 拒绝；Web 不暴露通用 `ResumeInput` | PASS |
| S26 Trace Viewer 只读 | `forme-harness::s26_trace_view_resolves_failure_and_verification_without_writing_history` + Gateway trace HTTP E2E | trace 按 `stream_seq` 折叠并解析 `DecisionTraceRecorded`、`FailureEvidenceRecorded`、`VerificationFinished` | 读取前后 event 向量逐项相等；snapshot upper bound 等于末事件；不输出或伪造 chain-of-thought；HTTP 拒绝 mutation 后 trace 仍不变 | PASS |
| S27 Candidate/Failure Review | `forme-harness::s27_candidate_review_compares_state_and_retraction_schedules_reevaluation` + candidate HTTP negative E2E | `CandidatePromoted{by=user} -> RetractionEvent -> ReevaluationTaskCreated`，后者 `caused_by` 指向 retraction | stale expected state 的第二次 promote 失败且零事件；非 owner/不存在 candidate 不改变 trace；历史不物理删除；未新增 `CandidateConfirmed` EventKind | PASS |
| S28 Manual Eval | `forme-harness::s28_manual_eval_is_repeatable_trace_bound_and_never_promotes_policy`、`forme-eval::manual_eval_never_turns_verification_failure_into_success` + eval HTTP/export E2E | 四类 repository-owned case 均真实运行；final-only = `Pass`，尚缺 M1-B/C required events 的三类 = `Fail` | 每类 report 的 trace refs 与完整 run 事件一一对应；同 snapshot 重跑、archive export 与首份 report 相等；事件不增加；无 `CandidatePromoted`；verification fail 不能变 success | PASS |

## 2. 控制面与协议证据

- `forme-protocol` 增加 versioned surface/cursor/summary/approval/review/eval DTO；serde round-trip、unknown enum、空边界、cursor overflow 和 owner/lineage 校验全有 contract tests。
- `EventKind::ALL` 保持 86。Notification 只增加协议 variant，M1-A execution 不注册 backend，继续 default-deny。
- Gateway 只依赖既有 `protocol/harness/communication` 内部边；handler 不直读 SQLite，不直调 loop/eval/memory/execution。
- `GatewayControl::start_run` 先校验、幂等归并并持久化 `RunAccepted`，再由 Harness worker 推进；同步 CLI 提交语义不变。
- one-shot approval 绑定 owner、plan digest、policy/tool schema、nonce 与 expiry；candidate review 使用 compare-current-state。
- Local Web 只调用 HTTP API，所有模型/tool/event 文本用 `textContent` 渲染，无 `innerHTML` 注入路径。

## 3. 进程与浏览器实弹

- 进程级测试使用真实 `forme-gatewayd`、loopback HTTP、真实 Harness、Chat Completions fixture server 和文件 SQLite，不用 mock controller 代替集成链。
- 认证、same-origin/CSRF、body limit、secret redaction、loopback-only bind 和 SSE cursor 有 server contract/E2E 断言；request timeout 与 rate limit 由有界配置和运行时 middleware/enforcer 落地。
- 浏览器完成 token 认证、真实 run、18-event timeline、Trace 18/0/18、final-only eval 和 daemon 重启恢复；重启后 3 个 SQLite run 均可重新列出并读取完整事件。
- 最终 1280 x 720 检查：document/body `scrollWidth == clientWidth`，18 个 event rows、3 个 run items，无页面或文本控件横向溢出。375px 基础布局在状态投影接入前已实测；最终增量复用同一 `overflow-wrap:anywhere` 与 `@media(max-width:780px)` 约束，浏览器安全策略阻止了再次创建额外窄屏上下文，未绕过该限制。

## 4. 依赖与合规

- 新增 runtime 依赖精确锁定：axum 0.8.9、tokio 1.52.3、tower-http 0.7.0、getrandom 0.4.3。
- `cargo tree -p forme-gateway -d` 无重复版本；GNU Windows 目标编译通过。
- 新依赖最高声明 MSRV 为 Rust 1.85（getrandom），低于当前 stable GNU Windows Rust 1.97。
- dependency/license/copy-risk/implementation-rule/NOTICE action 已记录在 `docs/compliance/third-party-dependencies.md`。
- compliance doctor 修正为只阻断构建文件中的真实 `research/upstreams` 路径；项目自有 `include_str!/include_bytes!` 不误报。正向阻断与反向允许 fixture 同时存在。

## 5. 最终门结果

统一脚本最终输出：

```text
[S1] PASS
...
[S28] PASS
[PASS] upstream
[PASS] license
[PASS] copy
compliance-doctor: PASS
M0 ACCEPTANCE: PASS (S1-S22 + compliance)
M1-A ACCEPTANCE: PASS (S23-S28 + S1-S22 + compliance)
```

同时通过：

```text
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace --all-targets   # 182 passed
py -3 -m unittest discover -s tools/tests -v  # 9 passed
node --check crates/gateway/assets/app.js
bash tools/compliance-doctor.sh
```

结论：**M1-A 完成；下一波为 M1-B（S29-S33）。**
