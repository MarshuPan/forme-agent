# 原件采样失败与恢复数据持有期检查（2026-11-05）

本报告分别记录真实本机服务窗口和源码 guard；二者不能互相替代。整体目标、真实历史 collector、管理 reader 修复、旧脑接管、真实 provider 与 IP 联合验收仍开放。

## 1. 实际原件窗口

owner 已批准原 journal 采样和限定服务窗口。原脚本停服前错误要求历史 updater ExitCode 为零，未进入窗口；修正版只保留该历史值，没有扩大服务额度。旧失败脚本、日志和 56/56 两版 PowerShell guard 证据全部保留。

此前系统提权取消，没有管理员子进程。owner 回复“没有看到系统提示”后，20:54:24 +08 重新发起一次合并的预检/窗口，获得管理员进程 PID 55456；未再索取采样授权。它在 20:54:29 依次请求 Web stop、Host stop，然后停在 `offline-original-observation`：没有得到可接受的离线快照。原进程已结束，不是仍在等待 UAC 或采样。

真实结果为 `target/legacy-release-review/native-offline-source-window-v2-20261105.json`，SHA-256 `0100e8cffcb9e26a5ba9ab06d151dc509fe1967f4cfef7d5a38770f0a54030f6`；原日志 SHA-256 `ddcd914b9e3e5d585eb4e68aaf92862f858385ec4671b23778327cf1169764ad`。恰好两个 stop、零 start、零产品写入/模型调用/脑库打开/凭据读取；未改程序、配置、ACL 或原升级。不能把原件检查未返回快照写成 journal 已验证。

20:54 窗口后的 SCM 为四服务 Stopped / PID 0；当时 Host/Web ExitCode 0，updater 为原 ExitCode 1。网页因此不可达。该窗口失败后没有启动旧 writer，也未重跑原升级。只读系统事件中同期的 Application Error 来自另一个 Flutter 测试进程，与本工具无关，不作为 ForMe 失败原因。随后另获批准的诊断与恢复结果见 §2，不能将本段的 Host ExitCode 0 当成当前状态。

原封存 inspector 对不存在的本地临时 manifest 的独立进程探测正常返回 exit 1 和固定的材料不可用错误；这仅排除了该进程入口完全无法运行，不解释真实原件为何拒绝。准备的安全诊断会保留原工具退出码、输出长度及静态错误白名单；原脚本只留下 RuntimeException，不能从中推断具体根因。

只读诊断和原 Host/Web 恢复方案为 ignored 的 `target/legacy-release-review/native-offline-diagnostic-restore-plan-20261105.md`。原脚本摘要 `221bb978e691b1621189cf0f35391ef1e3f3aee69d64f58538966827dfa61368`，PowerShell 5.1/7 各 19 项入口/动作边界检查通过。owner 随后明确批准“只读诊断并恢复原 Host/Web”；该批准不是升级、修复包切换或历史审查通过。

## 2. 已批准诊断与原服务恢复的实际结果

第一版管理员诊断于 21:19:16 +08 执行，完成固定输入检查后在读取诊断中抛出 RuntimeException，零服务动作。结果 SHA-256 为 `dfa2fa3999ec76a804f2bb5c9d2442bdfabca52335516c1457bbd7f71d269e52`，日志为 `39558e8b878fda9e85f1d4ef48f65baf12ff81f06ccf2336db7a2784f4718290`。原件和失败证据全部保留。

尚未请求启动服务时，V2 只修正诊断结果的保留，增加有界整数/布尔/封闭枚举，不保存未知正文或将校验失败算成 Verified。V2 摘要 `3bc3784a6c54f3e0ccaaa19e4930e29d86d5d653109b7c4481b2e8ef8916fc38`；两版 PowerShell 各 19 项边界检查和真实子进程的缺 manifest 反例通过，在原批准额度内于 21:23:38 +08 执行。

实际离线 inspector **exit 0**，stdout 1,279 字符，stderr 0。返回 schema 1、journal seq 28 / 14,777 bytes / schema floor 1、config revision 1、`Validated / pending Quiescing`，三个 image claim/checking/baseline 标志均为 false。拒绝发生在外层 PowerShell 的 `Snapshot outside approved observation window` 校验；这与 Rust 读原 journal 失败不同。尚不能在缺少逐项绑定比对时指定哪一个条件失败，也不能据此判断 journal 损坏或历史审查通过。

诊断前后原 journal SHA-256 均为 `fdccb6963906461cc82c324fe690f080cfead7ba5d2c53ab5a9704a12c0866d9`，ACL 摘要为 `cc7e86bd23370a1e008724e7eb53b0ed748f480f51d3049178867654e7b6a05f`；六程序、六配置、ACL、四服务注册及空 worker 校验保持后，脚本于 **21:23:42.998 +08** 请求原 Host start 一次。Host 立即以 ExitCode 1 退出，未请求 Web start，未启动 Brain/updater，未重试原 Upgrade。

这里的一次请求不等于一次进程启动：Windows 原有有限恢复策略又在 21:23:48、21:24:18、21:26:18 自动启动 Host，全部失败。SCM 记录 7031 的 5/30/120 秒恢复动作，第四次为 7034，没有自动更改恢复策略或继续重试。21:27 的只读核对为四服务 Stopped / PID 0，Host/updater ExitCode 1，Web/Brain ExitCode 0；网页不可达。SCM 的“函数不正确”只是服务 ExitCode 1 的显示，不足以定位内部错误。

| 原生证据 | SHA-256 |
|---|---|
| `native-offline-diagnostic-restore-v2-20261105.json` | `97d633a5cf6bc56694c4c45ae50aa3808cbe606e9f1e0ea0eb6c5e7a2c852330` |
| `native-offline-diagnostic-restore-v2-approved-launch-20261105.log` | `34bab1bcbdfde5361fce0c6fdb77c08bfba5c9f4c315d2860478d3f466a3e351` |

本次没有替换程序/配置/ACL，没有升级或迁移数据；正常服务启动可能写诊断日志。Host 的受保护 `logs/host/startup-failure.json` 在非管理员进程下不可读，故后续独立只读分析不包含任何服务动作。第一次系统提权未产生管理员进程或结果文件，不能计为已诊断或误称仍在等待同一 UAC。

### 2.1 原件采样误拒已经定位

回查原正式档案发现，采样脚本把 owner ID 写死成了 `owner:shu`，与实际安装的系统生成 owner ID 不同。这是本轮诊断脚本错误；不应让用户重新认领或修改原数据来满足错误预期。原 intent/result 摘要重新核对与历史记录一致，后续校验从这些固定回执取得 owner，而不从登录名猜测身份。

修正后的纯只读脚本摘要为 `637dbd83762b37051e18702e7f39c0070d714fae0824dabfa02ea9d7be270e81`。owner 回复继续后，21:36:14 +08 取得管理员进程 6096，21:36:18 开始现场诊断，实际完成并退出。inspector 再次 exit 0，**14 项状态/时钟/水位/摘要检查与五项身份绑定全部通过**，原 journal、程序、配置、ACL、服务注册和 worker 保持；零服务动作、零产品写入、零凭据读取或模型调用。安全结果 `native-readonly-failure-analysis-20261105.json` 摘要为 `26c9ff3e9cc22f7369529ba759713cd99410f75db91f4ee1d3d96ce6773e06cf`。这闭合本次只读原件观察，未补出历史恢复基线、八类证据或恢复授权。

### 2.2 旧 Host 的另一个启动阻断

Host 错误文件实际修改时间为 **2026-10-27 05:02:32 UTC**；其中 `management_endpoints` 不是本次失败记录，不可用于认定当前端口故障。当前 `18765–18769` 不在只读观察到的 Windows TCP 排除区间中，也未观察到监听者；不能拿 10 月端口冲突充当新诊断。

已安装 `694b6bb` 的源码在加载配置前严格比较 SCM 命令中的路径写法；当前 Host 登记仍为 11 月 2 日获批修复后的 `bin/forme-hostd.exe`。按旧代码和标准反斜杠 image 派生的完整命令不匹配，统一分隔符后才匹配。同机无依赖 Rust 观察程序通过混合分隔符启动，实测 `std::env::current_exe()` 返回反斜杠。当前源码已包含 `11dffc1` 的等价分隔符修复，但已安装程序没有更新。由此确认旧 Host 的启动绑定谓词存在确定阻断；不把这项推导当作修复后已经启动成功，也不排除后续阶段暴露其他错误。

已准备[仅恢复原 Host 路径表示与限定启动计划](installed-original-host-path-restore-20261105-plan.md)，不更换程序、配置或数据，不裁决原 Upgrade。PowerShell 5.1/7 各 29 项状态/动作/备份 ACL 边界检查通过。后续管理员只读预检未取得管理员进程，结果文件不存在，不计通过；真正的 `-Apply` 尚未执行。下次按精确计划确认后在同一 UAC 中先检查再变更，不再重复拆分预检提权。

## 3. 源码持有期检查

契约细化见[历史审查方案 §9.6](../architecture/proposals/installed-legacy-release-recovery-review.md#96-独占数据-worker-的持有期检查)，文档提交 `f58cb92`。真实维护子进程返回的 identity 只是初始快照；子进程随后退出时，不能继续据此宣称数据锁仍被持有。

新增检查核对同一个 child handle、仍然存活的 IO supervisor、未关闭的通道和原截止时间；进程 exit 0 也失效。已 Release、Backup 或耗尽期限不能继续作为只读 guard。失败回收本 child，不发送 Backup/Release、不再派发 worker、不延长期限。普通有基线恢复在初始观察和最终 Committed 前均检查，最终检查位于文件/服务重查之后；已落 Intent 仍只允许原号查询和显式 verify。

实现提交为 `48211dc`。新增定向场景为真实子进程正常退出、重复只读检查/过期，以及真实临时 journal 中 Intent 前后丢失 guard。定向 transport 6/6、release staging 36/36 全部通过；SCM/数据身份在后者仍为可信组件注入，不计入原生 IP。

当前源码完整质量门 exit 0：fmt、质量棘轮、locked workspace check、全部 workspace binaries 构建、locked workspace tests（**182 targets，1233 passed / 0 failed / 15 默认 ignored**）、strict Clippy、完整默认合规及 diff check 均 PASS。测试数按本轮实际输出解析，不累计定向重复执行；ignored 不计通过。质量门检查 856 个文件，保留 63 项现有规模债务。没有因模块修复重跑无变化的整轮 M5，旧 M5/发布回执不覆盖本提交；最终产品收口仍需相应当前源码门。

PowerShell transcript 仅记录步骤标签，因此另将这次已捕获的完整 native stdout/stderr 原样保存在 ignored 的 `target/legacy-release-review/data-custody-gate-native-output-20261105.log`，SHA-256 `7d70da6f7677b5f73bc3df5f13682da71308a6788682ca6af78fe0dd05a15c75`；原 transcript `data-custody-gate-20261105.log` 摘要 `3bfb977691353f1b66ac874f2bb6e7cb79ee736552b09d443a15b2109bc651bd`。这保留真实命令输出，不是以重新生成的测试回执替代原执行。

本改动不挂载完整历史 collector，不证明八类历史材料或当前数据已完整重新核验，不改变产品 DTO、脑事件、crate 依赖或 unsafe 边界。

本轮资源报告保存于 `target/legacy-release-review/data-custody-build-resources-20261105.json`，摘要 `656b18e9ee8e73f73ab4db095d01a264b72ef9752ff0f4a1e025dbbf81055af8`。固定 target 缓存 44,567,960,658 logical / 41,685,429,908 allocated bytes，低于 64 GiB 阈值，`over_budget=[]`、`errors=[]`；阶段前可用空间 66.1 GiB。没有清理缓存、安装包、原件或验收证据。
