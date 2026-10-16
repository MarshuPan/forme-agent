# V1 核心大脑 B1 模块实施与验证报告

## 1. 状态与范围

**B1（持久 tick claim 与三相状态机）已在 `shu/v1-core-brain-a` 分支实现，模块自检 PASS；B 波整体仍在实施中，未合入 `main`。** 本报告对应 `prd/23` §4 B1 与 C7 的模块证据，不是 C7–C12 的整波验收，也不是 V1 最终闭合声明。

- 运行细节文档提交：`041c3a8`（PRD23 B1、architecture/09 §5.2）。
- 实现提交：`c6066d059ce23c96395fd83f09bf8a01e89e1d86`（`feat(harness): run the durable three-phase cognitive tick`）。
- B0 稳定认知晋升/重放前置项已先行实现；本模块继续验证其在线、重放与维护路径的一致性。
- 18 crates、内部依赖图、99 EventKinds 与前 97 exact prefix 均保持不变；未新增事件类型、内核或依赖边。

## 2. 实现结果

| 部分 | 行为与边界 | 主要实现 |
|---|---|---|
| 持久 tick ledger | owner/authority 围栏下原子 claim；严格按窗口顺序与 cadence 执行；接管沿用原输入范围；完成时同事务推进 consumed cursor | `store/src/brain.rs`、`store/src/sqlite.rs` |
| 三相状态机 | `ObserveAndPropose -> ReflectAndCandidate -> MaintainMemory` 固定顺序；每相返回状态、事件引用、模型调用数；报告绑定 claim/range 并计算 digest | `cognition/src/brain_tick.rs` |
| 默认运行时 | scheduler、Gateway 与显式 tick 使用同一 ProductionRuntime、store 和 ledger；冻结 `tick_once() -> Result<CognitiveTickReport>` 不变，未 claim 结果经 companion 显式返回 | `harness/src/production.rs`、`gateway/src/server.rs` |
| 提议与反思 | 失败/unknown 信号复用 value、competence、policy 与 emission guards；反思生成可重试的低置信候选；不执行外部动作、不自动晋升 | `harness/src/brain_tick.rs`、`harness/src/lib.rs` |
| 基础维护 | 从固定认知 head 重建快照，陈旧证据产生 failure/重评/降级候选；维护事实与候选具有 window/object-version 身份，部分写入后可补齐 | `cognition/src/lib.rs` |
| 并发与前台优先 | 重相执行前重新检查前台活动；让路时保留输入；认知命令串行化 append/fold，旧维护快照不覆盖后来已更新的稳定对象 | `harness/src/brain_tick.rs`、`cognition/src/lib.rs` |

窗口的判断时间与输入范围固定于首次 claim；重试不会把晚到证据混入旧窗口。memory/cognition aggregate head 仅作认知快照锚点，tick/aggregate 自身产出不作为下一轮失败观察源，避免自激反馈。

完成 claim 时重新核对已持久记录的 schema、owner/window/nonce、attempt、expiry 与原始 range，拒绝调用方篡改 range、旧 attempt 完成新 claim，以及越界推进 cursor。失败窗口保留 lease 待恢复；最多运行三次，第四次显式以 skipped 关闭该范围，原证据与部分输出保留，新窗口继续。skipped 不表示输入已成功处理。

`FORME_STABLE_EVIDENCE_MAX_AGE_MS` 默认 30 天，要求为正且可表示为时间戳差；它只决定重评/降级候选的时机，不提升 confidence/autonomy。完整 conflict、long-unseen 与 low-value branch 维护仍属 B5。

## 3. 模块证据

| 测试入口 | 数量 | 关键正反断言 |
|---|---:|---|
| `forme-store / v1_b1_tick_ledger` | 8 | 重复 signal/已完成窗口零重复履约；原子接管保留范围；旧 nonce/attempt/schema/expiry/range 无法完成；前窗口未完成禁止新窗口；重启持久、owner 围栏与 cursor 单调 |
| `forme-cognition / v1_b1_tick_rules` | 4 | 同一失败去重、unknown 优先、观察容量边界后仍进入反思；反思确定性且低置信；报告必须三相有序、defer 不消费未处理输入 |
| `forme-cognition / v1_b1_tick_recovery` | 4 | invalidation 已写但 candidate 未写时重启补齐且不重复事实；反思链部分写入后补齐并拒绝同 ID 改事实；旧快照不使新版本失效；受控并发维护/晋升顺序与重放相等 |
| `forme-harness / v1_core_brain_b1_tick` | 6 | 三相顺序/固定范围；重复 signal 与真实 SQLite 重启；前台让路后可继续消费；失败窗口重试与三次上限；晚到认知不进入旧快照；unknown 只形成受治理提议、零动作重试 |

专项门共 **22 项通过，0 失败、0 忽略**。Gateway E2E 另验证三相顺序与 digest、重复 tick 的显式未运行回执、零认知模型调用。

恢复测试使用 file-backed SQLite 或可控 append 中断；并发测试在维护事件追加前暂停，同时发起稳定认知晋升，验证它等待维护命令，最终事件顺序与 replay 一致。模型链路使用本地 HTTP 夹具；不把它计作真实 provider golden。

## 4. 最终代码验证

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo test --workspace --all-targets` | PASS：105 个 test targets 的结果，共 472 passed / 0 failed / 4 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `bash tools/compliance-doctor.sh` | upstream/license/copy 全部 PASS |
| `tools/verify-core-brain-b1.ps1` | PASS：workspace contract + 22 项专项；脚本明确不认证 B2–B6/最终闭合 |
| `tools/verify-core-brain-closure.ps1 -Wave A` | PASS：43 个按名场景完整复跑 |
| `git diff --check` | PASS |

复现模块门：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-core-brain-b1.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-core-brain-closure.ps1 -Wave A
```

本地完整输出在 `target/v1-b1-workspace-tests.log`、`target/v1-b1-module-gate.log` 与 `target/v1-b1-a-regression.log`；这些是未提交的本地诊断文件，持久证据入口为已提交的测试与门脚本。

## 5. 尚未完成与验收边界

- B2 AskToLearn、B3 owner AgentWorkspace、B4 激活扩散及模型基线、B5 完整维护、B6 C7–C12 整波门尚未完成；C/D 波未实施。
- 本轮未复跑 `tools/verify-m5.ps1` 与 4 个默认 ignored 的真实环境 golden，不宣称 S1–S99 全部发布门通过。B 波结束仍须执行这些永久回归及正式 closure gate。
- 本次工作区保留了到场时已有的许可证与发布审计相关变更（`Cargo.toml`、`LICENSE`、`NOTICE`、根 `README.md`、`tools/release_audit.py`、对应 Python 测试）；它们未纳入 B1 实现提交。当前不是 clean-tree release 验收，既有 M5 receipt 不作为当前树发布证据。
- A 波历史验收报告保留原结果与当时边界。本报告只说明 B0/B1 后续实现，不回写历史报告冒充当时已闭合。
- 全部认知质量、真实长期有用性、真实外部 provider 与 C1–C24 最终工件，仍按 `requirements/16` 与 D 波最终门验证。
