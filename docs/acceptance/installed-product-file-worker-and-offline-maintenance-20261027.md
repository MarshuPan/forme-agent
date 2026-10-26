# 安装文件 worker、离线核验与活动程序组件验收

日期：2026-10-27；分支：`shu/installed-product`。实现提交为 `cc43440`、`1270e54` 与活动程序绑定修复 `7cb5b8b`；承接 PRD24 §4.1.1d、§4.10.4、PRD25 §11.13 及既有 worker 结果通道。先落文档再实施，没有增加 crate、内部依赖边、EventKind 或 unsafe 例外。

本报告仅记录各层实际证据；没有安装系统服务、读取本机实际 `.forme`/模型密钥或发布产品，不关闭 IP01–IP24。

## 1. 交付的行为

- `cc43440` 的真实 `forme-executord --installed-file-worker` 仅执行明确支持的 File Read/Write/Delete。原目标经 Host 规范化到 exact workspace-relative target，worker 只访问本 job 私有副本；Host 依据原树身份/内容、最终计划、变更范围和 durable intent/receipt 核验回写，不能执行 shell 或回退到 brain 进程内 driver。
- `1270e54` 的 Windows 首次安装生成固定 file profile、`workspace:default` 与受保护 `worker.json`。认领前不创建脑/owner/core 配置；旧安装不静默迁移。Host schema 9 持久 worker projection，旧 revision、命令重放和配置原字节保持，正式 ProductionRuntime 只给已匹配 scope/backend 的能力准入。
- 完整 bundle、发布签名输入、native 编译快照和 Inno 文件清单共用六程序集，必须包含 `forme-executord`；缺少它不能生成“完整安装”候选。新增文件和代码来源仍通过现有原创/许可证门。
- `POST /api/v1/product/data/verify` 经独立配置桥调用固定 `forme-hostctl maintenance-verify-backup`。owner/session/Origin/CSRF/幂等键和 install/revision/digest 绑定保持；配置锁与生命周期 fence 覆盖核验。core 离线时用原 key/加密备份检查完整性、事件连续性和 replay，不启动模型/runtime，不恢复数据、不换 active pointer、不产生脑事件。worker 上限 8 秒、外层桥 10 秒。
- Web 升级/兼容回滚控件使用已有 Host 操作、最终候选绑定、近期重新认证和原 operation 核实。响应丢失及刷新后只查原号，不再提交一次副作用；不可用动作不显示为可执行。
- 活动程序绑定修复：初始 InstallationManifest 和 `worker.json` 不改写。Host 从原 product journal 的 active release 重新取得同安装 committed staging/签名/闭合集核验句柄，供 core peer image、保留 file profile 和离线 hostctl 使用。仅暂存、staging 丢失或实际文件被替换时拒绝，无旧程序 fallback；自定义 profile 不因此重写或启用。

## 2. 当前源码质量证据

`tools/verify-code-quality.ps1` 本轮退出码 0，完整日志为 `target/installed-file-worker-evidence/active-programs-quality.log`：

| 检查 | 实际结果 |
|---|---|
| workspace 全部 Rust targets | 176 targets；1088 passed / 0 failed / 15 默认 ignored。ignored 不计为原生通过。 |
| fmt、all-targets check、strict Clippy | PASS。 |
| 前端依赖/类型/行为/可重复资源 | PASS；行为测试 92/92。 |
| bundle | 10/10 PASS。 |
| native 输入与 POSIX 包控制流 | 18/18 PASS，不等于原生安装。 |
| 离线签名与真实 Rust release verifier | 5/5 PASS，不是产品签名/发布。 |
| Windows NTFS 与只读防火墙策略 fixture | PASS；不更改真实规则，不等于 SCM/跨设备 LAN 通过。 |
| 规模/依赖边与完整默认合规 | PASS；763 files、63 项既有规模债务，未刷新 baseline。 |

活动程序新增三个命名场景先单独执行 3/3 PASS，日志 `active-programs-tests.log`；随后包含在上述全 workspace 回归中。它们证明暂存不选用、升级/重启绑定、配置不改写、篡改/丢证据拒绝和 custom profile 保留，不替代真实服务升级。

`tools/verify-installed-host.ps1` 本轮完成 211 个命名场景，全部实际执行并 PASS，日志 `final-host.log`；末尾仍明确 IP01–IP24 NOT_RUN。它覆盖当前源码的 Host/worker、恢复、结果封存、AgentExecution peer、离线核验与活动程序绑定，不能证明本机实际注册服务或 LocalSystem 运行。

`tools/verify-product-configuration.ps1` 本轮退出 0，日志 `final-configuration.log`：配置 69/69、网络 fence 4/4、脑偏好 3/3、managed skills 2/2，正式产品二进制构建及四项真实临时进程场景 4/4 PASS（进程场景串行约 156 秒）。它再次实际验证 core 停止后的固定 hostctl 加密备份核验、配置原字节不变、模型请求数不增长与错误 revision/路径拒绝，没有调用真实外部 Provider 或 SCM 服务。

原生文件入口显式执行日志 `final-native-file.log`：7 passed / 0 failed，含真实低完整性 AppContainer 中的文件二进制、Host 回写、加密结果与失败/取消/执行后替换无回写。唯一 `native_file_entry_rejects_a_symbolic_link_target` 明确 filtered out：本机此前创建 symlink 返回权限 1314，未提权或修改 Developer Mode；该项不计通过。其余 symlink/junction 参数与路径拒绝已有可运行 fixture，但不能替代这个原生 symlink 场景。命令如下：

```powershell
cargo test --locked -p forme-execution --test installed_file_worker_contract -- --skip native_file_entry_rejects_a_symbolic_link_target --include-ignored
```

## 3. 进程与浏览器证据的边界

前一提交 `1270e54` 的完整源码质量门日志为 `offline-quality.log`，当时 1085 Rust tests PASS；之后新增活动路径测试不能借该旧门声称通过，已以上述当前源码门重跑。

此前正式二进制的四项临时进程场景串行执行 4/4 PASS，日志 `offline-processes-serial.log`：配置/认领/重启、core 停止时离线备份核验、无额外模型请求、网络恢复与 Web 单例竞争。此前失败日志保留，第二次在负载下出现正向核验 502；串行通过没有证明该次失败的根因，不把它描述为已定位并发缺陷，也没有放宽 8 秒限期。

浏览器记录 `browser/summary.json` 与四张截图：`actual-offline-maintenance.png`、`upgrade-controls-component.png`、`lost-response-original-operation.png`、`rollback-controls-narrow.png`。第一张为真实临时 Web 登录且 core 离线；其余通过前端响应 fixture 验证升级/回滚控件、一次 mutation、刷新恢复与原 operation GET。390px 无溢出，pageerror 为 0。前端 fixture 不证明真实签名升级、SCM 切换或备份恢复。

本机 evidence 目录位于 ignored `target/installed-file-worker-evidence/`，不包含实际 owner 数据或密钥；本地日志/截图不是可移植签名产品验收工件。

## 4. 保持开放

- 当前源码 S1–S99/适用 closure 与 clean-source release audit 按实际完成结果另记，不能沿用历史回执；Host/配置/原生文件入口专项已按上文复验，原生 symlink 单项未通过。
- 真实 SCM/LocalSystem、服务账户/ACL、冷启动/注销、完整安装→认领→配置→工具→重启→升级/卸载，以及 IP01–IP24 联合验收尚未关闭。
- 生产桌面仍使用 `UnavailableDesktopHelper`；原生会话 helper 未实现，不能把文件 worker 进展记作 IP11。具体待审边界见 `../architecture/proposals/installed-windows-desktop-helper-native-boundary.md`。
- MCP/plugin/package/channels 按 PRD25 §11.10–11.11 保持实际 backend 可用性；不制造假开关或 fallback。
- Linux/macOS 按 owner 选择继续有限服务恢复与 `release_ready=false`，不扩大原生支持/发布声明。模型认知长期收益、真实 Provider 和原生平台均不能由组件门证明。
