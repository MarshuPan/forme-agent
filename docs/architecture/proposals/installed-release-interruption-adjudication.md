# 安装升级中断的显式裁决契约草案

状态：**owner 已于 2026-11-03 批准契约补充及实现/隔离测试；未批准本机程序替换或 journal 迁移**。精确协议见 prd/24 §4.10.5–6、prd/25 §4.4.1 和 architecture/10 §9.1。依据 prd/24 §4.10、canonical §29.6 与 IP14–15，补足现有故障恢复义务，不增加 Agent 内核、crate、依赖边或脑 EventKind。实现与原生执行分开验收。

## 1. 已复现的问题

本机 `maintenance:acceptance-upgrade-after-path-20261102-db96ec2` 已接纳，product revision 28，原状态为 `Validated / pending Quiescing`。独立 updater 消耗 dispatch claim 后退出，Web 被停止，Host 原 PID 仍 Running，Brain Stopped；原 active release、配置/ack revision 1 和原程序保持。维护 Web 已通过限定的一次启动恢复，原升级未重发。底层退出原因尚未证明。

当前 `ProductJournal::Projection::has_unfinished_operation` 将一切非 Committed 操作视为未完成；`RecoveryRequired` 也不释放互斥。现有协议没有表达“保留原失败事实、经 owner 核实后终止这次升级并恢复原版本管理”的动作。把旧 operation 改写成 Committed、删除 journal/claim、恢复旧 journal 或重新提交原 Upgrade 都不成立。新的脱敏阶段诊断只能帮助定位，不能补足裁决权。

## 2. 推荐的有限恢复语义

第一版只支持 **AbortBeforeSwitch**：明确终止尚未进入备份/切换的升级或兼容回滚，保持原版本。它是新的、重新认证的产品恢复决策，引用原 operation；不是重试原执行、延长原 deadline 或宣布升级成功。其他阶段继续保持 RecoveryRequired，必须使用与该阶段匹配的单独恢复方案。

恢复请求必须同时固定：schema version、独立恢复 operation/idempotency key、当前 product revision、原 operation id/command digest、原 release ref/digest、当前 config revision/digest、owner、恢复决定、原状态证据 digest 和新 deadline。真实 owner 来自正式认证与 operation 绑定的密码重新认证，浏览器不能自行提供身份、成功断言、文件路径或程序。CAS 或任一绑定变化即拒绝；同 key 不同内容冲突，同 key 相同内容只查询原恢复决定。

Host 在同一产品写者/互斥边界重新核对以下条件：

1. 原操作确属本 installation/owner 的 Upgrade/Rollback，原 pending stage 仅为 Quiescing；原独立 updater 已实际 Stopped，旧授权已停止接纳，dispatch claim 继续保留。
2. Brain 实际 Stopped；不存在未知 worker、仍在运行的 updater、BackedUp/Switching intent、image claim、active release 变更或 release config binding。不能仅凭某个 marker 不存在证明零副作用；必须同时核对完整 journal/配置水位及三个实际服务 image/参数/账户/恢复策略和原文件摘要。
3. 原 release/配置与同一 owner/store 连续性成立；若数据被改动、无法独占核实、备份/切换可能已经开始或文件/服务身份不明，恢复请求拒绝，不以“测试环境”降低门槛。
4. Web 可保持认证维护可达，但恢复期间不启动脑、不开放 Agent 工作准入、不替换 image、不变更配置或删除失败现场。

通过后先落恢复 intent，再落绑定原状态证据的裁决事实。原升级投影必须显示独立的 **Aborted** 终态，原 intent/receipt/failure/claim 全部保留；原 target ref 不代表已安装，不能显示“升级成功”。新裁决有自己的 Committed 回执，不冒用原升级回执。仅裁决完整落盘、回读与原版本准入核实后，才可释放该原操作占有的管理互斥；工作准入仍由正式启动、配置 ack 和当前 readiness 分别控制。

恢复决策完成后，后续修复版本才可有新的 Stage/Upgrade。旧命令重放始终返回 Aborted，不重新派发；原 Agent Unknown/action receipt 不被这次产品管理裁决改变。

## 3. 冻结协议与版本兼容

需要显式批准的协议增量是：版本化 typed 恢复请求/回执、维护查询对 Aborted 的投影、独立产品裁决事实及其 replay/CAS/幂等语义；保留原脑的全部协议、99 EventKinds 和 18-crate 图。确认后须先回填 canonical §29、architecture/10、prd/24–26 的精确类型与路由，再实现。

旧 Host/Web/updater 对 closed enum/未知 journal fact 会拒绝，不能假称 additive 即可被旧程序读取，也不能把新事实写入仍由旧 writer 使用的现场。新 reader 必须重放旧 journal 原字节/原 hash；新增事实后的兼容下限与实际 updater 版本必须显式提升。固定初始 updater 不会随业务 release 自动更新，这一点必须在迁移计划里处理。

因此本次先实现并在隔离副本上完成协议/恢复工具链；实际本机迁移须另外准备**精确签名、程序摘要、旧/新 reader 和 updater 身份、受保护原件备份、互斥切换顺序及失败留存**的有界管理员修复方案。不能为了继续本次验收直接覆盖在用程序、重新首装、修改初始安装清单或把开发补丁写成正式签名升级完成。若新 reader/updater 未能安全接入，现场仍保持原未决状态和可达的维护 Web。

## 4. 必须通过的验收

| 情况 | 必须证明 |
|---|---|
| 备份/切换前的明确中断 | 裁决引用原 command，保留原历史，原操作为 Aborted，新恢复决定有独立回执；零升级/工具副作用。 |
| 重复/并发恢复与断线 | 一次 CAS/裁决，查询原号；同 key 改参数拒绝，重启不重新停服或切换。 |
| 已开始备份/切换、未知进程或不同 owner/配置/文件 | driver=0、无裁决事实、无准入开放，原 RecoveryRequired 保留。 |
| 恢复 intent 后中断 | 只核实原决定与确切状态，不生成替代 key，不把缺回执变成成功。 |
| 旧 journal / 新 journal / 旧程序 | 新 reader 校验历史原 hash，旧 reader 对新版本显式拒绝；不能选择性跳过未知事实。 |
| 真实 Windows 安装 | 实际 SCM、签名/reader/updater 兼容、文件与 owner/store 连续性、认证 Web、恢复后的正式启动和独立新升级均有证据；组件门不替代 IP14–15/IP24。 |

## 5. 确认范围

本稿请求批准上述有限恢复契约及其实现/隔离测试。它不批准删除数据、重跑旧 updater、修改 UAC/防火墙、注销/重启机器、卸载、替换实际运行程序或迁移本机 journal；后者必须等精确的签名修复方案和只读预检完整后再作为具体动作审查。
