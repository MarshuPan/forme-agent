# 普通 Shell 子进程与输出清理组件验收（2026-11-09）

分支 `shu/closure-inventory`，实现提交 `b131832`，实施约束先由 `e665415` 回填 PRD08 §5.2。此次修复现有 timeout/cancel/output/Unknown 与资源所有权，不改变公开 DTO/trait、18-crate 图、99 EventKinds 或五个原生安全代码边界。

## 1. 已复现的问题

自建有限子进程启动一个持有继承 stdout/stderr 的有限后代，分别触发超时、取消、严格输出预算和直接父进程先退出。旧 monitor 在这些路径仍无限等待输出接收或 reader join，原调用不能在清理期限内返回。原实现专项实际为 3 passed / 4 failed，四次均触发 `shell exceeded cleanup bound`；失败日志保留，未以修复后的结果覆盖。

## 2. 实现与验证边界

- `shell/process.rs` 明确持有原 child、stdout/stderr reader、输出队列和进程名额。每块最多 4 KiB，共享队列最多十六块，读取错误与 EOF 区分。
- 正常退出、超时、取消、预算和观察错误共用显式清理；直接 child 的 kill/wait 与 reader join 由同一清理任务持有，原调用最多等待两秒。直接 child 退出不能单独证明继承管道已经关闭。
- 同一进程最多八个 Shell 槽位。原清理未确认时只产生唯一 `ActionOutcomeUnknown`，由 supervisor 保留原句柄并阻止新 backend 绕过准入；清理线程创建失败也保留原任务，确认清理失败继续阻断。
- 原清理稍后结束只释放资源，不改写 Unknown、不重派原动作、不推进原 run。派发前取消、正常成功/失败和已确认停止的取消仍保持原语义。

这是普通 Shell 组件证据，不证明后代 OS 隔离或安装版 placement。测试使用真实有限子进程及独立 marker/结束观察，没有使用外部模型或本机系统服务。

## 3. 检查结果与原件

专项 **8/8**，另有真实读取失败的单元场景；覆盖四个继承管道故障、原 marker 恰一次、另一 backend 零派发、原清理 join 后仅新动作可运行、八槽容量和原普通终态。完整 workspace 为 **189 targets、1,355 passed / 0 failed / 22 默认 ignored**，专项不重复叠加计数。fmt、all-targets check/test、strict Clippy、质量与完整合规均 exit 0；质量门为 900 files / 63 existing size debts，未放宽基线。

原检查在本地时间 11:49:21–12:07:57 完成 fmt/check/test 后，被原 20 GiB 空间门阻止。对本工作树空闲 dev 缓存执行 NTFS 压缩后，可用空间恢复到约 36 GiB，未删除文件；原资源拒绝保持。随后只续跑剩余检查，Clippy 在测试夹具的故意父进程退出路径发现 `zombie_processes`，原 exit 101 保留。修正夹具以显式退出表达该故障，没有增加 lint 豁免。

最终检查绑定修正后的四份精确源码，在 **12:16:20–12:32:56** 实际重新完成全部七阶段，exit 0。由于测试源码发生变化，这次全量测试是新的验证，不能与原测试合并冒充同一源码。实现提交前后再次逐份核对摘要一致。独立 verifier 核对失败原件、最终脚本/日志/源码摘要、命名场景、全量计数及资源报告并落盘 PASS。

工件均位于本工作树 `target/closure-inventory-evidence/`：

| 工件 | SHA-256 |
| --- | --- |
| `shell-io-original-reproduction.log` | `0a1af82112187b31c0415a51ddbee9ac51b1e9261330a27966d11049a8e3e997` |
| `shell-io-final-checks-process.json`（原空间门拒绝） | `e5605372db8dce07c4f73b94b24666cacfdc5b69dd59a8c087c25ffd73f3504a` |
| `shell-io-continuation-process.json`（原 Clippy 拒绝） | `9f6408a9c23761a958ee4dade2e592b46c21027bb5617d5e940ac14f6d486550` |
| `shell-io-reviewed-checks-process.json` | `4dd9a5ce1779c00add9880dfb38e0c6c6f4f1df7e32be684445d88d81dfc0cb9` |
| `shell-io-reviewed-test.log` | `f96b3b98084768ac230f3f80b51ddc024fb541dff23a1d857ae06663493c4d51` |
| `shell-io-reviewed-build-resources.json` | `532413beef5a25675a8af71b22cf80ca5d70fdf018b3309644cc02746377ba44` |

最终空间报告 PASS：固定 target 缓存约 45.2 GiB 逻辑 / 16.9 GiB 分配，低于原 64 GiB 预算。两次先行停止回执与最终成功回执分别保存，不修改资源阈值。

## 4. 尚未完成的安装义务

安装版 Shell 仍须经过 Host/WorkerBroker 的实际隔离、受保护输入/结果、生产 placement 和真实服务验收，继续见[剩余义务审计](installed-product-remaining-obligations-audit-20261109.md)。本次没有部署修复、刷新完整 Final/M5 或提升 IP01–IP24。

本机原升级 `maintenance:correction-scope-upgrade-20261109` 尚未结算，后续只核实原号；不因组件通过重派旧升级。受管消费者、桌面授权与结果三份冻结提案继续等待明确答复。Windows 提权与测试授权分开处理，此次未修改全机 UAC 策略。
