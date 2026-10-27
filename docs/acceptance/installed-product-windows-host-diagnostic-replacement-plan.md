# Windows Host 开发诊断替换计划

状态：已按 owner 批准执行一次，`failed_preserved`。承接[原生验收计划 §3](installed-product-windows-native-acceptance-plan.md)、[实际安装失败](installed-product-windows-native-install-20261027.md)和[诊断候选](installed-product-windows-candidate-20261027-694b6bb.md)。此前批准的 observer 已启动一次旧 Host 并失败；本次仅按新的明确批准替换候选 Host 并再次启动一次。

本计划是一次明确例外的本机开发诊断，**不是签名发布、升级事务或 IP 验收**。不修改正式 active-release/staging/updater 契约，不运行首装安装器，不新增服务或放宽产品的身份/权限检查。原生完整产品继续未通过。

## 1. 准确输入

| 对象 | 已核对输入 |
|---|---|
| 待执行脚本 | `C:\code\forme\target\installed-file-worker-evidence\native-host-diagnostic-replacement.ps1 -Apply` |
| 脚本 SHA-256 | `70338f9252f4a68a8363b4b2068a920ffab2f400704895253d7a0672b97b7380` |
| 候选 Host | `C:\code\forme\target\product-bundles\forme-windows-x86_64-20261027-694b6bb\bin\forme-hostd.exe`，39,406,437 bytes |
| 候选 Host SHA-256 | `570549c220f410f585c30b3c0819146a5e99fedb36afc18a4702fe59de08fcf2` |
| 安装中的 Host | `C:\Program Files\ForMe\bin\forme-hostd.exe` |
| 原 Host SHA-256 | `cd550db788cdb84e2f1058396e730f18757175f55f232b183957de710ed86b7e` |
| 原件备份 | `C:\code\forme\target\installed-file-worker-evidence\host-original-before-694b6bb\forme-hostd.exe`，必须全新，摘要等于原 Host |
| 结果报告 | `C:\code\forme\target\installed-file-worker-evidence\native-host-diagnostic-replacement.json`，create-new，不覆盖旧尝试 |

候选来自源码 `694b6bb` 的原生 release 构建，未签名。它包含 Host 脱敏阶段诊断；不包含本轮后续的 updater 补开放时限修复，不能以当前源码测试冒充此候选已具备那项修复。

## 2. 操作和保留边界

1. 管理员/UAC 下先只读核对固定根、原 inventory/配置/密钥摘要、六程序、四服务的原 image/参数/账户/启动方式、全部 Stopped、端口无占用，以及 brain/default workspace 仍为空。任一不一致即停止。
2. 保存原 Host 到上述全新备份，核对摘要。候选先复制到安装 `bin` 内唯一临时文件并核对摘要；再次核对四服务停止及所有原件后，原子替换**唯一 Host 文件**，核对摘要和原 SDDL 保持。其他五程序、安装 manifest、配置/密钥、SCM binding/账户/恢复策略不改变。
3. 只请求 Host 启动一次；观察到稳定 Running 才请求 Web 一次。Brain/updater 不启动，不认领账号，不配置真实模型，不读取或迁移仓库 `.forme` 和个人项目。原 SCM 有限自动恢复仍由 OS 执行，脚本不添加 Start 重试或第二监管者。
4. 报告只记录状态/PID/退出码，以及新诊断文件的固定 code/stage/reason digest。原始异常、配置、凭据或脑内容不写报告。Host 启动后的产品 journal/私有诊断写入是原服务的正常观察结果，不属于 brain 事实。
5. 任一失败保留原件备份、新程序、日志和已观察状态，不自动恢复旧文件、不反向切换服务、不重跑安装器。恢复或进一步重试另行明确批准；备份可用于经核对的原字节恢复，但本计划不会执行该恢复。

这会使现有安装成为显式标记的开发诊断状态，不可用于正式发布或 IP PASS。即使 Host/Web 可达，也还需正式部署路径及后续认领、真实模型/worker 和 IP01–IP24 各自证据。

## 3. 执行前已验证与待验证

脚本在 Windows PowerShell 5.1 与 PowerShell 7 完成语法解析及**独立临时目录**夹具：原子替换、原 ACL 保留、备份/数据字节保留、备份覆盖拒绝均通过。夹具曾发现 PowerShell 将 `$null` 转为非法空路径，已改为明确的 `[NullString]::Value` 并在两种 PowerShell 复验通过。

这些是脚本准备证据；默认不带 `-Apply` 时仅只读核对，不创建备份、替换文件、启动服务或写结果报告。批准必须覆盖本表脚本摘要及本节边界，不能由 UAC 出现或工具可运行推定批准。

2026-10-27 12:40:38（Asia/Shanghai）曾在当前交互 PowerShell 以默认只读模式运行该脚本。当前进程不是管理员，预检在管理员读取权限检查处安全停止；输出为 `failed_preserved`、`failure_stage=preflight`、`start_requests=[]`、`replaced_host=false`、`backup_verified=false`。四个服务仍为 Stopped，安装程序、配置、密钥、备份和 Host 文件均未改变。该结果只证明当前 shell 没有执行替换所需的权限，不构成替换失败、Host 根因结论或 IP 验收结果。

随后在 UAC 管理员上下文中以默认只读模式复跑，结果为 `preflight_passed_no_change`。候选 `forme-hostd.exe` 摘要、原 Host 摘要、四服务固定绑定与恢复策略、安装 ACL、端口和原始数据均通过；`replaced_host=false`、`backup_verified=false`、`start_requests=[]`，没有创建备份、替换文件或启动服务。证据为 `target/installed-file-worker-evidence/native-host-diagnostic-replacement-preflight-7.txt`。本次只读预检不消耗 `-Apply` 授权。

随后 owner 明确批准执行本表脚本的唯一一次 `-Apply`。备份原 Host、核对候选摘要、原子替换唯一 Host 文件均成功；配置、密钥、其他五个程序、服务绑定和原始数据保持不变。Host 只请求启动一次，在 `StartPending` 后回到 `Stopped`、PID `0`、退出码 `1`；由于 Host 未稳定运行，没有请求 Web，Brain/updater 仍未启动，四个服务最终均为 `Stopped`。结果为 `failed_preserved`，证据为 `target/installed-file-worker-evidence/native-host-diagnostic-replacement.json` 与 `native-host-diagnostic-replacement-console-9.txt`。脚本未自动回滚或重试。

本次候选已写入的脱敏 Host 诊断阶段为 `management_endpoints`。只读核对显示本机 Windows 动态 TCP 排除范围包含 `8730–8829`，而已安装旧配置中的 Host `8766`、Configuration `8768`（以及预留 Agent `8767`）均在该范围内；因此绑定失败根因已闭合为旧安装的固定内部端口与本机排除范围冲突，不是密钥、服务账户或数据损坏。源码已改为安装准备阶段探测并持久化可绑定的内部 loopback 端口；该修复尚未覆盖当前安装，后续替换/重启必须另行批准。
