# worker 终态回执来源核验（2026-11-06）

本记录承接 PRD24 §4.1 的结果封存/清理链与[历史审查协议 §9.8](../architecture/proposals/installed-legacy-release-recovery-review.md#98-worker-原件与当前静止水位)。契约细化提交为 `2e99841`，实现提交为 `eb99013`；本批不修改冻结 trait 签名、产品 DTO、摘要域、EventKind、crate 依赖或 unsafe 边界。

## 1. 问题与复现

原 `WorkerExecutorHost::record_receipt` 验证 AgentExecution 身份后，会依照调用者提交的回执封存一个结果并追加 terminal receipt，没有要求平台实际观察终态及完成资源交接。这是受认证库接口的事实来源漏洞；当前 Host HTTP 不暴露该入口，本记录不将它表述为已经存在的网络攻击路径。

新增反例先在旧实现运行并失败：真实 `WindowsJobWorkerIsolation` 启动原创 fixture，在实际 marker 出现后、进程仍睡眠时提交 Completed；旧实现接受请求，`a supplied receipt is not an OS terminal observation` 断言失败。本轮使用临时测试目录/Job，没有操作真实安装或原脑数据。

## 2. 修正

原接口只读确认 Host 已经通过平台观察、结果封存及清理链持久化的**完整同一回执**。先核对 AgentExecution 身份和 epoch，再在既有 operations 互斥中锁住原 journal writer，从原句柄有界重读并重放完整日志，核对真实水位与内存一致，校验原 job 后逐字段比较回执。

没有原回执、错误 schema/job/plan/epoch/nonce、不同结果/证据/时间等均拒绝。不隐式 probe、不生成结果工件、不追加事实、不终止或清理进程。实际新终态仍从原 probe/cancel/desktop 链取得。有效请求重复确认、并发确认及重启后确认均不重复平台操作或结果封存；原 Unknown 保留 Unknown，不能成为恢复静止证据。

重读继续持有原 OS 文件锁且恢复原文件位置。损坏、合法截断、额外记录、IO 或位置恢复失败使 writer 拒绝后续变更；有效但不匹配的请求不 poison 原有效 writer。该语义复用原件水位检查，不重写历史或为旧自报回执补造 OS 来源。

## 3. 定向验证

- `installed_worker_contract`：**21 passed / 0 failed**。含五个新增场景：不观察/封存/完成未观察 job；逐字段替换和错误认证拒绝；四种已保存结果的重启确认；清理失败或重启 Unknown 不被回执覆盖；并发确认零重复效果。
- `installed_windows_job_worker_contract`：**3 passed / 0 failed**。含上述实际活进程反例及既有真实完成/失败、后代取消场景。修正后自报被拒、journal 不追加、恢复 callback 不运行，原进程仍为 Running；真实取消后才可取得静止水位。
- `installed::worker::recovery::tests`：**11 passed / 0 failed**。新增场景通过原 writer 将日志截为合法在途前缀，确认旧内存回执被拒、writer 保持 poisoned 且原字节零修复。

本批七个新增命名测试已加入 `tools/verify-installed-host.ps1`，并将两个既有原生 Job 场景加入该永久门。集成夹具核对日志长度与结果工件字节、平台调用次数；原 journal 的完整字节由持锁句柄单元场景检查，不绕过 Windows OS 锁。

第一次修正后定向执行有五个测试在夹具快照读取 journal 时失败（Windows error 33），真实活进程反例已经通过。夹具改为仅从外部查询 journal 长度，完整字节检查留在原持锁句柄场景；再次执行上述 35 个测试全部通过。两次输出分别保留，不将夹具失败计为生产修复通过。

## 4. 完整检查与本地证据

完整源码质量门 `tools/verify-code-quality.ps1` 已结束，exit 0：182 个 Rust targets、**1269 passed / 0 failed / 15 默认 ignored**；fmt、locked workspace check/build/test、strict Clippy、结构门与完整合规均通过。前端 **115/115**、嵌入资源、签名/打包边界、真实临时 NTFS **8/8** 与只读防火墙夹具 **25/25** 通过；863 个源文件、63 处既有规模债务没有增长。未提供 Inno toolchain 的安装器 exit fixture 为 NOT_RUN。

永久宿主门 `tools/verify-installed-host.ps1` 也已结束，exit 0：本批七个新增场景和两个既有真实 Windows Job 场景均逐项通过，最终结果为 `installed-host-foundation and core-shutdown-components: PASS`。该门没有安装服务或认证原生安装包，明确输出 `IP01-IP24: NOT_RUN`，不登记整个产品或发布通过。

本批证据在本工作树 `target/worker-receipt-origin-20261106/`。`contract-compliance.log` 为文档提交前合规 PASS；`targeted-contract.log` 保留夹具 error 33；`targeted-contract-fixed.log`、`targeted-original-journal.log` 为最终定向结果。日志不包含真实登录凭据、模型密钥或原脑正文。

| 本批固定证据 | SHA-256 |
|---|---|
| `full-quality.log` | `e6662286d793b31d5aee29d1fb68bc0601a915c56a5b1ce82e7783e4b856ea4b` |
| `installed-host-gate.log` | `a3f9a929d9270f187c8753699659a830a117ece17b7f1aa1e5a0fc46898a28af` |
| `targeted-contract-fixed.log` | `dc5e56341952f5f3f4b6502644f1bc367053e35875410721a3898ff86637dd3f` |
| `targeted-original-journal.log` | `ac75edabada7a19f2fcc8feac814b277ba03a9f4ba26fc35d3cf21683714a3a5` |
| `contract-compliance.log` | `369aa4ca23bbb16cce58bb22be8be49e290675d13943609a496fa0cd0b6bdc92` |
| `build-resource-report.json` | `9a1fdc66cb6697bf82720966357098b6c5f987d556d95e798e554dd9523ae5e3` |

最终使用固定 target 执行 `py -3 tools/build_resources.py report --json`，结果 PASS，无超预算项，报告已保存为上表文件。本轮未清理缓存，未刷新 M5 或发布回执。完整质量与永久宿主门已经结束，不为本批文档收尾重复运行整仓 Rust 检查。

## 5. 原安装与整体目标

本轮只读 SCM 重查仍为 Host/Web/Brain/release-updater 全部 Stopped/PID 0；Host/updater ExitCode 1，Web/Brain ExitCode 0。当前 Codex 未提权，没有等待中的 UAC 进程。本批未修改 SCM、启停服务、替换安装程序、迁移数据或重派原 Upgrade，网页尚未恢复。

[原 Host 路径恢复计划](installed-original-host-path-restore-20261105-plan.md)和固定脚本仍存在，脚本 SHA-256 为 `93defb96100806aac062a3298cadee9d098144b563f9ae657e38a7fd2a9d43bf`；一次 Apply 的精确问题仍待答复。当前“继续”来自只读 UAC 问题，不改作 SCM 修改批准。

完整原生 collector、可独立接纳的历史来源、原 updater 到被审计源码的构建绑定、worker/lease 历史连续性、管理 reader 修复和原安装迁移仍按[八项材料状态](installed-legacy-recovery-evidence-status-20261106.md)独立闭合。当前 `with_quiescent_workers` 还没有独立核实实际隔离器进程/lease 资源；本修正不把日志终态升级为完整资源或历史证明。真实服务/模型/隔离 worker/helper、升级恢复和 IP01–IP24 不能由本批源码结果升级，整体目标仍未完成。

实际进程/lease 守卫已有[具体接口补充提案](../architecture/proposals/installed-worker-native-quiescence.md)，因新增平台 trait 方法，按 AGENTS.md 硬规则 3 单独待确认；本批未修改该 trait，也未将提案描述为已实施。
