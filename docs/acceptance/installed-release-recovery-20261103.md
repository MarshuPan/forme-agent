# 升级中断裁决：实现与隔离验收（2026-11-03）

本轮范围为 owner 明确批准的 `AbortBeforeSwitch` 契约补充、实现和隔离测试，依据[批准草案](../architecture/proposals/installed-release-interruption-adjudication.md)、canonical §29.11、architecture/10 §9.1 与 PRD24 §4.10.5–6。它不授权本机程序替换、旧 journal 迁移或重跑原升级。整体目标仍 active，IP14–15/IP24 不因本报告关闭。

## 1. 实现

- 独立的恢复 candidate/request/view/verification/list、closed 版本校验及 owner、原 operation/command、release/config、证据摘要、CAS、期限绑定。
- 升级前 Brain 已停止且 worker 静止时采集源基线；恢复期间按配置锁 → worker operations 锁 → 产品 journal 串行核实，原数据只 Inspect/Release，持独占检查直到裁决持久化。
- 独立 intent/commit；一个 commit fact 原子形成原升级 `Aborted` 与新恢复决定 `Committed`。保留原历史、failure、claim/handoff；不改变 active release、脑事件或工作准入。
- 同请求重放只查；同 key 异义拒绝；intent 后中断只能显式重新认证 verify 原号。未决恢复占互斥，不能换号绕过。
- 新 journal envelope 使用 v2，保留历史 v1 原字节/hash，一旦进入 v2 禁止回退。固定 updater 的真实离线能力探测证明 reader/updater 均为 2，不以业务 Host 自报代替。
- 正式公开 Web 认证、Origin/CSRF、操作绑定密码重认证及内部 Configuration 域路由；网页只读找回原 intent，明确区分“升级已终止”与“升级成功”。

脑 EventKind、18-crate 依赖图、Agent action Unknown、审批和模型路由不变。没有新增 unsafe 边界。协议文档提交为 `f3f2e74`、`af0a73e`，实现提交为 `fbca8ab`；本报告只登记本轮证据，不把产品化未完成项折算为通过。

## 2. 验证层次与执行结果

| 层次 | 证据和边界 | 结果 |
|---|---|---|
| 产品 journal | 六项正常/拒绝、并发 CAS、跨域 key、原始 hash、版本降级和 intent 恢复测试。 | targeted 6 PASS；原历史、failure、claim/handoff 保留断言通过。 |
| Host 事务 | 真实临时文件/配置、持久 journal；SCM 与数据身份为受控 adapter。验证零服务/备份/切换、漂移拒绝、原号查询。 | targeted 3 PASS；恢复提交只发生一次且 active release 不变。 |
| Worker | 真实 worker journal 与 operations 锁；Running/restart Unknown 拒绝，临界区阻止新准入。 | targeted 2 PASS。 |
| 固定能力探测 | 实际 `forme-hostd --release-recovery-capabilities` 子进程；未知版本/字段拒绝，不读配置/凭据/journal。 | targeted 1 PASS；固定 reader/updater 能力均验证为 v2。 |
| 数据检查子进程 | 默认 System inspector → 固定子进程 → Inspect/Release；子进程为原创 transport fixture，不是真实用户数据库。 | recovery data transport 场景 PASS；不外推为真实用户数据库迁移证据。 |
| 正式 Web HTTP | 实际身份库、challenge/login、Origin/CSRF/单次密码重认证、公开 HTTP → 受控内部 HTTP；内部 Host 回执为受控。 | targeted 3 PASS；不等于本机 SCM 恢复。 |
| 前端 | closed parser、Aborted 终态、断线只查、刷新找回原 intent 与显式 verify；TypeScript 与资源生成。 | 100/100 PASS；本轮未做真实安装浏览器验收。 |
| 全量源码门 | `tools/verify-code-quality.ps1`：fmt/check/test/strict Clippy、前端、包边界/原生夹具、合规。 | PASS：178 个 Rust test suites，1132 passed / 0 failed / 15 ignored；前端 100/100；npm audit 0 vulnerabilities；strict Clippy、结构/包边界、合规均通过。 |
| 永久宿主门 | `tools/verify-installed-host.ps1` 新增全部恢复精确命名场景，每项必须恰好运行一次。 | PASS：238/238 named scenarios，0 failures；新增 recovery 场景通过。 |
| 配置/核心回归 | 正式配置进程、S1–S99/M5 与相应 closure 门。 | PASS：配置/多进程真实组件门 4/4；Rust 全量回归包含现有 S1–S99/M5/closure 场景。 |
| 本机原生恢复 | 真实安装的 reader/updater 兼容迁移、旧 operation 裁决、后续新升级/启动。 | NOT_RUN；未获本轮授权。 |

日志目录为 ignored 的 `target/installed-status-authorization-evidence/`；不得提交原测试账号、密码、cookie、API key 或完整配置。证据摘要如下：

| 证据 | SHA-256 |
|---|---|
| `release-recovery-quality-final-20261103.log` | `3fdf6da0da3c4e685f67bb1ca8d51976e1a399ed968a68871be633663899136d` |
| `release-recovery-host-20261103.log` | `3af031f5d40185684ce9dfe661881053b65110cca8009517f6aa75b97ff98875` |
| `release-recovery-config-20261103.log` | `20becccdf294b16ac6dad95e1f41a999bcaf0c2f0cf4a6e109460300840d1644` |
| `build-resources-20261103.json` | `352ea32ccca58d8464becef0eade5e313d2854b20f4eb3f6d0b070f6e2de3a1a` |

本轮源码/测试提交为 `fbca8ab`。构建资源检查命令可执行并返回 PASS，但报告同时记录 `C:\code\forme\target` 超出缓存预算；当时约有 58 GiB 可用空间。本轮没有未经批准的缓存清理，证据文件留在 ignored 目录，不进入 Git。

## 3. 本机只读复核与历史边界

本轮 SCM 只读复核：Host/Web Running，PID 分别为 `37740`、`75344`；Brain/updater Stopped，updater exit code 1。没有启停、修改注册、写配置或替换程序。

最近正式维护观察仍是原升级 `maintenance:acceptance-upgrade-after-path-20261102-db96ec2`，product revision 28、`Validated / pending Quiescing`；原 release 为 `forme-windows-x86_64-20261027-7cb5b8b`。这是此前正式回执，不以本轮 SCM 观察冒充重新读取了受保护 journal。原升级的根因仍不能从 Win32 通用退出码推定。

该旧操作没有新协议要求的升级前 journal 基线，因此已实现的正常恢复入口会拒绝它。故障后摘要不能补造为“升级前采集”。旧 reader/updater 也不能消费新 v2 facts；不能只替换一个 Web 按钮或追加 Aborted 后启动旧程序。

已核对历史入库报告 `signed-inbox-after-start-20261102.json`，SHA-256 `4da0f673ff9fb9c31410fa6b6d2651c62bdd54253a702059a9a55fc471156707`。它含实际 owner/store/schema/数据库摘要、event_count=1、last_stream_seq=1；database_schema_version 在该旧报告中为 null。它只是可供进一步审查的既存档案，**不是**本协议的基线，不证明完整 SCM/worker 水位或允许直接裁决。

旧安装处理的待审查边界见[历史安装恢复审查方案](../architecture/proposals/installed-legacy-release-recovery-review.md)。本轮不删除历史、不重放原 Upgrade、不启动 Brain、不把源码门写成 IP PASS。报告结论为“恢复协议与隔离实现已闭合”，不是“当前旧安装已裁决”或“产品 IP01–IP24 已通过”。
