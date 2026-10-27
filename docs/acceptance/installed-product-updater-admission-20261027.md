# 已提交发布补开放的时限修复与回归

2026-10-27，分支 `shu/installed-product`。契约提交 `97e6e0c`，实现提交 `0afe809a77b27000afb9a39090cd62e3184a3f45`。本报告记录源码、独立夹具及组件门，不是已安装服务恢复或 IP01–IP24 通过报告。

## 问题与修复

Host 永久门曾在 `updater_transfers_real_writers_and_child_pipe_then_leaves_target_checking_closed` 失败。前两次网络证明已经分别覆盖伪签名拒绝与有效证明提交；第三次是已 Committed 操作恢复关闭的工作准入。原实现先建立 2 秒期限，再复核大型发布文件与程序身份，网络挑战尚未开始便耗尽期限。服务端等待连接的 10035 是未发出第三次请求的表象，不能把根因定为 socket 首读。

按 `prd/24 §4.10.3` 区分两类时限：未提交 Checking 仍沿用原授权的绝对截止时刻；已提交操作先只读复核同一发布、程序与服务，再为新鲜网络证明建立 2 秒窗口。补开放不能再次 stop/switch/backup，证明或最终复核失败仍保持关闭。未改变 public trait、事件、crate 或依赖边。

回归在已提交补开放前显式注入 2100 ms 程序检查延迟，证明慢速只读复核不会提前耗尽新的网络窗口；同时保留伪签名拒绝、正确 binding 及不重复 OS 操作断言。测试 socket 显式回到 blocking，连接失败报告带证明序号。

## 验证

| 检查 | 结果与边界 |
|---|---|
| 原失败目标 | PASS，74.18 s；独立测试，不操作安装中的服务。 |
| 完整 `installed_release_staging` | 32/32 PASS，75.33 s。 |
| `tools/verify-installed-host.ps1` | 退出 0，`installed-host-foundation and core-shutdown-components: PASS`；终端回执同时明确 IP01–IP24 NOT_RUN。没有单独保存本次完整 Host 门日志。 |
| 统一质量门 | `tools/verify-code-quality.ps1 -WindowsInstallerToolchain <reviewed-config>` 退出 0；本次只将 Cargo 构建并发设为 1，未减少测试，结束后恢复环境。 |
| Rust | 176 个目标、1090 passed / 0 failed / 15 默认 ignored；fmt、workspace check 与 strict Clippy PASS。 |
| 工程/前端 | 检查器 21/21、规模棘轮 PASS（765 文件、63 项既有债务）；前端类型/92 项回归/嵌入资源核对 PASS，依赖审计 0 vulnerabilities。 |
| 包与平台组件 | bundle 10/10、包输入/POSIX 控制流 18/18、真实 Rust 签名校验 5/5、临时 NTFS 8/8、防火墙只读策略 25/25、真实低权限 Inno 退出码 2/2 PASS；不构成安装或服务 token 验收。 |
| 原创/许可证 | 默认完整 `tools/compliance-doctor.sh` PASS，未使用缺语料豁免；`git diff --check` PASS。 |

统一门日志为 `target/installed-file-worker-evidence/native-updater-readiness-quality-serial.log`，SHA-256 `59cc0d1c12475becafd160ce6e3d1ecb101211aaf03796bb93c61f7ebb6f60cb`。此前默认并发统一门失败日志 `native-updater-readiness-quality.log` 保留，包含 linker exit 66 与后续 rlib 缺失；本报告不据此断言环境根因，也不将失败改写为通过。测试前后的缓存均未删除。

任务收尾空间报告保存为同目录 `native-updater-readiness-build-resources.json`，来自 `tools/build_resources.py report --all-worktrees --json`。主工作树及 maintenance 工作树均超过 64 GiB 报告预算；统计没有读取错误，未因此执行清理或修改预算。

## 原生安装状态

获批的旧 Host observer 启动已失败，后续独立 SCM 查询仍为四服务 Stopped、PID 0，Host exit 1；实测见[安装报告](installed-product-windows-native-install-20261027.md)。本轮源码修复没有替换安装中的任何程序、配置或密钥。

现有 `694b6bb` 诊断候选没有本报告的 updater 修复，未被重新打包或安装。[Host 诊断替换计划](installed-product-windows-host-diagnostic-replacement-plan.md)及其脚本已准备、尚待批准；不得复用上一次只启动旧 Host 的批准进行替换或再次启动。原生完整产品、真实 provider/worker、签名发布及完整 IP24 仍未闭合。
