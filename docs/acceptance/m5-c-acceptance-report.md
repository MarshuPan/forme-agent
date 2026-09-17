# M5-C Federated Distribution 与 Release 验收报告

- 最终复核日期：2026-09-02（阶段验收记录仍保留原始日期）
- 验收范围：M5-C S95-S99，并永久回归 S1-S94
- 权威规格：`docs/requirements/14-m5-verification-strategy.md` S95-S99；`docs/prd/22-m5-program-prd.md` §5
- 结果：**PASS - M5 工程最终验收候选**

## 1. S95-S99 逐场景证据

| 场景 | 可执行证据 | 事件或状态序列断言 | 反向断言 | 结果 |
|---|---|---|---|---|
| S95 Authorized distribution | Harness `s95_distribution_rejects_non_executor_stale_and_revoked_inputs_before_network`；Gateway `s95_gateway_distribution_prepare_is_owner_only_and_has_no_execution_side_effect` | 有效分发先固定 package/release/digest、peer/grant/epoch 与 plan，再走 `ActionPlanned -> ApprovalRequested/Resolved -> CompetenceGateEvaluated -> RemoteExecutionLeaseChanged{Acquired} -> ActionStarted` | Replica、stale epoch、wrong grant、非 owner 和 package revoke 均在网络前失败；拒绝路径 driver calls=0，且无 `ActionStarted`/Q distribution event | PASS |
| S96 Unknown/no blind retry | Harness `s96_s97_distribution_unknown_recovers_after_authority_and_executor_restart_once` | 首次 dispatch 为 `Lease{Acquired} -> ActionStarted -> FailureEvidenceRecorded -> ActionOutcomeUnknown -> RunWaiting`；重启只读取 original receipt，随后 `ActionCompleted -> CapabilityPackageDistributionRecorded -> Lease{Released}` | authority/executor restart、重复 recovery 和 duplicate receipt 均不产生第二次 dispatch/install；driver=1、executor install=1、distribution event=1 | PASS |
| S97 Restart/replay/legacy | 同一 restart test；Harness `s91_s97_restart_reverifies_archive_signature_publisher_dependencies_and_policy`；store `legacy_m4_database_opens_with_an_empty_ecosystem_projection` | authority SQLite、executor package ledger 和 dispatch ledger 重启后恢复同一 package/receipt/CAS 事实；legacy M4 store 以 ecosystem version zero/empty projection 打开 | unknown schema、archive/signature/key/publisher/dependency/policy drift、nonce/receipt 异语义重放和 stale CAS 均 fail closed，不从 catalog 猜测权威状态 | PASS |
| S98 Three-role real golden | ignored exact `s98_three_process_registry_authority_executor_golden_is_governed_end_to_end`；独立 `forme-m5-registryd`、authority Harness/store、`forme-m5-executord`；`forme-m5-artifact-verify` | 真实链路完成 publisher -> approved AppApi fetch -> admission -> install/enable -> governed registry use -> remote unknown/recovery -> verified distribution -> revoke；权威 trace 为 25 个连续 `stream_seq=1..25` 事件 | registry fetch=1、authority `ActionStarted`=1、executor install=1、distribution event=1、unknown retry=0；revoke 后 visible contribution=0、new distribution=0、restricted material=0、非 authority append=0 | PASS |
| S99 Supply-chain release gate | eval 四个 artifact contracts；29 个 Python tests；`tools/m5_release_audit.py` generate/verify/compare；S22 doctor | closed five-file artifact set content-addressed 且两次独立真实实跑 receipt 相同；release audit 固定 9 项 typed checks，全部 `outcome=pass, blocking=false` | tamper、unknown/extra/missing file、cross-ref/order drift、secret/private path/endpoint、NOTICE/pin/borrowing/copy/advisory/dirty-tree 任一问题均非零阻断 | PASS |

## 2. S98 Repository Golden

闭合工件目录：`docs/acceptance/m5-artifacts/`

| 工件 | 内容寻址 digest | 关键事实 |
|---|---|---|
| Publisher | `sha256:975dd6c66021343d1c7bce4d419d69516d2b568eb42d734f364b495afcd6d22f` | owner-provisioned grant、public-key digest、scope/version 与 active observation |
| Admission | `sha256:027ab061ba2d9ce7af3edbb9ce2617a8e229e1db1b4ff2c4db7cd91374e9d829` | package/release/policy、13 个 hard checks、SBOM/license；运行时 evidence label 只保留稳定语义 digest |
| Install | `sha256:7ea8c5dd2c586bd98c0d08be7fe9cdb2d01a94cc06573e21135931b08efc10bf` | immutable plan、owner approval、Enabled state、ecosystem version 与 registry digest |
| Distribution | `sha256:1c664dcf415206932dedd97dcc2d8318115eb312c1381913403a6b3ca9686d5a` | package/peer/epoch/plan/lease/fence、executor ledger semantic digest 与 authority verification |
| Trace | `sha256:a693098714ca1491a1a9b77ca163bd94e7562ca4939b3a2be9b616602cb219f6` | 25-event lineage、ground-truth counters、revoke/unknown/no-retry 禁止断言 |

五件套 receipt：`sha256:237570b140753cc57478a5f681c2487bf1448bab287c0cd562abf20b776bbcbc`。两次独立 registry/authority/executor 实跑生成完全相同的 receipt；TTL、临时 transport identity、外层 receipt id 和原始 admission evidence label 均未进入 portable projection。

## 3. S99 Clean-tree Release Audit

- typed receipt：`sha256:eae2549a7b2a4f2b98d02988ddd052831ee264a4fb18addb5af45c07035ec497`
- audit profile：`forme-release-audit/5`
- checks：9/9 Pass，`blocking=false`
- 覆盖：clean tracked tree、LICENSE/NOTICE、exact direct pins/offline metadata、borrowing records、upstream copy、acceptance artifacts、RustSec target reachability、M5 ecosystem threat fixtures

本次复核将 `chacha20 0.10.1` 与 `event-listener 5.4.1` 分别更新至 `0.10.2` 与 `5.4.2`，消除了当前报告的 yanked/unsound warning。`cargo-audit 0.22.2` 仍保留 `quick-xml 0.30.0/0.39.4` 的 `RUSTSEC-2026-0194/0195` 四项 observation；反向依赖检查证明它们在冻结 release target `x86_64-pc-windows-gnu` 不可达。release target 或 feature 改变会重新计算，任何当前 target 可达 advisory 仍为 blocking finding。

## 4. 工程门

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets`、strict Clippy：PASS。
- workspace 列出 383 个 Rust tests：379 个非忽略测试由完整 workspace suite 执行，4 个真实 Browser/federation/ecosystem golden 由统一门以 `--ignored --exact` 显式执行。
- `py -3 -m unittest discover -s tools/tests -v`：29/29 PASS。
- `tools/verify-m5-workspace-contract.ps1`：18 crates、无新增内部 edge、exact pins、97-kind exact prefix 与 process/verifier targets 全部 PASS。
- `bash tools/compliance-doctor.sh`、`git diff --check`：PASS。

## 5. 残余边界

- loopback HTTP/mutual TLS 和独立进程证明真实 transport/process/side-effect/recovery，不宣称公网部署、证书运营、HA 或开放 marketplace。
- M5 package 只含声明式 UTF-8 资源和既有 registry contribution；不执行 native/WASM/container/install script，也不动态加载 package 代码。
- Distribution receipt 只证明 exact public package bytes 在指定 executor ledger 的受验证事实，不产生远端 enable、permission 或自治。
- unknown outcome 仍需 original receipt/read-only probe/owner disposition；不承诺通用 distributed exactly-once。
- release audit 是工程合规证据，不替代法律意见、公开发布审批或外部安全审计。

## 6. 结论

M5-C 把受治理 package lifecycle 接到 M4 peer/epoch/lease/fence/unknown recovery 上，并用真实 registry/authority/executor 与 closed typed artifacts 证明分发没有产生第二 authority、peer self-install、blind retry 或敏感信息泄漏。**M5-C 自验收 PASS，进入 M5 最终封板门。**
