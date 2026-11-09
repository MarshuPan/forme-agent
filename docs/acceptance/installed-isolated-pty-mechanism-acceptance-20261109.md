# Windows 隔离 PTY 机制组件验收（2026-11-09）

实现提交：`1207740c01657fe15647e09fca71851e05b3a2b4`，分支 `shu/closure-inventory`。范围遵守[机制设计稿](../architecture/proposals/installed-isolated-pty-mechanism-spike.md)：只验证固定测试 runner 的 Low AppContainer、真实 ConPTY 与 Job 托管。不启用生产 PTY profile，不修改冻结 DTO/trait、18-crate 图、99 EventKinds 或五个 unsafe 文件白名单；不是 IP10/IP11/IP24 通过声明。

## 1. 已证明的机制

固定自建 runner 在挂起状态原子进入原命名 Job，完成 token/特权回读后才 resume。显式测试由普通权限进程执行，Windows build 为 `26200.9711`。fixture 只访问工作树证据区内本 job 的副本、两份自建 sentinel 和本次 loopback listener。

| 场景 | 实际证据 |
| --- | --- |
| 真实 PTY 正常退出 | 80×24 ConPTY，child stdin/stdout 为 terminal，固定输入往返匹配；独立核对 runner、conhost、child 的 image、creation time、Low AppContainer SID、capabilities、剩余特权和同一 Job。原 root 正常退出、所有持有原句柄的进程终止、Job 为空。 |
| 后代取消 | runner、conhost、child、grandchild 全部独立核验同一隔离与 Job；只终止原 Job，所有原进程终止后才撤 ACL、删除原 profile。 |
| 原生产派生限制 | 不开启测试分支时，固定 fixture 的子进程创建被 OS 以 `ERROR_CHILD_PROCESS_BLOCKED`（367）拒绝；未产生 grandchild。 |
| 文件、网络和环境 | 两份 sentinel 不可读且原字节保持；自建 loopback 连接被拒，listener 零接纳。最终执行前在父进程设置自建凭据标记及三项代理标记；runner/child/grandchild 均未继承这些标记或 PATH，不记录真实环境值。 |
| 清理 | 三个最终场景各有独立 PASS 回执；Job 为空、原观察句柄对应进程已退出、原 ACL/profile 清理成功。没有已安装 workspace 回写。 |

生产 `LaunchRequest` 不含可配置的派生开关；唯一实验开关位于 `cfg(test)`，服务、Web 和 WorkerJob 均不能启用它。只读进程观察留在已有 `windows_appcontainer_native.rs`；纯数据准备、等待编排和测试移到 safe 模块，原生文件维持规模门。

## 2. 原失败与恢复

先行实验的路径规范化、ConPTY 初始化/光标查询和退出后的 image 查询均出现过真实失败，原回执和日志保持，不将最后通过覆盖前次失败。最终 fixture 处理 Windows 终端握手，使用原子发布的有界观察记录。

两份原实验 `isolated-pty-b2OO9I`、`isolated-pty-l3CwYK` 已证明 token/Job，但在进程退出后查询 image 失败，清理为 UNVERIFIABLE。修正 observer 后，退出证明使用持续持有的原进程句柄、creation time 与 exit time，避免要求已退出进程仍有可查询 image；对应自动回归覆盖 PID 不在原 Job 和已退出的原句柄。

显式恢复场景仅核验原 fixture、lease、精确路径/文件身份及原命名 Job，随后撤销原四项 ACL、删除原 profile，分别生成 `observer-cleanup-recovery.json`。实际恢复 **2 份原件**，启动程序 **0**，不改原 FAIL/UNVERIFIABLE 回执，不重派动作。

## 3. 最终源码回归和绑定

完整组件检查在本地 07:32:41–07:52:15 完成，六项 exit 0：fmt、workspace all-targets check、workspace all-targets test、strict Clippy、code quality 和完整 compliance。Rust 为 **186 个测试目标、1,319 passed / 0 failed / 20 默认 ignored**；四个新增 ignored 场景是三个机制场景和一个原件恢复场景，默认忽略不计通过。结构门为 891 files / 63 existing size debts，不表示历史债务清零。

提交前逐项核对八个实现文件和六份检查日志摘要，均匹配完整检查记录。提交只包含该八个文件，未重复整仓回归。

先行显式机制日志绑定 fixture `06b073e9…`；随后完整 all-targets 构建产生了另一份 fixture 程序，不能将两份二进制混作同一采样。为此在已提交且干净的 `1207740` 上，仅对最终二进制再次显式执行三个机制场景：**3 passed / 0 failed**，07:57:52–07:57:57；不重跑已通过的完整 workspace，也不重启先前失败的原 experiment。此次三个新 experiment 及各自清理均 PASS。

最终 fixture SHA-256：`19549894b646d665742c2f0501d5f6248ad358538f857f233f24ab31ac2fab70`。最终测试程序 SHA-256：`ea754b5424772ab96a800732e26ce3e72be18332d66c1eb2b7ee2cb7fc63c0d8`。执行前后分别核对，三个实际回执均绑定同一 fixture 摘要。

固定本地证据目录为当前工作树 `target/closure-inventory-evidence/`：

| 工件 | SHA-256 |
| --- | --- |
| `isolated-pty-checks-process.json` | `8b95e35a6e98344978b4eab37d98098c0d817570cbdbd5ca34f7cd866aec2f69` |
| `isolated-pty-final-mechanism-process.json` | `c9a527d0d05e1bae2f50c8bcac090c7d111d4936899c749dc14dd7b12b6bd414` |
| `isolated-pty-final-mechanism.log` | `fc5abfdf6c77a7f69092ca7b18cfc62ace7256183a1f1f64d0ee36c6960fad72` |
| `isolated-pty-nnq8Mr/mechanism-receipt.json`（原文件策略） | `ba7b365a9c4c9c259e1c5c992a3d0f673b5c98c8e4b5b9f08b332d5610ac44f1` |
| `isolated-pty-fMWucL/mechanism-receipt.json`（后代取消） | `d579ba8111140146be51a214660b08421ce4c6ace9d0ae884448da1818438ee7` |
| `isolated-pty-y2D8bJ/mechanism-receipt.json`（正常退出） | `a026ef5d21caf27e5ac811c30de93427f5e0a55abb6afce84347a9d2ae110316` |
| `isolated-pty-original-cleanup-recovery.log` | `119d21e5e0a9beb0387cc815eb2f83224c2cb8ab71ce69b331383141d411956a` |
| `isolated-pty-b2OO9I/observer-cleanup-recovery.json` | `fd67c2eb03bc43257a02dbde4c92a21422d1ca3c7ed276a925a5edf2f35f26a6` |
| `isolated-pty-l3CwYK/observer-cleanup-recovery.json` | `3e1cee17d6e13107107e8b89f5ab0134bc374cbfec27e5ccdefe2f0444eb1317` |

## 4. 尚未关闭的要求

设计稿在 Low AppContainer 内的高输出、阻塞输入、PTY 初始化失败、错误 exit、输出截断和关闭等待故障矩阵尚未完整执行；10 秒观察期限和本次有界往返不能代替这些故障注入。后续 `451f3b4` 已专门修复普通 `PtyBackend` 的持续排空、同步写入与 reader join，并完成八项 Windows I/O 场景及完整组件门，详见[追加验收](installed-pty-io-lifecycle-acceptance-20261109.md)。该组场景未同时运行在本报告的隔离 runner 中，两个组件结果不能合并成隔离故障矩阵 PASS。生产专用 placement、Host grant、受保护输入/输出、overlay 回写、恢复与真实 SCM/session 联合证据也未完成。

本轮没有安装、升级、启停服务或修改 UAC，未刷新完整 Final/M5，也不关闭三份待确认的冻结协议提案或任何完整 IP 场景。普通 PTY I/O 的后续结果以上述追加验收为准；原生批次仍只核对原提权进程和原回执。
