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

2026-10-27，干净 `shu/installed-product@da0d686` 的完整 `verify-m5.ps1` 运行到最终比较，S1–S99、workspace、严格 Clippy、89 项 Python 门（1 项 symlink 权限跳过）、M3/M4/M5 goldens、完整默认合规以及新 S99 审计生成/独立验证全部通过。新临时 receipt 为 `sha256:f6aada28c0657bb74462539f1bb95a1aece9cec5aacd52e085833d33725c748a`，仓库 receipt 仍为 `sha256:ac03fbb2490eb49dc47e50c3c1345f6ff5b50a8c0b23f26215a966f59d7f01d7`，最终 compare 按规则拒绝、脚本退出 `1`。**本次不计 FINAL PASS。** 完整输出在本轮工具执行记录中，不冒用早先 `final-m5.log`。后续当前树回执刷新与独立比较单独记录；没有为匹配旧回执弱化检查。真实模型 C24 和原生安装状态仍与本门分开。

2026-10-26，本轮完整 `tools/verify-m5.ps1` 已执行 `5391750` 所含相同运行代码的 S1–S99、V1 C1–C24、workspace、strict Clippy、89 项 Python 工具测试（1 项因无法创建 symlink 跳过）、格式/结构/合规及 M4/M5 工件检查。所有这些阶段通过；最终 S99 在干净 `51df714` 上生成并独立验证了新审计，但仓库 current-tree receipt 仍绑定旧源码，因此脚本以回执比较失败退出。**不将该次完整脚本记作 FINAL PASS。**

其后先将浏览器组件证据、未激活契约草案与 Windows 候选报告提交，再在干净 `3499ff9` 上单独重新生成当前树 M5 审计。8 个审计检查均 PASS，`blocking=false`，新 receipt 为 `sha256:37c05fe3a1db82d88aba781386329a9b73dbea673b370d133ee3a0809ccc8547`。生成工件和仓库工件的独立 `--verify` 均 PASS，`--compare` 也 PASS。该工件替换仓库 current-tree receipt；旧 `08ff70…` 回执保留在 Git 历史。此处是同一运行代码的完整场景回归与更新后独立发布树审计证据，不声称已重跑整套脚本并取得新的 FINAL PASS 终线。

浏览器证据限于预认领、core 离线的临时真实 Web daemon、320/390 窄屏、键盘和两标签页退出；新 Windows 包仍 unsigned/uninstalled、release_ready=false。实际 SCM、worker 结果通道、真实 provider、其他平台及完整 IP01–IP24 仍未由本轮通过。构建空间报告已保存在本地 ignored `target/build-resources-browser-5391750.json`：当前 target 为 97,774,282,024 logical bytes / 94,793,977,273 allocated bytes，超过 64 GiB 报告预算；构建前最近可用空间为 265.0 GiB。本轮未清理构建缓存或用户数据。

2026-10-26，在 `shu/installed-product@abad1e8` 上完成 Web 多标签页认证失效同步。完整 `verify-m5.ps1` 顺序跑过 S1–S99、C1–C24、Rust workspace、strict Clippy、Python、合规与 M4/M5 专项，并推进至最终 S99；当时源码尚未提交，发布树审计按规则拒绝 dirty tree。提交后对 clean commit 重新生成 M5 当前树回执，M5 release audit PASS，独立验证与仓库回执比较均 PASS；当前摘要为 `sha256:e41af6d2c50ded1d2db31782bb94db22d5d5b50258c5b421c5e05f8c07ae5d7c`。依赖审计中针对非 Windows target 的 advisory 为非阻断观察项。前端 `npm run verify` 为 81/81；代码质量门 PASS。旧回执 `sha256:0f0579703789d601d3d5480f808668ce52fa099cb85722bf92be820f519e99ec` 仍保存在 Git 历史。此结果不证明真实 SCM/LocalSystem 安装、原生平台/IP10/IP24 或 IP01–IP24 已通过。

2026-10-26，在 `shu/installed-product@93f501e` 上完整复跑 `tools/verify-m5.ps1`，终线为 `M5 FINAL ACCEPTANCE: PASS`。S1–S99、V1 C1–C24 回归、18 crates、97 EventKinds、严格 Clippy、workspace/Python tests、合规及 M5 当前树发布回执生成、独立验证和比较均通过；当前回执摘要为 `sha256:0f0579703789d601d3d5480f808668ce52fa099cb85722bf92be820f519e99ec`。工作树干净。该次默认全量测试中 3 个真实 Windows AppContainer 集成测试因需要显式原生环境而 ignored；其余另有按命令行/浏览器/多进程条件显式运行的 goldens 已由验收脚本执行。故此结果证明当前源码和受控回归门通过，不证明真实 SCM 安装或 IP10/IP24 平台验收通过。

2026-10-26，`shu/installed-product` 当前实施快照 `c76794c` 的 S99 M5 release audit 为 PASS，回执 `sha256:f788bcc3189d7644655418c3b47c1304ab2b51b5b4107c9bea8308057ea438c5` 已生成、独立验证，并与仓库 current-tree 回执比较 PASS。此次完整 `verify-m5.ps1` 在提交 Host 永久门更新前运行，S1–S99、C1–C24、workspace、strict Clippy、Python 89 tests、代码质量和合规均通过，最终只因工作树未提交在 S99 被 `RELEASE_TREE_DIRTY` 拒绝；提交 `68cca5a` 后单独重跑 S99 当前树审计通过。该回执不表示真实 SCM/LocalSystem 安装、交互桌面 helper、Linux/macOS 原生服务或 IP01–IP24 已验收。

2026-10-21，分支 `shu/installed-product` 的 Web 入口 `49674b7`、审计定位修复 `57e34b8` 与宿主拆分 `26ce4b4` 已通过 688 passed / 0 failed / 5 默认 ignored、strict Clippy、合规、质量和宿主组件门。Python 工具 72 项中 1 项按平台跳过，其余通过；当前 341 个源文件、65 处历史规模债务。

前置审计发现 Gateway 拆分后仍按旧 lib.rs 定位实现；已修正并新增反向测试。2026-10-21 在干净的 `afe7fdb` 上执行完整 `tools/verify-m5.ps1`，终线为 M5 FINAL ACCEPTANCE: PASS；S1–S99、显式 M3/M4/M5 goldens、workspace、Python、strict Clippy、合规及新回执 generate/verify/compare 全部通过。日志保留于本地 `target/verify-m5-product-ingress-final.log`，不包含个人配置读取或真实模型验收。

2026-10-24 在当前干净树（含 `de3c7b1` 的来源材料正文组件与 `7be4016` 的验收/口径更新）上重新生成、独立验证并与仓库工件比较，M5 release audit 为 PASS（9/9 blocking checks）；当前回执为 `sha256:bfc78be6b66eabc195916c177b2990157c1c8f5db614e5649c5da801bfe1ada4`，位于 `m5-release-audit-artifacts/`。旧回执 `sha256:6e2be4451db0822a732a5c0389d24818c723021a5b21875c178e89ccc3384ca3` 保留于 Git 历史；它仍可独立验证，但不再代表当前树。该永久回归不表示 IP01–IP24、原生安装、LAN TLS 或完整工作台已通过；后续产品增量仍需自己的组件与当前源码验收。

2026-10-26，在 `shu/installed-product@3c686ad` 上完成当前提交最终重验。`pwsh -NoProfile -File tools/verify-m5.ps1` 以退出码 0 结束，终线为 `M5 FINAL ACCEPTANCE: PASS`；S1–S99、V1 C1–C24、18-crate workspace 回归、格式/check/strict Clippy、89 项 Python 工具门（1 项因本机无法创建 symlink 而按规则跳过）、原创性/许可证/合规门、M4 历史回执以及 M5 当前树审计生成、独立验证和仓库 receipt 比较均通过。C13–C18 DAG/runtime 场景 23/23，C19–C24 产品闭合场景 7/7。当前 M5 receipt 为 `sha256:08ff70d06b39292042f98ebab4ab94da4bbc9858dc93a5c623654f7fda746c74`；审计观察到的 quick-xml 与 zbus/zvariant 项仅影响非本次 Windows release target，按审计规则记为非阻断观察项。

边界：全量 workspace 测试中 3 个真实 Windows AppContainer 用例仍按设计默认 `ignored`；本轮不将其计为通过。此 M5 工程总门也不证明真实 SCM/LocalSystem 安装、Linux/macOS 原生服务、安装包验收或 IP01–IP24 已通过；产品化状态继续以 [产品化实施进度](installed-product-implementation-progress.md)及各平台组件验收记录为准。

2026-10-26，在包含 Web 会话可见性修复 `a5b6a62` 与 Web 运行时验收文档提交 `c0593a2` 的干净源码上重新生成 M5 release audit。9/9 blocking checks PASS；生成工件独立 `--verify` PASS，替换仓库 current-tree receipt 后 `--compare` PASS。当前回执为 `sha256:e82f98856adf3c7e3a8a710473e78bee269d066b65149a1f3855e3b51d74e41d`。本次更新只刷新当前树审计绑定，不把受限浏览器夹具扩大为真实 Provider、系统服务、安装包、隔离 worker 或 IP01–IP24 通过；非 Windows target 的依赖 advisory 仍按既有规则记为非阻断观察项。

2026-10-26，在 `shu/installed-product@5726a5a` 上完成当前树最终回归。`tools/verify-m5.ps1` 退出码为 `0`，终线为 `M5 FINAL ACCEPTANCE: PASS`；S1–S99、V1 C1–C24、18-crate workspace、97 EventKinds、fmt/check、strict Clippy、89 项 Python 门（1 项因本机无法创建 symlink 按规则跳过）、M4/M5 goldens、原创/许可证合规及当前 M5 release receipt 的生成、独立验证与 current-tree 比较全部通过。当前仓库回执为 `sha256:81d40c41322af7e1ff455dd2fe2f62aa096541bb2a88afcc8055d7c6f18e18fd`。本轮同时包含已批准的 bounded worker result channel 组件验收（Process worker 4/4、typed transport 8/8、Windows Job worker 2/2）；结果通道专项报告明确其不等于正式 ProductionRuntime worker backend。

本轮最终门禁记录的 RustSec 观察项只影响非 `x86_64-pc-windows-gnu` target，按审计规则不构成当前目标的 blocking finding。最终构建资源报告显示 `C:\code\forme\target` 为 `116234168572` logical bytes、`113246893538` allocated bytes，超过 64 GiB 缓存预算；没有执行清理。以下边界仍未被本轮折算为通过：正式 ProductionRuntime 隔离 worker backend、结果工件应用层加密与真实 key resolver、Windows SCM/LocalSystem 真实安装运行、Linux/macOS 原生服务与安装包、正式签名、真实 Provider、Windows AppContainer 三项原生测试（默认 ignored）以及完整 IP01–IP24。Linux/macOS 继续保持 `release_ready=false`。

## 11. 产品化分支的 M5 current-tree 收口（2026-10-27）

在 clean `shu/installed-product@0190695` 上独立运行 `py -3 tools/m5_release_audit.py --root C:\code\forme --output <ignored-output>`，9 项检查全部 PASS，并通过独立 `--verify`。该次是中间预收口，随后已完成 core receipt 与最终 M5 receipt 的依赖顺序收敛；中间回执不替代最终 tracked M5 receipt。最终结果以 `docs/acceptance/m5-release-audit-artifacts/` 中唯一 receipt 及其 `--verify/--compare` 为准。

2026-10-27，在干净的 `shu/installed-product@14aef92` 上完整执行 `tools/verify-m5.ps1`，退出码 `0`，终线为 `M5 FINAL ACCEPTANCE: PASS`。S1–S99、18-crate graph、历史 97-kind strict prefix、workspace、fmt/check/strict Clippy、89 项 Python 门（1 项因本机 symlink 权限不足跳过）、M3/M4/M5 goldens、完整默认合规，以及当前 M5 release receipt 的生成、独立验证和仓库比较均通过。8 个发布审计检查通过，当前回执为 `sha256:fa4d2ae80e31fdf41d9677a4dff81c556e2b1fa522800d93cee65e5b0ab2bb8f`；日志 `target/installed-file-worker-evidence/final-m5.log`。上文旧快照不因本次结果而改写。

同一源码随后显式复跑 closure A、B、C 和 D runtime-only，命名场景实际执行数分别为 `47/47`、`50/50`、`36/36`、`71/71`，全部 PASS；18-crate/99-kind 当前 V1 契约检查通过。A/B/C 当前源码 runtime 回执及 D runtime-only 回执保存在同一 evidence 目录。D 使用 `-RuntimeOnly`，没有重新调用真实 Provider，故不声称新的 C24 live、D/Final 或 IP 产品验收通过。

本批统一质量门的唯一 workspace 总数为 176 targets、1088 passed / 0 failed / 15 默认 ignored，不能把后续重复运行相加。Host 命名门 211/211、配置临时真实进程 4/4，以及显式原生文件入口 7/7 的具体行为和证据限制见[文件 worker 与离线维护组件报告](installed-product-file-worker-and-offline-maintenance-20261027.md)。该报告 §4 中的当前源码 S/M5/适用 closure 重验义务由本节承接。唯一原生 symlink 用例仍未运行；桌面 helper、完整原生安装生命周期、正式签名、跨平台与 IP01–IP24 仍未闭合。

空间报告为 `target/installed-file-worker-evidence/final-build-resources.json`：当前 checkout target 为 `148201553979` logical bytes / `145200444254` allocated bytes，超过 64 GiB 报告预算；未执行清理。此后另获批准的本机安装尝试与任何恢复操作须独立记录，不从本次源码回归推导安装成功。

## 12. 当前主线 `1230900` 的 M5 回执重封（2026-10-28）

当前主线工作树保持干净，独立执行 `py -3 tools/m5_release_audit.py --root C:\\code\\forme --output <ignored-output>`，9 项发布审计检查全部通过；生成回执与仓库回执的 `--verify`、`--compare` 均通过。当前唯一 tracked receipt 为 `sha256:3eebe149a2d3cc99df8c232a4a01eabed80d992d830e7c0536d57512795a529a`，绑定当前 release tree `sha256:50b7c80680f74dd4a74365b339bf44f9da68e0cf91b4aabaac49be1763bf73ce`。

本次只修复当前树与旧 M5 receipt 的证据漂移，没有修改运行代码、模型配置、安装目录或用户数据。此前 Final 回归已完成核心测试和 M5 审计，但在最后的旧 receipt 比较处 fail-closed；因此本条只证明当前 M5 receipt 已闭合，不把此前整轮命令改写为新的 `M5 FINAL ACCEPTANCE: PASS`。核心 Final 仍需在该 receipt 收口后重新执行。

## 13. 历史审查组件后的当前源码重验（2026-11-05）

本节承接[缺基线历史升级的组件报告](installed-legacy-release-review-20261105.md)，只记录 `shu/bootstrap-claim-journal` 干净基线 `ab67e6c` 的源码回归和发布审计。实施代码包含 `8991875` 的独立 Host 审查；没有合入 main、推送或操作本机安装。

完整 `pwsh -NoProfile -File tools/verify-m5.ps1` 于本地时间 07:01–07:43 执行。M0–M2 最终门、M3 permanent regression、S1–S99、M4/M5 显式多进程 golden 与工件核对、18-crate 依赖图、历史 97-kind strict prefix、fmt/check/strict Clippy、完整 workspace/Python 测试和合规均通过。最后一次完整 workspace 的唯一汇总为 **180 targets、1168 passed / 0 failed / 15 默认 ignored**；Python 为 **89 tests、1 skipped**，不累计前置重复运行。命令只临时设置 `CARGO_BUILD_JOBS=2` 限制编译并发，没有减少测试或改变运行语义。

本轮 S99 的 8 项检查全部 PASS、`blocking=false`，生成回执为 `sha256:4cb70a891104726a19040c84f8a1323c4d88133b035b04b2b8544ec7ccc4806a`；生成与仓库回执各自的独立验证通过，但最终 `--compare` 返回 `1` 并触发脚本异常，**本次完整脚本没有取得 `M5 FINAL ACCEPTANCE: PASS`**。日志原样保留在本工作树 `target/legacy-release-review/final-m5-jobs2-20261105.log`。

差异随后精确核实为外部漏洞审计观察的变化：整轮生成结果比仓库回执少一条 `TARGET_UNREACHABLE_YANKED_PACKAGE`，涉及仅在非 `x86_64-pc-windows-gnu` 目标可达的 `async-executor 1.14.0`。源码 root、8 项 input digest 和其他观察不变；在内存中仅移除原回执这一条观察后，重新计算的 domain digest 恰为上述 `4cb70a…`，该诊断没有写成替代审计工件或更改规则。

在同一干净源码上另行完整执行 `py -3 tools/m5_release_audit.py --root . --output target/legacy-release-review/refreshed-m5-audit-20261105`，8 项检查再次全部通过；这次 cargo-audit 返回了该 yanked 观察，独立生成的回执恢复为仓库现有的 **`sha256:10e588f98be0d96220151e9d12f9f47cf01f75945c892dd59ebcb30c1303f5af`**，root 为 `sha256:97d1912cf9d734816bf307a38d4a82636dd9e76b4fb91b6ccaa355ac3e9706ec`。对新生成和仓库工件分别执行 `--verify`，再执行二者 `--compare`，全部 exit 0。无需替换 tracked receipt；这证明随后独立重验与当前回执一致，不将此前整轮失败改写为成功，也不宣称外部审计观察恒定。

同一实施源码的永久宿主门 **266/266**、closure A/B/C 及 D `-RuntimeOnly` 分别 **47/47、50/50、36/36、71/71**，详细命令与分层证据见组件报告。当前 99 EventKinds、五个既有 unsafe 边界保持，默认忽略的原生场景和 Python symlink 跳过不计为通过。D runtime-only 没有重新调用真实 Provider；实际 Inno 退出夹具未提供工具链，因此仍为 NOT_RUN。本轮不证明新的 C24 live、D/Final、原生历史收集器、正式历史审查 HTTP/Web、本机 reader/journal 迁移、Linux/macOS 或完整 IP01–IP24，整体交付目标仍开放。

构建资源报告保存为 `target/legacy-release-review/final-build-resources-20261105.json`：当前 checkout target 为 `33849706189` logical bytes / `31044564345` allocated bytes，低于 64 GiB 报告阈值，最终审计前可用空间为 25.3 GiB。本次收口未清理缓存、修改已安装程序/配置/现场 journal、启停服务或重跑原 Upgrade。
