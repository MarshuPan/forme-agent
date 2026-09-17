# M1 验证与验收策略

本文承接 `05-m1-scope-decisions.md`，把 M1 的真实使用体验增强转成可执行验收。S1–S22 继续证明 M0 内核没有回退；S23–S37 证明控制、评价、后台、主动、context 和 capability 体验真正闭合。

## 1. 验收原则

- 最终页面或回答不能单独证明正确，必须检查事件序列和权威 store/projection。
- UI/API 测试必须证明请求仍进入 Gateway/Harness，不得用 mock 绕过核心路径冒充集成验收。
- 读模型（viewer/search/eval export）必须证明不修改历史事件。
- 后台与主动场景必须覆盖重启、lease、cancel、duplicate intent、unknown outcome 和 foreground priority。
- capability 场景必须覆盖未选、disabled、untrusted、schema mismatch 和 execution-time recheck。
- 每个 M1 场景同时给出 Runtime、Governance、Learning/Projection 三类正确性证据。

## 2. 全局回归门

每次 M1 合并前必须执行：

```text
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace --all-targets
py -3 -m unittest discover -s tools/tests -v
bash tools/compliance-doctor.sh
```

`tools/verify-m0.ps1` 保持为 S1–S22 固定回归入口。M1 另建统一入口，但不能替代 M0 gate。

## 3. S23–S37 场景矩阵

| 场景 | 证明目标 | 主责波次 |
|---|---|---|
| S23 | CLI/Web 仍走同一 RunRequest 与事件流 | M1-A |
| S24 | event stream 断线重连按 stream_seq 无丢失/重复 | M1-A |
| S25 | approval/cancel/resume 从控制台完整往返且不绕过治理 | M1-A |
| S26 | trace viewer 可解释且严格只读 | M1-A |
| S27 | candidate/failure review 只触发治理迁移 | M1-A |
| S28 | manual eval/golden task 可重复且不自动提升策略 | M1-A |
| S29 | background intention 到期产生受治理 schedule run | M1-B |
| S30 | scheduler 重启/lease/重复 intent 不重复副作用 | M1-B |
| S31 | foreground priority 与 budget/cancel 能打断后台后续执行 | M1-B |
| S32 | 主动 follow-up 遵守三门、AttentionBudget 与 suppression | M1-B |
| S33 | local notification/approval delivery 仍经 action 治理 | M1-B |
| S34 | compaction 保留 approval/tool/decision/evidence lineage | M1-C |
| S35 | skill 按需加载 不泄露未选正文 | M1-C |
| S36 | MCP dynamic discovery/schema 延迟加载/tool search 仍受治理 | M1-C |
| S37 | plugin lifecycle 与 scoped memory review 不污染稳定层 | M1-C |

## 4. 逐场景规格

### S23 CLI/Web 同协议

- **Fixtures**：同一 harness/store；CLI 与 loopback Web surface；deterministic final model。
- **触发**：两入口提交语义相同的 `RunRequest`。
- **事件**：两者均为 `RunAccepted -> SessionBound -> GoalFramed -> ResourcePlanned -> DoneContractSet -> AutonomyEnvelopeSet -> DecisionTraceRecorded -> TurnStarted -> ContextBuildStarted -> ContextBuildFinished -> ModelCallStarted -> ModelCallFinished -> OutputClassified -> VerificationStarted -> VerificationFinished -> TurnComplete -> RunComplete`；provider 可在 started/finished 间追加 `ModelCallDelta`，但两入口必须同构。
- **断言**：除 run/event identity 与 surface provenance 外序列同构；Web 不存在 loop 直调 API。

### S24 Event stream 重连

- **Fixtures**：含至少 20 个事件的 run；客户端在 cursor N 断开。
- **触发**：用 `(run_id, after_stream_seq=N)` 重连两次。
- **结果**：只返回 `N+1..latest`，严格递增；重复重连响应确定。
- **反向断言**：重连不 append 新事件、不改 projection、不以 timestamp 排序。

### S25 控制操作往返

- **Fixtures**：ask action、pending approval、running cancellable action、suspended run。
- **触发**：控制台 resolve deny/grant 或 cancel；合法 approval resolve 以 one-shot grant 恢复 run，不暴露内部通用 `ResumeInput`。
- **事件**：approval 路径沿用 `ApprovalRequested -> RunWaiting -> ApprovalResolved -> RunResumed|ActionDenied`；cancel 产生 `ActionCancelled -> RunAborted`。
- **反向断言**：过期/错误 digest/错误 session 的控制请求失败且不产 `ActionStarted`。

### S26 Trace Viewer 只读解释

- **Fixtures**：含 coordination、tool、verification、failure、candidate 的 run。
- **触发**：读取 timeline、decision refs、failure digest 和 provenance。
- **结果**：viewer 顺序与 event log `stream_seq` 一致，DecisionTrace 引用可解析。
- **反向断言**：读取前后 event checksum、row count、projection snapshot 不变；不展示模型隐推理为审计事实。

### S27 Candidate/Failure Review

- **Fixtures**：working candidate、conflicting stable object、FailureEvidence。
- **触发**：owner confirm/promote、reject、downgrade 或 retract；confirm 复用 user-owned promote 语义。
- **事件**：只允许合法的 `CandidatePromoted{by=user}|CandidateRejected|CandidateDowngraded|RetractionEvent -> ReevaluationTaskCreated` 路径，不新增 `CandidateConfirmed` EventKind。
- **反向断言**：无 owner confirmation 不 promotion；UI 不能直接 UPDATE stable projection 或删除 failure。

### S28 Manual Eval

- **Fixtures**：四类 project-owned golden tasks；固定 model/profile/policy/tool/schema snapshot；human rubric。
- **触发**：运行 eval set 并记录 pass/fail/unverifiable 与 trace refs。
- **结果**：同 snapshot 可重复；报告能回到 run/event；verification failure 不算成功。
- **反向断言**：eval score 不自动修改 LoopSpec、CoordinationSpec、TrustProfile 或 capability policy。

### S29 到期 Background Intention

- **Fixtures**：due 与 not-due intention；有效/过期 envelope；scheduler clock。
- **触发**：daemon tick claim due intention。
- **事件**：`ProspectiveIntentionCreated -> RunAccepted{source=schedule} -> ... -> RunComplete|RunWaiting|RunAborted -> ProspectiveIntentionResolved`。
- **反向断言**：not-due/expired/unauthorized intention 不产生 run；schedule run 仍经 SessionBound/policy。

### S30 重启、Lease 与重复 Intent

- **Fixtures**：claimed lease 后 crash；已 `ActionStarted` 且 outcome unknown；重复 intent id。
- **触发**：重启 scheduler/harness 并扫描恢复。
- **结果**：安全的未开始任务可重新 claim；相同 intent coalesce；unknown outcome 进入 waiting/manual review。
- **反向断言**：副作用动作执行次数不超过一次；lease expiry 不等于允许盲重试。

### S31 Foreground Priority

- **Fixtures**：同 session 活跃前台 run、queued schedule run、有限 budget、cancel signal。
- **触发**：前台运行中 tick，再完成前台并释放；随后耗尽预算或取消后台。
- **结果**：后台 `TurnStarted` 不早于前台 terminal；budget/cancel 后不再开始新 action。
- **反向断言**：后台不写前台稳定态，不抢占 approval channel。

### S32 主动 Follow-up

- **Fixtures**：verification gap/failure observation；quiet hours；近期 reject；hitchhike delivery。
- **触发**：post-run/scheduled tick。
- **事件**：`ObservationRecorded -> OpportunityDetected -> ValueGateEvaluated -> ImpulseRaised -> CompetenceGateEvaluated -> DecisionTraceRecorded -> ProactiveProposalEmitted -> ProactiveProposalResolved`。
- **反向断言**：quiet hours/rate/suppression 触发时不打断；hitchhike 不消耗 interrupt quota；reject 不执行 action。

### S33 Local Notification/ApprovalChannel

- **Fixtures**：低风险提醒与高风险 delivery；local notification surface。
- **触发**：接受 proposal 后尝试 delivery。
- **事件**：`ToolCallProposed -> ToolPolicyEvaluated -> [ApprovalRequested -> ApprovalResolved] -> ActionPlanned -> ActionStarted -> ActionCompleted|ActionDenied`。
- **反向断言**：Notification backend 不可绕过 immutable plan digest；目标/scope 变化使原 approval 失效。

### S34 Context Compaction

- **Fixtures**：超预算 context，含 approval、tool result、DecisionTrace、FailureEvidence 和候选 lineage。
- **触发**：自动 compaction 后继续 run。
- **事件**：`CompactionStarted -> CompactionFinished -> ContextBuildFinished`。
- **断言**：preserved refs 完整，token 使用下降，继续执行仍能验证 done contract。
- **反向断言**：不能压掉未解决 approval、unknown outcome、failure 或来源边界。

### S35 Skill 按需加载

- **Fixtures**：多 skill metadata、一个 selected、一个 untrusted、一个 disabled。
- **触发**：search/rank 后只选择一个 skill。
- **事件**：`SkillMetadataExposed* -> SkillBodyLoaded(selected)`。
- **反向断言**：未选/untrusted/disabled skill 无 body event，正文不进 context。

### S36 MCP Dynamic Discovery

- **Fixtures**：运行中 schema 变化、schema 延迟加载、allowlist、disabled server、timeout/schema mismatch。
- **触发**：refresh discovery、tool search、选择后 resolve schema、执行。
- **事件**：`McpDiscovered -> CapabilityIndexed -> ToolsetResolved -> ToolCallProposed -> ToolPolicyEvaluated -> McpCallEvent -> ActionCompleted|FailureEvidenceRecorded`。
- **反向断言**：摘要命中不等于授权；执行前 schema/allowlist/policy 必须重查；disabled server 不可见。

### S37 Plugin Lifecycle 与 Scoped Review

- **Fixtures**：enabled plugin、reload 后贡献变化、crashing plugin；session/project memory candidates。
- **触发**：load/disable/reload；查询并 review scoped candidates。
- **事件**：`PluginContributionRegistered -> PluginToggled -> CapabilityIndexed/ToolsetResolved`；review 只走 candidate lifecycle。
- **反向断言**：disable/reload 后无 ghost capability；plugin failure 不终止 daemon；session candidate 不自动写 project/global stable。

## 5. Golden Task 最小集合

M1 至少维护四组项目自有任务：

1. final-only 问答与 trace。
2. 需要 tool + approval + verification 的工作区任务。
3. 需要 compaction + skill/MCP 按需加载 的长上下文任务。
4. 由 due commitment 或 verification gap 触发的后台/主动任务。

每个 task 固定 input、workspace fixture、done contract、allowed capabilities、policy profile、rubric 和禁止行为。报告必须同时给 task outcome 与事件证据。

## 6. 分波次门

- **M1-A gate**：S23–S28 + S1–S22 + compliance。
- **M1-B gate**：S29–S33 + M1-A 全回归。
- **M1-C gate**：S34–S37 + M1-A/B 全回归。
- **M1 final**：S1–S37、真实模型 golden tasks、合规门和 `docs/acceptance/m1-acceptance-report.md` 全部完成。

任何场景只验证页面文案、最终回答或 mock controller 而没有权威 event/store 证据，均不得计为通过。
