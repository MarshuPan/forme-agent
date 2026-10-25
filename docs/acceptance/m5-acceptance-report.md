# M5 受治理能力生态最终验收报告

- 最终复核日期：2026-09-02（阶段验收记录仍保留原始日期）
- 主线基线：M4 封板
- 范围：M5-A/B/C、S85-S99；S1-S84 为永久回归门
- 结论：**M5 工程最终验收 PASS；尚未合并 `main`**

## 1. 完成范围

M5 保持一个 configured owner、一个 authority Harness、一个 authoritative EventStore 和既有 18-crate 图，把能力来源扩展为受治理生态：

- M5-A：owner-provisioned publisher grant/keyring、content-addressed signed declarative package、catalog quarantine、固定 hard-check admission 和真实 registry intake。
- M5-B：immutable plan + one-shot owner approval + ecosystem CAS、现有 registry 原子切换、update/rollback/revoke，以及 hook/AgentProfile 声明式边界。
- M5-C：M4-bound executor distribution、durable package/dispatch ledger、unknown no-blind-retry recovery、真实 registry/authority/executor golden、五件套与 clean-tree release audit。

## 2. S85-S99 逐场景证据

| 场景 | 核心可执行证据 | 权威事实与反向断言 | 结果 |
|---|---|---|---|
| S85 | Harness owner publisher control + store ecosystem CAS | `RunAccepted -> SessionBound -> CapabilityPublisherChanged -> RunComplete`；非 owner/TOFU/stale CAS/expired grant 零写 | PASS |
| S86 | real catalog quarantine tests | external bytes/provenance 保持 Untrusted；注入文本不能生成 owner/trust/permission/admission/state | PASS |
| S87 | real Ed25519/digest tests | key possession、grant、package/release/body digest 精确绑定；tamper/wrong key/rebind 零 admission | PASS |
| S88 | 13-kind admission matrix | schema/size/closed set/path/digest/publisher/signature/dependency/SBOM/license/secret/risk/policy 全 Pass 才 admission；self score 不覆盖 hard failure | PASS |
| S89 | approved loopback registry golden | approved AppApi fetch exactly once -> verification -> admission；redirect/extra/tamper/unapproved refetch 均阻断 | PASS |
| S90 | Harness install/enable plan contracts | admission、install、enable 分离；owner/plan/nonce/expiry/policy/CAS 任一漂移零 registry change | PASS |
| S91 | staging/registry/store restart contracts | complete source staging 后才原子切 generation；失败/缺 archive 保留 previous known-good，无 partial visibility | PASS |
| S92 | update/rollback history contracts | exact immutable v1/v2 与新 rollback approval 全保留；无 floating latest、permission expansion 或外部效果回滚宣称 | PASS |
| S93 | package/publisher revoke contracts | revoke 追加历史并移除后续可见 contribution；old signature/cache/approval 不能复活或再分发 | PASS |
| S94 | hook/AgentProfile contracts | hook 仍是 proposal 并 final policy recheck；profile 无 memory/cognition/policy owner，只能由 Harness spawn child | PASS |
| S95 | federated authorization/Gateway contracts | only active Executor + exact peer/grant/epoch/plan；Replica/stale/revoked/non-owner 在网络前零 driver | PASS |
| S96 | durable unknown/recovery contract | `ActionStarted -> ActionOutcomeUnknown -> RunWaiting -> original receipt -> DistributionRecorded`；dispatch/install 严格一次 | PASS |
| S97 | authority/executor restart + legacy open | CAS/projection/receipt/ledger 重启一致；legacy ecosystem empty，schema/digest/policy drift fail closed | PASS |
| S98 | independent registry/authority/executor golden | fetch=1、driver=1、install=1、distribution=1、retry=0；revoke 后 use/distribution=0、restricted material=0 | PASS |
| S99 | artifact negatives + 29 Python gates + release audit | closed set、content address、cross-ref、secret/path、NOTICE/pin/copy/RustSec/dirty tree fail closed；9/9 checks Pass | PASS |

逐场景的测试名、事件子序列和负向断言见 `m5-a-acceptance-report.md`、`m5-b-acceptance-report.md` 与 `m5-c-acceptance-report.md`。

## 3. M0-M4 永久回归

`tools/verify-m5.ps1` 首先运行 M3 permanent regression（S1-S69 与 S38/S68 真实 Browser goldens），随后复跑 M4 S70-S84 contracts、三进程 federation golden 与历史 M4 artifacts/release receipt，再运行 M5 S85-S99。M5 没有用 catalog、signature、publisher、package 或 peer 绕过已有 Harness、Policy、Approval、CompetenceGate、DisclosurePolicy、candidate/active/permission 分离、single authority 或 unknown no-blind-retry。

workspace 共列出 383 个 Rust tests，其中 379 个非忽略测试由 `cargo test --workspace --all-targets` 执行；4 个真实 Browser/M4 federation/M5 ecosystem tests 由统一门显式执行。因此最终门覆盖全部 383 个列出测试，而不是把 ignored golden 当作默认已运行。Python 工程门当前为 29/29 PASS。

## 4. Frozen Architecture

- 18 crates；内部 dependency edge 不新增、不反向；冻结 `EventStore`、`VersionedEventStore`、`EvolutionEventStore`、`FederationEventStore`、`ActionBackend` 和 `AgentHarness` signatures 未改变。
- 97 EventKinds；前 93 项是 M4 exact prefix，只在末尾追加 `CapabilityPublisherChanged`、`CapabilityPackageAdmitted`、`CapabilityPackageStateChanged`、`CapabilityPackageDistributionRecorded`。
- publisher grant、signature、admission、lifecycle、registry visibility、per-run permission 与 distribution receipt 是独立事实，任何一个都不能替代另一个。
- package 只通过现有 Capability/Plugin/Skill/MCP/Connector/AgentProfile registry 暴露；enabled contribution 的真实使用仍走 per-run toolset、policy、approval、CompetenceGate 和 final recheck。
- catalog/package/README/skill/profile/remote receipt 始终是 UntrustedData；provenance 只由 Gateway/Harness/Verifier 按真实入口盖章。
- secret/private key/SecretRef id/private endpoint/owner path 不进入 package、event、trace、portable artifact 或 release receipt。
- federated distribution 复用 M4 active Executor grant、authority epoch、plan/lease/fence 和 unknown recovery；peer 不是 writer、publisher trust owner 或第二 Agent。

## 5. Typed Artifacts

S98 repository closed set 位于 `docs/acceptance/m5-artifacts/`：

| 证据 | 值 |
|---|---|
| 五件套 content receipt | `sha256:237570b140753cc57478a5f681c2487bf1448bab287c0cd562abf20b776bbcbc` |
| registry fetches | `1` |
| authority driver calls | `1` |
| executor installs | `1` |
| verified distribution events | `1` |
| unknown retries | `0` |
| post-revoke visible/distribution | `0 / 0` |
| restricted material matches | `0` |
| authority trace | 25 个连续 `stream_seq=1..25` typed events |

`forme-m5-artifact-verify` 检查 schema、closed five-file set、content address、package/release/grant/policy/approval/plan/peer/epoch/lease/fence/ground-truth/trace cross-ref、event order、97-kind taxonomy 和 sensitive markers。Repository set 与第二次独立真实实跑 set 的 receipt 完全一致。

## 6. Release Audit

| 项 | 值 |
|---|---|
| typed receipt | `sha256:5c4875a436169b93f645256897aec5b1b7d993dc2a268b88a6ae331c16170d95` |
| audit profile | `forme-release-audit/5` |
| checks | `9/9 pass, blocking=false` |
| target | `x86_64-pc-windows-gnu` |

S99 针对 clean tracked tree 执行 release tree、LICENSE/NOTICE、direct exact pins/offline metadata、borrowing records、upstream copy、acceptance artifact secret/path scan、RustSec reachability 和 M5 ecosystem threat profile。`ed25519-dalek 2.2.0` 已 exact pin、登记 third-party/borrowing record 并保留 BSD-3-Clause NOTICE。

本次复核已将可修复的 `chacha20 0.10.1` yanked warning 与 `event-listener 5.4.1` unsound warning 分别更新至 `0.10.2` 与 `5.4.2`。RustSec 对 locked `quick-xml 0.30.0/0.39.4` 的四项 advisory 仍保留为 target-unreachable observations，不被忽略。任何 release target/feature 变化或当前 target 可达 advisory 都会重新变成 blocking finding。

## 7. Reproducible Final Gate

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m5.ps1
```

该命令覆盖 S1-S99、97-kind exact prefix、18-crate graph、383 个 Rust tests、M3/M4/M5 四个显式真实 goldens、29 个 Python gates、temporary/repository M4/M5 artifacts、fmt/check/strict Clippy、原创合规、historical M4 receipt 和 M5 current-tree release receipt generate/verify/compare。

成功终线：

```text
M5 FINAL ACCEPTANCE: PASS (S1-S99 + 97 EventKinds + 18 crates + governed ecosystem golden + release compliance)
```

## 8. 残余边界

- 这是单 owner、单 authority、有限 publisher/catalog/executor 的声明式生态，不是多租户 marketplace、支付/声誉系统、多主 package state 或 peer-to-peer 分发。
- 没有 native/WASM/container/install script/build-from-source；MCP executable、connector credential 和 Agent cognition 仍来自受管理的本地配置与唯一 Harness。
- loopback registry/mutual TLS 证明真实系统边界，不宣称公网 registry、production PKI、HA 或 supply-chain 法律认证。
- rollback/revoke 只改变后续 active visibility/distribution，不宣称删除远端历史 bytes 或撤销既有外部副作用。
- release audit 是工程证据，不执行 merge、push、公开发布或法律签核。

## 9. 结论

M5-A/B/C 已拼成完整受治理能力生态闭环：owner publisher grant -> untrusted catalog quarantine -> signed content-addressed admission -> plan/approval/CAS lifecycle -> existing registry -> governed capability use -> M4-bound remote distribution -> unknown/no-retry recovery -> authority ground-truth verification -> revoke -> typed artifacts/release audit。S1-S99、97 EventKinds、18 crates、全部列出测试、29 个 Python gates、真实三角色 golden、strict Clippy 与 clean-tree compliance 均满足，且 M0-M4 治理不变量没有削弱。

**M5 工程最终验收 PASS，达到 owner review/合并候选标准。**

## 10. 产品化分支的永久回归重验

本节只追加当前产品增量的回归记录，不改写上文 M5 历史验收，也不宣称 IP01–IP24 已通过。产品实施状态仍以 `installed-product-implementation-progress.md` 为准。

2026-10-26，在 `shu/installed-product@93f501e` 上完整复跑 `tools/verify-m5.ps1`，终线为 `M5 FINAL ACCEPTANCE: PASS`。S1–S99、V1 C1–C24 回归、18 crates、97 EventKinds、严格 Clippy、workspace/Python tests、合规及 M5 当前树发布回执生成、独立验证和比较均通过；当前回执摘要为 `sha256:0f0579703789d601d3d5480f808668ce52fa099cb85722bf92be820f519e99ec`。工作树干净。该次默认全量测试中 3 个真实 Windows AppContainer 集成测试因需要显式原生环境而 ignored；其余另有按命令行/浏览器/多进程条件显式运行的 goldens 已由验收脚本执行。故此结果证明当前源码和受控回归门通过，不证明真实 SCM 安装或 IP10/IP24 平台验收通过。

2026-10-26，`shu/installed-product` 当前实施快照 `c76794c` 的 S99 M5 release audit 为 PASS，回执 `sha256:f788bcc3189d7644655418c3b47c1304ab2b51b5b4107c9bea8308057ea438c5` 已生成、独立验证，并与仓库 current-tree 回执比较 PASS。此次完整 `verify-m5.ps1` 在提交 Host 永久门更新前运行，S1–S99、C1–C24、workspace、strict Clippy、Python 89 tests、代码质量和合规均通过，最终只因工作树未提交在 S99 被 `RELEASE_TREE_DIRTY` 拒绝；提交 `68cca5a` 后单独重跑 S99 当前树审计通过。该回执不表示真实 SCM/LocalSystem 安装、交互桌面 helper、Linux/macOS 原生服务或 IP01–IP24 已验收。

2026-10-21，分支 `shu/installed-product` 的 Web 入口 `49674b7`、审计定位修复 `57e34b8` 与宿主拆分 `26ce4b4` 已通过 688 passed / 0 failed / 5 默认 ignored、strict Clippy、合规、质量和宿主组件门。Python 工具 72 项中 1 项按平台跳过，其余通过；当前 341 个源文件、65 处历史规模债务。

前置审计发现 Gateway 拆分后仍按旧 lib.rs 定位实现；已修正并新增反向测试。2026-10-21 在干净的 `afe7fdb` 上执行完整 `tools/verify-m5.ps1`，终线为 M5 FINAL ACCEPTANCE: PASS；S1–S99、显式 M3/M4/M5 goldens、workspace、Python、strict Clippy、合规及新回执 generate/verify/compare 全部通过。日志保留于本地 `target/verify-m5-product-ingress-final.log`，不包含个人配置读取或真实模型验收。

2026-10-24 在当前干净树（含 `de3c7b1` 的来源材料正文组件与 `7be4016` 的验收/口径更新）上重新生成、独立验证并与仓库工件比较，M5 release audit 为 PASS（9/9 blocking checks）；当前回执为 `sha256:bfc78be6b66eabc195916c177b2990157c1c8f5db614e5649c5da801bfe1ada4`，位于 `m5-release-audit-artifacts/`。旧回执 `sha256:6e2be4451db0822a732a5c0389d24818c723021a5b21875c178e89ccc3384ca3` 保留于 Git 历史；它仍可独立验证，但不再代表当前树。该永久回归不表示 IP01–IP24、原生安装、LAN TLS 或完整工作台已通过；后续产品增量仍需自己的组件与当前源码验收。
