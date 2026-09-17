# M2 最终验收报告

- 验收日期：2026-07-17
- 工程规模：18 crates；252 个非忽略 Rust tests；1 个真实浏览器 test 单独实跑；9 compliance fixtures
- 权威规格：`docs/requirements/07-m2-scope-decisions.md` §7；`docs/requirements/08-m2-verification-strategy.md` §6；`docs/prd/19-m2-program-prd.md` §10
- 最终结果：**PASS - M2 完成**

## 1. 完成条件

| 条件 | 证据 | 结果 |
|---|---|---|
| M0/M1 永久回归 | `tools/verify-m2.ps1` 先运行 M1 gate，再内嵌 M0 gate；S1-S37、workspace tests、M1 typed artifacts 与合规门全绿 | PASS |
| M2 场景门 | S38-S52 逐条精确测试；A/B/C 报告记录事件、store/projection、真实地面结果和反向断言 | PASS |
| 真实跨系统行动 | 真实 Chrome/Edge 访问 repository-owned loopback 页面，审批后 mutation 严格一次；真实 AppApi 与 communication loopback 路径再次通过 | PASS |
| 最严治理不退化 | external 默认 L3、L5 one-shot、执行前 policy/capability/digest 重查、CompetenceGate 出口、Untrusted provenance、unknown outcome no-retry 全部保留 | PASS |
| 事件与架构冻结 | 18 crates、既有内部依赖边、86 EventKinds；所有 M2 变化 additive/versioned；冻结 EventStore/Harness/ActionBackend 契约无旁路 | PASS |
| 凭据与工件 | credential value 只在 resolver 边界；typed eval/portable trace/sync transfer/验收报告通过字段和值扫描，SecretRef identifier 不出同步 transfer | PASS |
| 原创合规 | 9 个自建 fixture、真实树 upstream/license/copy 三门和依赖记录全部 PASS | PASS |
| 最终报告 | 本报告与 `m2-c-acceptance-report.md` 已生成，统一门在同一工作树通过 | PASS |

## 2. 三波结果

| 波次 | 范围 | 场景 | 结果 |
|---|---|---|---|
| M2-A | Browser/Computer/PTY 作为受治理 `ActionBackend`；L3/L5 floor、plan digest、Untrusted output、unknown outcome、SecretRef 与真实 browser golden | S38-S42 | PASS |
| M2-B | AppApi connector、真实 External CommunicationSession、Disclosure/representation、device retention/revocation、bounded A2A | S43-S47 | PASS |
| M2-C | ResourceGraph、长期 goal lineage、结果主导的能力候选、managed plugin policy、hot/cold memory、单 peer CAS sync | S48-S52 | PASS |

逐波证据分别见 `m2-a-acceptance-report.md`、`m2-b-acceptance-report.md` 和 `m2-c-acceptance-report.md`。A/B/C 均在 owner review 后才进入下一波；M2-C 未越入 M3 的自动规格提升。

## 3. 真实世界 Golden

M2 最终门重新运行 `s38_real_browser_golden_runs_harness_approval_and_observes_server_mutation`。真实事件骨架为：

```text
ToolCallProposed -> ToolPolicyEvaluated{ask} -> ApprovalRequested -> RunWaiting
-> ApprovalResolved -> RunResumed -> ToolPolicyEvaluated
-> CompetenceGateEvaluated -> ActionPlanned -> ActionStarted
-> ActionOutputDelta -> ActionCompleted -> CapabilityEvidenceRecorded
-> VerificationStarted -> VerificationFinished -> RunComplete
```

地面真值与离线证据：

- owner approval 前 loopback server mutation count=`0`，one-shot plan-bound approval 后严格为 `1`。
- `ActionOutputDelta`、Browser receipt 与 capability result provenance/trust 均为 `Untrusted`；页面注入正文没有产生 stable/candidate graph 写入。
- typed report `m2-a-browser-golden-report.json` 与 portable trace `m2-a-browser-golden-trace.json` 保存 35 个一一对应的 event refs、连续 `stream_seq`、plan/approval/receipt 摘要与 ground truth，不保存 URL、credential、raw DOM、transcript 或本机路径。
- artifact gate 在最终工作树重新解析两份 JSON 并通过 sensitive field/value scan；因此 real-world 证据可离线复核且无密钥。

## 4. 不变量复核

- **Harness-first**：Browser/Computer/PTY/AppApi/外部发言都只经 Harness -> policy/approval/competence -> execution；driver、connector、Gateway、plugin 和 peer 均无第二 loop 或 stable write API。
- **执行前重查**：capability schema/lifecycle、参数 scope、origin/surface/program/root、DisclosureBinding、plan digest 与 approval 在 `ActionStarted` 前重新校验。
- **能力门在出口**：对外动作和主动路径仍以结果证据为主；自评、trust、resource score、plugin 声明与一次成功都不能抬 permission。
- **候选先于稳定**：CapabilityUpdateProposal 只进 `CandidateCreated`；ResourceGraph/hot-cold 是 event-derived projection；外部 transcript/digest 不自动写 stable cognition。
- **事实源与并发**：store 仍是唯一 event writer；M2-C 增加 expected-version/CAS 后，冲突零 event、批次写入/投影/ledger/cursor 同事务。peer 只在 store open 时显式绑定，未配置实例和 TOFU 请求 fail closed，peer 不直写 projection。
- **防投毒**：网页/API/外部参与者内容贯穿 `UntrustedData` treatment；provenance 只由 Harness/Gateway 盖章，未信任 raw content 不改变 graph、trust、permission 或稳定模型。
- **可逆性真实**：rollback 只按 backend 声明；不可逆/high-impact 始终 L5 one-shot，补救不冒充 rollback，unknown outcome 不自动重试。
- **Schema/replay**：EventKind 保持 86；payload optional 字段 legacy-safe；SQLite schema 1 -> 2 迁移不改历史事件，S21 replay 与全部旧 contract tests 全绿。

## 5. 最终门

最终统一入口：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m2.ps1
```

该入口依次通过 S1-S37、S38-S52、真实 Chrome/Edge golden、18-crate/依赖边检查、M1/M2 typed artifact gate、strict Clippy 与 compliance。补充独立执行的 `fmt/check/clippy/test/Python/compliance/git diff --check` 同样全绿。

## 6. 残余边界

- M2 首版只支持一个 owner-bound sync peer，不承诺多主合并、多租户、公网 Gateway 或 remote executor；这些不是本里程碑缺口。
- Browser/Computer use 遵守 canonical §23 潮线：forme 只保留薄 driver trait、受治理接入和地面真值捕获，不投资浏览器/视觉自主智能。真实 golden 选 Browser；Computer 使用隔离 recording/native boundary tests，不控制 owner 当前桌面。
- managed plugin 不含 marketplace 或 remote auto-update；signature verifier 是可替换边界，生产 source-of-trust 配置仍需部署方提供。
- 能力成长不自动改 LoopSpec、CoordinationPolicy、TrustProfile 或 managed policy。M3 才处理 replay/eval/promotion/rollback 和规格自动提升。
- 开源发布前仍需完整 LICENSE/NOTICE 与依赖安全审计；M2 已完成的是持续原创合规门，不替代发布审计。

## 7. 结论

M2-A、M2-B、M2-C，S1-S52，真实跨系统 browser/API/communication 路径，typed eval/portable trace，18-crate/86-event 冻结契约，strict Clippy 与原创合规门均已满足。**forme 能跨真实系统行动，且没有削弱任何 M0/M1 治理不变量。M2 最终验收 PASS。**
