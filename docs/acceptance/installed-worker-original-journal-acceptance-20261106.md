# worker 原 journal 与恢复水位核验（2026-11-06）

本记录承接 PRD24 §4.10.5–7 与[历史审查协议 §9.8](../architecture/proposals/installed-legacy-release-recovery-review.md#98-worker-原件与当前静止水位)。文档提交为 `bca17da`，实现提交为 `f83b63e`；本批处理恢复时的当前 worker 事实核验，完整历史采集器与安装产品验收继续独立记录。

## 1. 已复现的问题

原 `WorkerJournal::quiescent_digest()` 仅从内存 projection 计算 sequence/head，并检查缓存 job 的终态。新增反例先在旧逻辑执行：写入两个已取消 job，再通过原持锁句柄将文件截回第一个 job 的完整合法前缀；原方法仍返回第二个 job 后的缓存水位，`is_err()` 断言实际失败。

这证明当前原件与内存可能不一致时，旧检查仍可进入恢复 callback；它不证明本机安装的真实 journal 已发生过该类损坏。本轮仅使用原创临时数据。

## 2. 实现与行为

生产检查仍在既有 worker operations 排他区内执行，保留原 OS 锁，从同一 writer 句柄有界重读完整文件。每条记录按原 closed envelope、schema、序号、hash chain 与状态机 fold，实际 sequence/head 必须等于 writer projection，随后才返回原摘要域的静止水位。

原件损坏、合法前缀截断、额外合法记录、同序号异链或 IO/位置恢复失败，使 writer 拒绝后续变更；发现不一致后，即使重新放回原字节也不会自动清除该状态。完整且一致的 Pending/Running/CancelRequested/Unknown 只拒绝静止观察，保留原状态和后续正常 journal 行为。仅 Completed/Failed/Cancelled 可通过。

重读恢复原句柄位置，保持原字节、sequence/head、OS 锁和 `forme-worker-quiescent-v1` 摘要。没有新增协议对象、冻结 trait、EventKind、依赖边或 unsafe 边界；普通恢复、升级前源基线与历史恢复沿用这一生产方法。

## 3. 验证与证据

定向 `cargo test --locked -p forme-execution --lib installed::worker::recovery::tests`：**10 passed / 0 failed / 0 ignored**。覆盖空新日志、正常终态及后续追加、原字节/任意文件位置/OS 锁/重启保持、合法前缀与清空、同序号异链、额外完整终态记录、损坏/未知字段/未知 schema/残行、单行与总量上限、四种未决状态和 writer 拒绝继续写入。十个命名场景均已加入 `tools/verify-installed-host.ps1`。

完整源码质量门 `tools/verify-code-quality.ps1` 已结束，exit 0：182 Rust targets、**1262 passed / 0 failed / 15 默认 ignored**，fmt、locked workspace check/build/test、strict Clippy、完整合规及结构门通过；前端 **115/115**，可重复资源、签名/打包、真实临时 NTFS **8/8** 与防火墙只读 **25/25** 通过。862 个源文件、63 处既有规模债务未增长。未提供 Inno toolchain 的原生安装器 exit fixture 为 NOT_RUN。

永久宿主门 `tools/verify-installed-host.ps1` 也已结束，exit 0，最终输出 `installed-host-foundation and core-shutdown-components: PASS`；上述十个反例及既有 Host/恢复/数据子进程/bootstrap/管理修复场景均实际执行。该门明确输出 `IP01-IP24: NOT_RUN`，没有安装服务或认证原生安装包。没有因本批重跑 M5、刷新发布回执、合入 main 或推送。

中间一次正向夹具误在另一个仍持有 OS 锁的 writer 外直接读取文件，Windows 返回 error 33。夹具改为先释放重开的 writer，再检查字节；该失败与修正已保留在任务记录，最终定向门为上述 10/10。

本地日志位于本工作树 `target/legacy-worker-source-20261106/`，不随公开源码分发。质量及宿主命令使用 `2>&1 | Tee-Object`，保存 stdout/stderr，没有保存 PowerShell `Write-Host` 的 information stream 6；宿主的逐项 PASS 和最终结论来自本次实际工具输出，不声称其日志包含完整命名执行清单，也不为补日志格式重复整门。

| 本地工件 | SHA-256 |
|---|---|
| `source-quality.log` | `b36bfa6ffee67b0204d4b64ae8fbac06dd53eb56e7d78320710f4d94c0714433` |
| `worker-source-tests.log` | `1602161126fdbb333096a3cb15e6e476606400fe7bd0235d1e7c50facc191c40` |
| `installed-host.log` | `eca77c6e5492f12d7990ce877666e147f37bfef3476fec49fcfdb0eac2233d91` |
| `build-resources-final.json` | `a1cf5ca671aec307cfecf5d62c86f3abd3775ba2861be7fff22c4022c9336fb0` |

最终固定 target 空间报告为 PASS：逻辑 44,569,430,206 bytes（约 41.51 GiB），分配 41,691,397,212 bytes（约 38.83 GiB），低于 64 GiB 报告阈值；本批没有删除构建缓存或其他内容。

## 4. 当前产品边界

当前完整原生 `LegacyReviewCollector`、独立历史来源、原 updater 审计/worker 连续性、管理 reader 修复与实际现场迁移仍未闭合。当前 writer/原件一致和新建空日志，均不能替代升级前历史来源或证明历史从未执行；测试中的可信 collector 不作为生产 Verified。

本轮开始时原 Host/Web/Brain/updater 四服务均为 Stopped，未执行 UAC、SCM 变更、服务启动、原件迁移或原 Upgrade 重派。[原 Host 路径恢复计划](installed-original-host-path-restore-20261105-plan.md)的一次 Apply 确认仍待答复；上一次只读 UAC 已取消，没有管理员进程仍在等待。

M5/closure 发布回执、真实模型、原生安装和 IP01–IP24 不由本批源码证据升级；总目标保持未完成。
