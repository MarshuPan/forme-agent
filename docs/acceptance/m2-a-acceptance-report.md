# M2-A 验收报告

- 验收日期：2026-07-16
- 工程规模：18 crates；218 Rust tests；9 compliance fixtures
- 权威规格：`docs/requirements/08-m2-verification-strategy.md` S38-S42；`docs/prd/19-m2-program-prd.md` §4
- 统一入口：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m2.ps1`
- 最终结果：**M2-A 验收 PASS - S38-S42、S1-S37、真实浏览器 golden 与原创合规门全绿**

本报告只封版 M2-A 对外执行 backend。M2-B connector/真实沟通与 M2-C 认知、协调、记忆加深仍保持冻结；未经 owner review 不进入下一波。M2 整体尚未完成。

## 1. 场景证据

下表记录测试直接断言的事件顺序、外部地面真值和反向条件。driver 返回值、页面文案或最终回答均不单独计作通过。

| 场景 | 执行证据 | 事件序列断言 | 权威状态与反向断言 | 结果 |
|---|---|---|---|---|
| S38 BrowserBackend 真实动作 | `forme-execution::s38_browser_backend_emits_untrusted_typed_receipt_and_binds_plan`；`forme-harness::s38_real_browser_golden_runs_harness_approval_and_observes_server_mutation`；browser origin-guard unit contract | 真实链按序包含 `ToolCallProposed -> ToolPolicyEvaluated{ask} -> ApprovalRequested -> RunWaiting -> ApprovalResolved -> RunResumed -> ToolPolicyEvaluated -> CompetenceGateEvaluated -> ActionPlanned -> ActionStarted -> ActionOutputDelta -> ActionCompleted -> CapabilityEvidenceRecorded -> VerificationStarted -> VerificationFinished -> RunComplete` | 安装的真实 Chrome 访问 repository-owned loopback fixture；审批前 mutation=`0`，L5 one-shot 审批后严格为 `1`。plan digest 绑定 Browser spec；批后 target 漂移时事件为空且 driver 不再执行。CDP Fetch guard 拦截不在 allowlist 的重定向与子资源 origin；`file:` 和带 credential 的 URL 失败关闭 | PASS |
| S39 ComputerBackend 受治理动作 | `forme-execution::s39_computer_backend_rejects_out_of_bounds_before_driver_effect`；`forme-policy::s38_s40_external_parameter_recheck_is_fail_closed` | 合法 recording driver 路径为 `ActionStarted -> ActionOutputDelta -> ActionCompleted -> CapabilityEvidenceRecorded`；越界路径在 `ActionStarted` 前终止 | surface、bounds、坐标、input ref、按键和 artifact scope 进入 typed plan/digest；越界坐标、未授权 surface 或超出 grant 的 bounds 均不调用 driver。Windows `NativeComputerDriver` 只实现有限动作，production 默认不注册；测试不控制 owner 当前桌面 | PASS |
| S40 PtyBackend 真实 PTY | `forme-execution::s40_pty_backend_uses_real_pty_minimal_env_and_secret_ref`；PTY fixture binary；policy fail-closed contract | 成功：`ActionStarted -> ActionOutputDelta* -> ActionCompleted -> CapabilityEvidenceRecorded`；timeout：`ActionStarted -> ActionFailed -> CapabilityEvidenceRecorded`；cancel：`ActionStarted -> ActionCancelled -> CapabilityEvidenceRecorded` | `portable-pty` 运行 repository-owned fixture，输入输出具备真实终端语义；父环境清空，secret 只经 `SecretRef` 解析，跨 chunk 明文被 redactor 截断，event/result 无 secret value。program/root/env 越界被拒；timeout、cancel、strict-output failure 都终止 child，且无 `ActionCompleted` | PASS |
| S41 外部动作治理与恢复 | `forme-harness::s41_s42_external_floor_is_plan_bound_and_harness_stamps_untrusted`、`s41_explicit_narrow_l4_requires_result_evidence_and_skips_per_action_approval`、`s41_intent_cannot_claim_a_reversible_boundary_the_backend_did_not_declare`、`s41_l5_rejects_session_grant_without_calling_backend`、`s41_external_started_without_terminal_recovers_unknown_and_never_retries_backend` | 默认/L5 路径先 `ApprovalRequested -> RunWaiting`，批准后才 `ApprovalResolved -> RunResumed -> ToolPolicyEvaluated -> CompetenceGateEvaluated -> ActionPlanned -> ActionStarted`；恢复路径只追加一次 `ActionOutcomeUnknown -> RunWaiting` | external 默认至少 L3；high-risk、ExternalCommit 或 plan rollback 不成立为 L5 one-shot。L4 只在 low risk、窄 scope、显式 reversible plan/envelope、CapabilityEvidence/VerificationEvidence 和 L4 ceiling 同时满足时免逐次审批。intent 自称可回滚不能覆盖 backend plan；L5 session grant 无 `ActionStarted`；manual probe 后恢复时 backend 执行数仍为 `0` | PASS |
| S42 Provenance、Injection 与 SecretRef | S41/S42 Harness contract；protocol legacy/default contracts；PTY cross-chunk secret fixture；offline artifact validator | external `ActionOutputDelta`、`ActionCompleted`、`CapabilityEvidenceRecorded` 由 Harness 盖 `Untrusted` provenance；payload/receipt trust 同为 `Untrusted` | recording driver 即使自报 `OwnerInput` 也被覆盖；注入文本未产生 `CandidateCreated/CandidatePromoted/UserAttributeCandidateCreated/CognitiveMapUpdateProposed/MemoryNode*/MemoryEdge*/MemoryMaintenanceApplied`。legacy output 缺 trust 时安全默认 `Untrusted`；typed eval/trace 不含 URL、可执行路径、环境、raw DOM/transcript、credential 或 secret marker | PASS |

## 2. 冻结契约与运行边界

- `EventKind::ALL.len()` 保持 86；M2-A 只 additive 扩展 `BackendKind`、`ActionParameters`、`ConfigCheck` 和 action output/completion payload，legacy decode 有安全默认。
- workspace 保持 18 crates，内部依赖边与 M1 基线完全相同；Browser/Computer/PTY 都实现既有 `ActionBackend`，Gateway/model/driver 无第二执行路径。
- plan digest 覆盖完整 `ActionIntent`，包括 driver、target/surface/program、operation、input/`SecretRef`、scope、预算、timeout 和 backend 声明的 rollback boundary。
- Harness 在执行前再次做 capability/policy/envelope 检查；CompetenceGate 读取结果证据。L4 的 rollback 判断以最终 `ExecutionPlan` 为事实，intent 自报不能抬高权限。
- M1 owner-local notification 保持冻结的本地投递 guard；它不被归成 external counterparty action，也不能作为 Browser/Computer/PTY/API/对外发言的 L3/L5 例外。
- Browser 使用隔离 profile、清空父环境并通过 CDP request interception enforce origin allowlist；不提供 JS 执行、下载执行或浏览器规划智能。
- Computer production driver 需显式构造并注册；无 surface/bounds/config 即不可用。
- PTY 不 fallback shell、不继承全量环境；所有 post-start 错误必须形成 terminal action event 和 capability evidence，child 由 guard 收尾。
- ConfigDoctor 从 10 行扩为 13 行；Browser/Computer/PTY disabled 且未注册为显式 PASS，required/unavailable/allowlist/artifact/SecretRef/approval posture 缺失为阻断性 FAIL。

## 3. 真实世界 Golden 与离线工件

- typed eval：`eval:m2-a-real-browser-mutation`
- case：`case:m2-a-real-browser-mutation`
- run：`run:1784201579952916300:1`
- 权威事件：35 条，`stream_seq=1..35` 连续；typed report 的 35 个 `trace_refs` 与 portable trace 的 event ids 一一对应。
- 地面真值：真实安装的 Chrome 进程；one-shot approval；server mutation count=`1`；receipt effect=`Committed`、trust=`Untrusted`。
- 工件：`docs/acceptance/m2-a-browser-golden-report.json` 与 `docs/acceptance/m2-a-browser-golden-trace.json`。
- artifact gate 逐字段和 marker 扫描，禁止 endpoint、URL、浏览器 executable、本机 path/environment、Authorization、credential、secret、raw DOM/PTY transcript 进入工件。

## 4. 依赖与原创合规

- 新直接依赖精确锁定：`headless_chrome=1.0.22`、`portable-pty=0.9.0`、`url=2.5.8`、Windows-only `enigo=0.6.1`、`xcap=0.9.6`。
- source、license、copy risk、implementation rule、NOTICE action 已写入 `docs/compliance/third-party-dependencies.md` 与 `compliance/borrowing-records.json`。
- 实现只使用公开 crate API；Browser/Computer/PTY adapter、错误文案、fixture、测试和 golden 页面均为 repository-owned。
- 9 个自建 compliance fixtures 与真实树 upstream/license/copy 三门全部 PASS。

## 5. 最终门结果

通过：

```text
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets            # 218 passed; real-browser golden intentionally ignored here
cargo test ... m2_a_browser_golden -- --ignored # 1 real Chrome golden passed
py -3 -m unittest discover -s tools/tests -v    # 9 passed
powershell ... tools/verify-m2-workspace-contract.ps1
powershell ... tools/verify-m2-a-artifacts.ps1
bash tools/compliance-doctor.sh
git diff --check
```

`tools/verify-m2.ps1` 先执行固定的 M1 S1-S37 回归门，再执行 M2-A protocol/S38-S42/真实 Chrome/workspace/artifact/strict-clippy 门，最终输出：

```text
M2-A ACCEPTANCE: PASS (S1-S42 + real browser golden + artifacts + strict clippy)
```

结论：**M2-A 验收 PASS，等待 owner review。M2-B/M2-C 未激活，M2 整体未完成。**
