# Windows 开发安装与限定恢复实测

2026-10-27，在 `shu/installed-product` 上按 owner 对准确候选及后续限定恢复的分别批准执行。**当前 Host/Web 均已 Running，`http://127.0.0.1:18765` 真实浏览器初始化页可见；未认领，Brain/updater 仍停止。** 这里保留每次真实 OS 副作用、失败及后续恢复证据，不是 IP01–IP24 通过报告。

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
| 该阶段只读核验 | 四服务均 Stopped、PID 0；原配置/密钥文件逐一摘要仍等于首装诊断基线。Host 已创建 bootstrap/host/worker journal 和 sandbox lease 文件，未认领、未启动 Brain、未接入真实模型。后续端口修复和 Host/Web 启动证据见文末。 |

恢复 JSON 的 `original_files_unchanged=false` 是成功收尾字段没有赋值，不能据此推断原文件改变；后续逐文件只读核验已单独证明配置/密钥原字节保持。失败结果原样保留，不改写原回执。

## 源码修复和验证边界

首装 ACL 将新增 worker 配置限制为 SYSTEM/Administrators，将默认 workspace 纳入既有 Brain Modify 分区；在任何 ACL 修改前核对整个封闭 inventory，未知项或类型置换仍拒绝。原生产服务绑定/权限检查不放宽。

真实安装还暴露两处测试/入口问题：纯 registration 测试不应检查机器上的真实受保护安装，已换为独立临时路径；前台 managed Host 测试曾依赖全机没有 Brain Service SID 才退出。正式入口现提前拒绝非 SCM 的 `--config`，增加零 journal 断言；前台开发仍须显式 `--development-config`。

正式服务另有脱敏的阶段诊断，用于定位当时的启动/运行失败；在该阶段安装仍是原候选字节，没有把源码修复静默覆盖到安装中。后续端口修复已按独立批准执行并单独留存证据，不改写本节历史结果。

专项证据目录为 `target/installed-file-worker-evidence`：

- `native-install.log`、`native-install-diagnosis.json`、`native-install-recovery.json`、`native-host-readonly.json`：实际安装、恢复与独立只读观测。
- `native-layout-ps7.log`、`native-layout-ps51.log`：两种 PowerShell 的 8/8 ACL/拒绝回归。
- `native-installer-exit.log`：真实低权限 Inno 退出码夹具 2/2。
- `native-registration-fixtures.log`：registration 4/4；`native-managed-host-fixed.log`：前台拒绝 4/4；`native-startup-diagnostics.log`：Host 二进制 14/14（含脱敏/有界/拒绝覆盖）。
- 早期两次统一门失败与串行复跑失败日志仍保留；不能把它们描述成环境问题或通过证据。修复后的完整门独立记录。

修复后的 `tools/verify-code-quality.ps1 -WindowsInstallerToolchain <reviewed-config>` 已退出 0，日志为 `native-recovery-quality.log`：176 个 Rust 测试目标、1090 passed / 0 failed / 15 默认 ignored；fmt、workspace check、strict Clippy、结构棘轮、前端 strict 类型/92 项行为回归/嵌入资源重建核对、bundle 10/10、原生包输入/POSIX 控制流 18/18、真实 Rust 签名校验 5/5、NTFS 8/8、防火墙只读策略 25/25、实际 Inno 退出码 2/2、完整默认合规均 PASS。结构门为 765 文件、63 个既有规模债务，未刷新基线。此为当前源码及独立夹具证据，不能证明已安装旧候选的 Host 已恢复。

## 仍开放

在本报告前半段记录的失败阶段，Host 正式启动、Web 初始化、独立测试 owner 认领、真实 provider/worker、注销/冷启动、原生升级/恢复/卸载均未通过。文末已补充 Host/Web 的成功启动与初始化页证据；owner claim、真实 provider/worker、IP01 的正式签名、IP11 helper、跨平台与完整 IP24 仍开放，Linux/macOS 保持既定不发布边界。

后续重试、替换安装程序或移除服务/目录必须遵守原生计划 §3 的具体恢复确认；不可重新跑首装安装器，也不可用新代码测试通过宣称旧安装已经修复。

## 已批准的限定诊断启动

owner 随后明确批准 `native-host-start-observer.ps1 -StartServices`。实际证据 `target/installed-file-worker-evidence/native-host-observed-start.json` 记录：2026-10-27 03:29:36–03:29:38 UTC 只请求启动 `forme-host` 一次，随后观察到 Stopped、PID 0、Win32 exit 1；Host 未稳定，故没有请求启动 Web，Brain/updater 也没有启动。原配置/密钥和程序逐文件摘要核对通过，报告为 `failed_preserved`。

该次 observer 结束时的只读查询为四服务全部 Stopped。这次批准的启动已经执行完毕，不能重复使用该批准再次 Start。旧二进制没有新阶段诊断，退出 1 仍不足以判断具体失败模块；随后已按独立批准完成端口修复和 Host/Web 启动，相关新证据不改写本段 observer 结果。

后续源码组件回归另见[已提交发布补开放修复报告](installed-product-updater-admission-20261027.md)：`0afe809` 修复大型文件复核耗尽 live-proof 窗口的问题，Host 门与完整源码质量门通过。这不是旧安装启动失败的根因结论，也没有更新安装中的二进制；原安装仍保留失败状态。

随后对[Host 诊断替换计划](installed-product-windows-host-diagnostic-replacement-plan.md)执行了一次默认只读预检。当前交互 PowerShell 不是管理员，脚本在管理员读取权限检查处停止；没有写安装目录、替换 Host、创建备份或启动服务。该预检不消耗替换授权。

在上述失败后，按限定诊断范围通过 UAC 以管理员再次执行了同一 observer。Host 的唯一一次启动请求实际进入 `StartPending`，随后回到 `Stopped`，SCM/Win32 退出码为 `1`；脚本因此没有请求 Web，Brain/updater 仍未启动，四个服务最终均为 `Stopped`、PID `0`。原安装程序、配置、密钥和服务绑定摘要保持不变。证据为 `target/installed-file-worker-evidence/native-host-observed-start.json`。

随后以管理员运行了 Host 诊断替换脚本的无 `-Apply` 预检，结果为 `preflight_passed_no_change`：候选 Host、原 Host 摘要、服务 SID/账户/启动参数/恢复策略、ACL、端口和原始数据边界均通过核对；没有创建备份、替换 Host 或启动服务。证据为 `target/installed-file-worker-evidence/native-host-diagnostic-replacement-preflight-7.txt`。

owner 随后明确批准执行一次 `native-host-diagnostic-replacement.ps1 -Apply`。原 Host 备份、候选摘要核对和唯一 Host 文件原子替换均成功；配置、密钥、其他程序、服务绑定和原始数据保持不变。新 Host 只请求启动一次，随后在 `management_endpoints` 阶段失败，Host 回到 `Stopped`、PID `0`、退出码 `1`；Web 未请求，Brain/updater 未启动，四服务最终均为 `Stopped`。失败现场未自动回滚或重试，证据为 `target/installed-file-worker-evidence/native-host-diagnostic-replacement.json`。

根因已由脱敏诊断和本机只读 OS 核对闭合：Windows 动态 TCP 排除范围为 `8730–8829`，覆盖旧安装配置的内部 Host `8766`、Configuration `8768` 和预留 Agent `8767`，所以 Host 的管理端点无法绑定。源码已修复为安装准备阶段探测可绑定的内部 loopback 端口并将同一结果写入 Host/Web/Core 配置；在端口恢复执行前该修复尚未写入已安装实例，之后的恢复与启动结果见本报告末段。该结果不提升任何 IP01–IP24 状态。

进一步独立 bind 核验确认公共 Web `8765` 同样返回 `10013`，而 `18765–18769` 均可绑定。随后执行了[四文件十个端口值的限定恢复计划](installed-product-windows-port-recovery-plan.md)：管理员只读预检和 V2 保护校验通过，主脚本和原子替换函数的 PowerShell 5.1/7 临时夹具各 23 项断言通过；Apply 及后续 Host/Web 启动结果见本报告末段。

## 端口恢复的一次执行与保护 ACL 修正

owner 随后明确批准一次 `native-internal-port-recovery.ps1 -Apply`。该次已执行并退出 `1`，阶段为 `replace_config`：第一份空 staging 未获得与原配置完全相同的受保护 DACL，校验拦截，`changed_files=[]`、`start_requests=[]`；四原件备份已核对成功。没有执行任何配置原子替换或服务 Start。

管理员独立只读诊断 `native-port-recovery-acl-diagnosis.json` 返回 `originals_verified_staging_preserved`：原配置、既有数据/密钥、六程序和四备份摘要保持；brain/default workspace 为空；仅留下 `host.json.internal-port-recovery.tmp` 零字节文件，原件和失败现场均保留。该诊断完成时四个服务为 Stopped、PID 0。不能仅凭原报告未赋成功值的 `protected_files_unchanged=false` 推断原文件改变。

根因是磁盘读取的 ACL 对象没有修改标记，传给 SetAccessControl 后新文件仍使用继承 DACL。随后准备从原 SDDL 新建安全描述符的 V2，只在新 staging 写入并精确复核 owner/group/DACL；不放宽原安装权限，不删除旧 staging。管理员 PowerShell 5.1/7 保护 ACL 夹具各 30 项通过，V2 只读预检退出 `0`、全域 67 项快照、零配置/启动变化。具体摘要及后续实际结果见[恢复计划 §6](installed-product-windows-port-recovery-plan.md#6-v2-恢复与实际结果)。预检不等于 Apply 或 IP01–IP24 通过。

owner 随后另行批准并执行一次 V2。四配置的十值修改及启动前全域复核成功；脚本在唯一 Host 启动尝试处退出 `1`，Web 未尝试。管理员独立只读确认四新摘要与原 ACL 相符，其他数据/密钥、六程序、两套备份和旧 staging 均保持；该次失败收尾时四服务仍停止。旧 Host 诊断时间 `05:02:32 UTC` 早于此次 `08:30:32 UTC`，不是新的失败证据。

只读复现证明 PowerShell 5.1 的新 Job 缺少 ServiceController 程序集，构造控制器前报 TypeNotFound；原脚本在发 Job 前记的 start_requests 只能证明尝试，不能证明 SCM 接受。已准备显式加载程序集、保存子进程结构化结果的纯启动脚本；两种 PowerShell 各 9 项零 Start 观察断言和管理员只读预检通过，不再写配置。新的准确路径/摘要及限定一次启动待确认，见[恢复计划 §7](installed-product-windows-port-recovery-plan.md#7-已批准的纯启动修正)。不重复 V2、不删除失败现场，不提升 IP01–IP24。

## Host/Web 已启动与初始化页面

owner 随后批准一次纯启动。Host 唯一 Start receipt 返回成功/StartPending，实际服务进入 Running、PID `55500`、退出码 `0`。原观察脚本因初始陈旧 Stopped 样本提前退出，报告仍保留 `failed_preserved`；其最终服务快照及后续连续只读观测均证明 Host 稳定运行。Web 首次 Start 尚未执行，因此继续同一授权中未消费的 Web 动作，没有重复启动 Host。

`native-web-first-start.json` 退出 `0`、`host_web_running_unclaimed_development_only`，记录 Web 唯一 Start receipt 与 `Stopped → Start Pending → Running`，最终 PID `49552`、退出码 `0`。Host 保持原 PID，Brain/updater 均 Stopped。启动步骤未写配置、ACL 或程序；四配置十值与原 ACL、其他 keys/程序继续核对，旧失败报告、两套备份和空 staging 保留。

独立请求 `/` 为 HTML 200，`/api/v1/bootstrap/status` 返回 `claimed=false`、`setup_needed=true`；真实浏览器打开 `http://127.0.0.1:18765` 并看到本机证明初始化界面，截图为 `target/installed-file-worker-evidence/native-initialization-18765.png`，无口令/proof/密钥。只关闭端口与 Host/Web 初始运行问题，未执行认领、模型配置或 Brain 启动。服务注销/冷启动、真实 provider/worker、升级/卸载、正式签名和完整 IP01–IP24 仍未通过。详细脚本摘要和观察器限制见[恢复计划 §7–8](installed-product-windows-port-recovery-plan.md#7-已批准的纯启动修正)。
