# Windows 开发安装与限定恢复实测

2026-10-27，在 `shu/installed-product` 上按 owner 对准确候选及恢复脚本的两次批准执行。这里记录真实 OS 副作用及失败，不是 IP01–IP24 通过报告。

## 准确输入与权限

- 首装候选：`target/product-bundles/forme-windows-x86_64-20261027-7cb5b8b-package/artifacts/forme-0.0.1-windows-x86_64.exe`，73,225,411 bytes，SHA-256 `8703938b3b2e24f13b759583838cb70b3632c8de7ef4b400a6aaea5f0ecc6658`。Authenticode `NotSigned`，只作本机开发验收。
- 目标仅 `C:\Program Files\ForMe`、`C:\ProgramData\ForMe` 及 `forme-host/web/brain/release-updater`。未使用仓库 `.forme`、真实模型或原有个人项目。
- owner 另行批准 `target/installed-file-worker-evidence/native-install-recover.ps1`：核对原文件与服务绑定后，使用准确 helper 运行 Grant → Verify → Start，仅尝试 Host/Web；不重新安装、重置密钥或启动 Brain。

## 已发生的真实结果

| 步骤 | 实测 |
|---|---|
| 首次安装 | 创建受保护产品目录、六程序/资源、初始配置/密钥和四个 SCM 注册。Host/Web 为自动，Brain/updater 为按需；账户分别为 LocalSystem、固定虚拟服务账户。 |
| 首次失败 | ACL 封闭 inventory 漏掉 prepare 新生成的 `config/worker.json` 与 `workspaces/default`，在 Grant 阶段拒绝。没有越过拒绝继续启动。 |
| 安装器退出缺陷 | 原候选的 post-install 异常仍返回 0，不能把它解释成安装成功。源码新增完整阶段成功标记与自定义退出码，并用真实 Inno 夹具验证失败退出 1、成功退出 0。 |
| 限定恢复 | 原配置/密钥及程序摘要、服务原绑定/停止状态预检通过；修正后的 34 项矩阵 Grant、Verify 均通过。Start 阶段 Host 退出 1，Web/Brain/updater 未启动。恢复脚本返回 1，后续停止。 |
| OS 有限恢复 | SCM 独自按 5/30/120 秒尝试后停止。系统事件记录第 4 次停止；没有添加应用重试循环。 |
| 后续只读核验 | 四服务均 Stopped、PID 0；原配置/密钥文件逐一摘要仍等于首装诊断基线。Host 已创建 bootstrap/host/worker journal 和 sandbox lease 文件，未认领、未启动 Brain、未接入真实模型。 |

恢复 JSON 的 `original_files_unchanged=false` 是成功收尾字段没有赋值，不能据此推断原文件改变；后续逐文件只读核验已单独证明配置/密钥原字节保持。失败结果原样保留，不改写原回执。

## 源码修复和验证边界

首装 ACL 将新增 worker 配置限制为 SYSTEM/Administrators，将默认 workspace 纳入既有 Brain Modify 分区；在任何 ACL 修改前核对整个封闭 inventory，未知项或类型置换仍拒绝。原生产服务绑定/权限检查不放宽。

真实安装还暴露两处测试/入口问题：纯 registration 测试不应检查机器上的真实受保护安装，已换为独立临时路径；前台 managed Host 测试曾依赖全机没有 Brain Service SID 才退出。正式入口现提前拒绝非 SCM 的 `--config`，增加零 journal 断言；前台开发仍须显式 `--development-config`。

正式服务另有脱敏的阶段诊断，用于定位此后启动/运行失败；当前安装仍是原候选字节，没有把源码修复静默覆盖到安装中。原 Host 服务失败的精确原因尚未闭合，不按猜测放宽身份、路径、恢复或沙箱检查。

专项证据目录为 `target/installed-file-worker-evidence`：

- `native-install.log`、`native-install-diagnosis.json`、`native-install-recovery.json`、`native-host-readonly.json`：实际安装、恢复与独立只读观测。
- `native-layout-ps7.log`、`native-layout-ps51.log`：两种 PowerShell 的 8/8 ACL/拒绝回归。
- `native-installer-exit.log`：真实低权限 Inno 退出码夹具 2/2。
- `native-registration-fixtures.log`：registration 4/4；`native-managed-host-fixed.log`：前台拒绝 4/4；`native-startup-diagnostics.log`：Host 二进制 14/14（含脱敏/有界/拒绝覆盖）。
- 早期两次统一门失败与串行复跑失败日志仍保留；不能把它们描述成环境问题或通过证据。修复后的完整门独立记录。

修复后的 `tools/verify-code-quality.ps1 -WindowsInstallerToolchain <reviewed-config>` 已退出 0，日志为 `native-recovery-quality.log`：176 个 Rust 测试目标、1090 passed / 0 failed / 15 默认 ignored；fmt、workspace check、strict Clippy、结构棘轮、前端 strict 类型/92 项行为回归/嵌入资源重建核对、bundle 10/10、原生包输入/POSIX 控制流 18/18、真实 Rust 签名校验 5/5、NTFS 8/8、防火墙只读策略 25/25、实际 Inno 退出码 2/2、完整默认合规均 PASS。结构门为 765 文件、63 个既有规模债务，未刷新基线。此为当前源码及独立夹具证据，不能证明已安装旧候选的 Host 已恢复。

## 仍开放

Host 正式启动、Web 初始化、独立测试 owner 认领、真实 provider/worker、注销/冷启动、原生升级/恢复/卸载均未因此通过。IP01 的正式签名、IP11 helper、跨平台与完整 IP24 继续开放；Linux/macOS 保持既定不发布边界。

后续重试、替换安装程序或移除服务/目录必须遵守原生计划 §3 的具体恢复确认；不可重新跑首装安装器，也不可用新代码测试通过宣称旧安装已经修复。

## 已批准的限定诊断启动

owner 随后明确批准 `native-host-start-observer.ps1 -StartServices`。实际证据 `target/installed-file-worker-evidence/native-host-observed-start.json` 记录：2026-10-27 03:29:36–03:29:38 UTC 只请求启动 `forme-host` 一次，随后观察到 Stopped、PID 0、Win32 exit 1；Host 未稳定，故没有请求启动 Web，Brain/updater 也没有启动。原配置/密钥和程序逐文件摘要核对通过，报告为 `failed_preserved`。

后续当前只读查询仍为四服务全部 Stopped。这次批准的启动已经执行完毕，不能重复使用该批准再次 Start。旧二进制没有新阶段诊断，退出 1 仍不足以判断具体失败模块；待审的[Host 开发诊断替换计划](installed-product-windows-host-diagnostic-replacement-plan.md)只准备了候选、原件备份与一次启动的具体路径，尚未执行。

后续源码组件回归另见[已提交发布补开放修复报告](installed-product-updater-admission-20261027.md)：`0afe809` 修复大型文件复核耗尽 live-proof 窗口的问题，Host 门与完整源码质量门通过。这不是旧安装启动失败的根因结论，也没有更新安装中的二进制；原安装仍保留失败状态。

随后对[Host 诊断替换计划](installed-product-windows-host-diagnostic-replacement-plan.md)执行了一次默认只读预检。当前交互 PowerShell 不是管理员，脚本在管理员读取权限检查处停止；没有写安装目录、替换 Host、创建备份或启动服务。该预检不消耗替换授权。

在上述失败后，按限定诊断范围通过 UAC 以管理员再次执行了同一 observer。Host 的唯一一次启动请求实际进入 `StartPending`，随后回到 `Stopped`，SCM/Win32 退出码为 `1`；脚本因此没有请求 Web，Brain/updater 仍未启动，四个服务最终均为 `Stopped`、PID `0`。原安装程序、配置、密钥和服务绑定摘要保持不变。证据为 `target/installed-file-worker-evidence/native-host-observed-start.json`。

随后以管理员运行了 Host 诊断替换脚本的无 `-Apply` 预检，结果为 `preflight_passed_no_change`：候选 Host、原 Host 摘要、服务 SID/账户/启动参数/恢复策略、ACL、端口和原始数据边界均通过核对；没有创建备份、替换 Host 或启动服务。证据为 `target/installed-file-worker-evidence/native-host-diagnostic-replacement-preflight-7.txt`。

owner 随后明确批准执行一次 `native-host-diagnostic-replacement.ps1 -Apply`。原 Host 备份、候选摘要核对和唯一 Host 文件原子替换均成功；配置、密钥、其他程序、服务绑定和原始数据保持不变。新 Host 只请求启动一次，随后在 `management_endpoints` 阶段失败，Host 回到 `Stopped`、PID `0`、退出码 `1`；Web 未请求，Brain/updater 未启动，四服务最终均为 `Stopped`。失败现场未自动回滚或重试，证据为 `target/installed-file-worker-evidence/native-host-diagnostic-replacement.json`。

根因已由脱敏诊断和本机只读 OS 核对闭合：Windows 动态 TCP 排除范围为 `8730–8829`，覆盖旧安装配置的内部 Host `8766`、Configuration `8768` 和预留 Agent `8767`，所以 Host 的管理端点无法绑定。源码已修复为安装准备阶段探测可绑定的内部 loopback 端口并将同一结果写入 Host/Web/Core 配置；当前已安装实例仍保持失败诊断状态，修复尚未写入该实例。该结果不提升任何 IP01–IP24 状态。
