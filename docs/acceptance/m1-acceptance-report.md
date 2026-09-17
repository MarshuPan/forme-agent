# M1 最终验收报告

- 验收日期：2026-07-15
- 工程规模：18 crates；203 Rust tests；9 compliance fixtures
- 权威规格：`docs/requirements/05-m1-scope-decisions.md` §8、`docs/requirements/06-m1-verification-strategy.md` §6、`docs/prd/18-m1-program-prd.md` §10
- 最终结果：**PASS - M1 完成**

## 1. 完成条件

| 条件 | 证据 | 结果 |
|---|---|---|
| M0 永久回归门 | `tools/verify-m1.ps1` 内嵌复跑 S1-S22；203 个 workspace tests 全绿；无 ignored/placeholder 场景 | PASS |
| M1 场景门 | S23-S37、M1-A/B/C 补充 contract/golden/doctor tests 全绿 | PASS |
| 三波事件、投影与反向断言 | `m1-a-acceptance-report.md`、`m1-b-acceptance-report.md`、`m1-c-acceptance-report.md` 逐场景记录事件顺序、权威 store/projection 与禁止条件 | PASS |
| 真实配置模型 golden task | repository-owned `case:m1-final-only` 经真实 Gateway/Harness/Model/Verifier 路径运行；typed report 为 `Pass`，portable trace manifest 可离线解析全部 event refs | PASS |
| Harness-first | console、scheduler、notification、plugin runtime 无旁路；内部 crate 图未增加边，EventKind 仍为 86 | PASS |
| 最终报告 | 本报告已生成并引用可复核的 typed eval artifact | PASS |

## 2. 真实模型 Golden 证据

- `eval_ref`：`eval:real:790e0d0068a24827bdb02bbc0889761d`
- `run`：`run:1784050380441257600:1`
- `snapshot`：`snapshot:real:790e0d0068a24827bdb02bbc0889761d`
- `model profile`：`configured-chat-provider:gpt-5.6-terra`
- `rubric`：`rubric:verified-complete`
- `outcome`：`Pass`
- 模型 usage：input 4567 tokens；output 10 tokens
- 权威事件流：18 条，`stream_seq` 从 1 到 18 严格连续；run projection 为 `Complete` 且 `last_stream_seq=18`
- `ManualEvalReport.trace_refs` 18 条，与事件流 18 个 `event_id` 一一相等，无缺失或额外引用
- `m1-real-model-golden-trace.json` 持久化 `stream_seq/event_id/kind` 映射、snapshot upper bound、model usage 与禁止事件断言，离开本机 SQLite 后仍可复核 report refs

事件序列：

```text
RunAccepted -> SessionBound -> GoalFramed -> ResourcePlanned -> DoneContractSet
-> AutonomyEnvelopeSet -> DecisionTraceRecorded -> TurnStarted
-> ContextBuildStarted -> ContextBuildFinished -> ModelCallStarted
-> ModelCallDelta -> ModelCallFinished -> OutputClassified
-> VerificationStarted -> VerificationFinished -> TurnComplete -> RunComplete
```

关键事件断言：

- `SessionBound` 绑定 `policy:default-deny`、真实 configured model profile、`toolset:empty` 和 `workspace:default`。
- `DecisionTraceRecorded` 携带 `workspace_snapshot`。
- `ModelCallFinished` 与报告使用同一 model profile，并记录非零 token usage。
- `VerificationFinished{outcome=Pass}` 先于 `RunComplete{stop_reason=final_output}`。
- 禁止事件 `ActionStarted`、`CandidatePromoted`、`ActionOutcomeUnknown` 均不存在。
- 导出报告和事件流不含 `FORME_MODEL_API_KEY`、Authorization bearer、gateway token 或 api_key 字段。

## 3. 场景与合规门

最终真实模型运行完成后复跑：

```text
[S23] PASS
...
[S37-memory] PASS
[S1] PASS
...
[S22] PASS
[PASS] upstream
[PASS] license
[PASS] copy
compliance-doctor: PASS
M0 ACCEPTANCE: PASS (S1-S22 + compliance)
M1 SCENARIO GATE: PASS (S23-S37 + S1-S22 + compliance)
[PASS] real-model-report
[PASS] portable-trace-manifest
[PASS] artifact-secret-scan
[PASS] final-acceptance-report
m1-final-artifacts: PASS
M1 FINAL ACCEPTANCE: PASS (artifacts + S1-S37 + compliance)
```

`cargo fmt --all -- --check`、`cargo check --workspace` 和 `cargo test --workspace --all-targets` 均由统一入口完成。协议 contract test 同时确认 86-event taxonomy、M1 additive payload compatibility 和 schema round-trip。补充质量门 `cargo clippy --workspace --all-targets -- -D warnings` 也已 PASS。

## 4. 残余风险

- 真实模型凭证和 endpoint 未进入 artifact；复跑需要重新提供有效的 Chat Completions-compatible 配置。
- 权威 SQLite store 保留在本地忽略目录 `.forme/`；仓库持久证据由 typed `ManualEvalReport`、portable trace manifest 和本报告组成。manifest 保存事件身份/顺序/类型而非完整 payload，避免把模型正文或未来敏感 payload 固化进仓库。

## 5. 结论

M1-A、M1-B、M1-C 三个交付波次、S1-S37、真实配置模型 golden trace/eval、Harness-first 架构不变量和原创合规门均已满足。**M1 最终验收 PASS。**
