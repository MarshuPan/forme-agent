# 独立部署与 Web 产品化实施记录

本记录承接 requirements/17–18、architecture/10、prd/24–26。它只记录新的产品化施工与证据，不改写 M0–M5、C1–C24 的历史验收；宿主组件测试不等于 IP01–IP24 或安装包验收。

2026-10-26 `2702d30` 完成[受管模型代理配置](installed-product-model-proxy-acceptance.md)：Host schema 8、CAS/journal/revision、release binding 保留、core materialization、ProductionRuntime provider 传递和 direct/HTTP transport 绑定已闭合；直接模式禁用环境代理，显式代理禁止 `NO_PROXY` 绕路，失败不自动直连，公共 Web view 不返回 endpoint。workspace 全量测试、fmt、check、strict Clippy、代码质量、资源与完整合规门通过。该条是模型代理组件证据，不提升真实 Provider、系统服务、安装包或 IP01–IP24 状态。

2026-10-26 在当前 `e49ac89` 工作树显式运行真实 Chrome golden：`s38_real_browser_golden_runs_harness_approval_and_observes_server_mutation` 与 `s68_m3_candidate_live_v2_regression_rollback_and_live_v1_are_governed` 共 2/2 PASS。浏览器由真实 Chrome 进程启动，覆盖审批前零 mutation、审批后单次受治理 mutation、候选/活动版本边界与回滚证据；临时数据和浏览器进程清理完成。该证据属于核心 Harness/M2-M3，使用 fixture provider，不提升真实外部 Provider、安装服务或 IP01–IP24。

2026-10-26 `a5b6a62` 完成并提交 Web 会话可见性竞态修复，新增 7 个前端竞态测试，并用可选真实浏览器夹具验证 Web → Gateway → ProductionRuntime：临时 owner 登录、一条 `workspace:lifecycle` 对话、19 条运行事件、1 次计数型本地模型调用、0 次工具动作、刷新恢复且无重复运行，临时数据清理通过。详见 [Web 运行时链路验收](installed-product-web-runtime-acceptance-20261026.md)。该证据不提升真实 Provider、系统服务、安装包、隔离或 IP01–IP24 状态。

2026-10-26 基于实现提交 `5391750` 更新 [Windows x86_64 原生候选](installed-product-windows-candidate-20261026.md)：release workspace 构建、17-file bundle、固定 SHA-256 的 Inno 6.6.1 编译与独立摘要核对均通过；安装器为 65,564,782 bytes、SHA-256 `3acfe724a4c9f3a82faf687c16660f243eae1fec39e23a383deb97ac8068b957`。它仍是 unsigned/uninstalled/development、`release_ready=false`，没有运行安装器或注册服务，不提升任何 IP 状态。当前源码的 M5 回归与发布回执状态另见 [M5 报告 §10](m5-acceptance-report.md#10-产品化分支的永久回归重验)。

2026-10-26 `5391750` 修复 Web 320/390 窄屏溢出、顶栏挤压与导航焦点，新增默认 ignored 的独立临时浏览器夹具；[浏览器组件验收](installed-product-browser-component-acceptance-20261026.md)记录真实内嵌 `forme-webd` 的键盘导航、core 离线维护、两标签页注销与刷新后的认证清除。前端 81/81、workspace check/strict Clippy、工作台 8 项组件门、代码质量和合规通过，724 文件/63 处既有规模债务未增长。夹具为预认领临时账号，未连接 Host/core/真实 provider；本次不提升任何 IP01–IP24 状态，完整安装、认领、业务并发与平台联合验收继续开放。

2026-10-26 在 `04cbb03` 当前树运行 `pwsh -NoProfile -File tools/verify-code-quality.ps1`：完整源码质量门退出码 0，耗时 601.14 秒；结构/缓存安全/字节一致性回归 21 项、723 文件规模棘轮（63 项历史超限债务未增长）、Rust fmt/check/test/strict Clippy、前端依赖审计与 81 项验证、静态 bundle、原生打包/签名、Windows NTFS 权限与只读防火墙、原创合规及 diff 检查均通过。资源报告提示 `target` 为 90.54 GiB logical / 87.76 GiB allocated，超过缓存预算；门仍以 269 GiB 可用空间完成，未清理构建产物。此为源码门，不证明原生服务安装、真实 provider、真实浏览器完整 claim 或 IP01–IP24。

2026-10-26 在 `5019498` 当前代码上显式运行 `cargo test --locked -p forme-execution --test installed_windows_appcontainer_contract -- --ignored --nocapture`：真实 Low AppContainer 的私有副本/Host 核验回写、失败/取消/冲突、宿主崩溃恢复共 3/3 通过，外网 TCP 正向对照可达且 sandbox 内网络拒绝断言通过。fixture 使用临时目录并在恢复核验后清理。本机没有注册服务且当前身份非管理员，因此该运行不证明 SCM/LocalSystem 装配、安装包或 IP10/IP24；ProductionRuntime 结果通道仍未接入。

2026-10-26 在 `e46fe55` 上运行 `pwsh -NoProfile -File tools/verify-installed-host.ps1`：宿主永久合同门完整 PASS，覆盖安装/升级与管理 journal、worker admission/取消/receipt、Windows AppContainer 与 overlay lease、服务注册边界、恢复和受控停服。该门是临时 fixture/进程的组件证据；脚本明确不安装系统服务，也不认证原生包，因此 IP01–IP24 仍为 `NOT_RUN`。本机未注册 `forme-*` 服务且当前 token 非管理员；没有进行系统级变更。

2026-10-26 对当前源码 `5676dc1eeaea8874352c71d3b577d766f25b2350` 做只读复验：`npm --prefix frontend run verify` 的 TypeScript、81 项前端行为测试、生产构建及静态 bundle 检查通过；`verify-web-workbench.ps1` 8 项、`verify-web-identity.ps1` 25 项、`verify-web-ingress.ps1` 10 项通过；`verify-product-configuration.ps1` 的配置/激活 62 项、网络 fence 4 项、脑偏好 3 项、managed skills 2 项通过，产品二进制构建通过。受管 Host/Web/core 多进程桥因本机未注册 `NT SERVICE\forme-brain` 为 `NOT_RUN`；原生服务、安装包、真实 provider、浏览器完整 claim 后链路及 IP01–IP24 仍未通过。本轮静态对照另确认正式 worker driver 仍缺可取回的有界 ActionResult 工件契约：现行 transport 仅有 dispatch/probe/cancel，结果引用没有对应读取路径；需先按架构变更流程确认 PRD24/architecture/10 文档方案，再实现。该复验未修改代码，不提升任何产品 IP 状态。

2026-10-26 `89bec64` 完成 Windows 桌面 helper dispatch 的受治理 Host 边界：桌面 grant/capability/plan digest 精确绑定，拒绝过期 grant 与能力不匹配，helper 在动作开始后失联则保留 Unknown 且不重试；`installed_worker_contract` 11/11 通过。`68cca5a` 将三个 desktop-helper 场景加入并通过宿主永久门。当前生产仍使用 `UnavailableDesktopHelper`，ProductionRuntime 尚未接入正式 worker driver，也没有真实登录会话 helper、SCM/LocalSystem 安装运行证据或 IP01–IP24 通过；详见[桌面 helper dispatch 组件验收](installed-product-desktop-helper-dispatch-acceptance-20261026.md)。本轮完整 M5 场景/质量门均通过；首次 `verify-m5.ps1` 仅因当时工作树未提交而在 S99 退出，随后提交后单独重验当前树 M5 release audit 通过，最终当前树回执比较见 M5 验收报告 §10。

2026-10-26 `ece53be` 完成 Harness gateway control 实现的职责拆分，见[可维护性实施记录](installed-product-maintainability-progress.md#9-gateway-控制实现拆分2026-10-26)。本次完整 `verify-m5.ps1` 在当前提交运行了全部 S1–S99、C1–C24、workspace、strict Clippy、89 项 Python 门和合规检查；临时 S99 审计及独立验证通过，最终只因仓库回执绑定旧源码而在回执比较退出。该次不记作完整 M5 PASS；刷新后的当前树回执及复跑结果在 [M5 报告 §10](m5-acceptance-report.md#10-产品化分支的永久回归重验)。该拆分不改变生产 worker 可用性、原生平台状态或 IP01–IP24。

2026-10-25 完成[产品化验收脚本与 fixture 边界校准]：配置门按当前源码执行 62 个配置/激活 Rust 场景、4 个网络场景、3 个脑偏好场景和 2 个 managed skills 场景，均 PASS；Web identity 25/25、Web ingress 10/10、认知纠正组件门 PASS。当前 Windows 未注册 `NT SERVICE\\forme-brain`，因此 `verify-product-configuration.ps1` 与 `verify-workbench-correction.ps1` 将受管 Host/Web/core 进程桥、安装 daemon 重启和原生服务/IP01–IP24 明确记为 `NOT_RUN`，不以组件测试或交互 Host 替代真实 SCM 身份。Windows fixture 的 brain image 已改为读取安装清单中的 `manifest.paths.releases/bin` 后创建，避免错误路径造成假阴性。该批只修正验收边界与 fixture，不改变生产隔离语义；原生安装、真实 provider 和完整 IP01–IP24 仍未关闭。

2026-10-25 `558c7e7` 完成 [macOS LaunchDaemon 受管 worker adapter](installed-product-macos-launchd-worker-acceptance-20261025.md)：受管 Host 仅在 root、固定 `com.forme.host` LaunchDaemon、`launchctl`、`sandbox-exec` 和 `_formeexec` 执行账户均可验证时加载 macOS adapter；每个 job 使用独立 LaunchDaemon、sandbox、workspace/tmp 副本，worker 只能写副本，Host 按原始树身份/内容、最终执行规格、变更范围和 durable intent/receipt 核验后回写，冲突、回写失败、控制命令超时和状态不明均保持 `Unknown`，不盲目重试。Windows 主机上的 execution crate 测试、fmt、strict Clippy、代码质量和合规门通过；macOS target 未安装且没有 macOS 原生运行证据，因此不提升 macOS 原生服务、IP10/IP24 或 IP01–IP24 状态。

2026-10-25 `f2560da` 完成 [Linux systemd 受管 worker adapter](installed-product-linux-systemd-worker-acceptance-20261025.md)：受管 Host 仅在 root、`forme-host.service`、固定 `systemd-run/systemctl` 和 `forme-exec` 执行账户均可验证时加载 Linux adapter；每个 job 使用独立 workspace 副本，worker 只能写副本，Host 按原始树身份/内容、最终执行规格、变更范围和 durable intent/receipt 核验后回写，冲突、回写失败、控制命令超时和状态不明均保持 `Unknown`，不盲目重试。Linux 目标交叉 `cargo check`、workspace 全量 `cargo test --workspace --all-targets`、fmt、strict Clippy、代码质量和合规门通过；当前 Windows 主机未运行真实 root/systemd/`forme-exec`/安装服务，因此不提升 Linux 原生服务、IP10/IP24 或 IP01–IP24 状态，macOS 原生验收仍未完成。

2026-10-26 `beba3e6` 补齐 Linux `.deb`/`.rpm` 安装钩子的 `forme-exec` 专用系统组/账户创建，并在 POSIX 打包测试中锁定两种包格式的账户参数；这修复了真实安装后 Linux worker 因执行账户不存在而无法准入的静态缺口。`node --test` 打包套件 27/27、workspace check/test、fmt、strict Clippy、代码质量和合规门通过；当前仍没有 Linux 原生 root/systemd、账户、安装包或回写运行证据，因此不提升 Linux IP10/IP24 或 IP01–IP24 状态。

2026-10-25 `0f6829a` 完成 [Windows managed worker composition 接入](installed-product-windows-managed-worker-composition-acceptance-20261025.md)：只有固定 SCM 入口传入的受管 Windows Host 才能加载 worker 配置并组合每 job AppContainer、Job Object 与私有 workspace overlay；打开隔离状态前额外核验当前进程确为 `LocalSystem`。前台/开发入口继续保持原行为，缺少 worker 配置、SCM 绑定或 LocalSystem 身份时 fail closed。workspace fmt/check/test、strict Clippy、代码质量与合规门通过；本机没有已注册 forme SCM 服务且当前用户不是 LocalSystem，因此没有真实服务身份/安装/IP10/IP24 证据，Linux/macOS 路径未因此关闭。

2026-10-25 `b901dff` 完成 [Windows managed Host fail-closed 回归](installed-product-windows-agent-execution-peer-transport-acceptance-20261025.md)：在没有真实 `NT SERVICE\\forme-brain` 服务身份时，受管 Host 必须在打开 listener 前失败退出，不能回退 TCP，manifest 替换也不能绕过失败或创建管理 journal。`forme-execution` 134/134 非忽略测试通过；`verify-installed-host.ps1` 宿主永久门 PASS，IP01–IP24 仍明确为 NOT_RUN。

2026-10-25 `2f5f6a9` 完成 [Windows AgentExecution peer transport](installed-product-windows-agent-execution-peer-transport-acceptance-20261025.md)：managed Host 的 AgentExecution 从 loopback bearer-only TCP 切换到安装绑定的 named pipe，校验 Brain Service SID、当前 active release image 和应用层 bearer；开发模式仍保留 TCP 协议测试路径。该组件不等于真实 SCM 服务、完整 worker sandbox 或 IP01–IP24 验收。

2026-10-25 `85b59f5` 完成 [SCM Host 当前进程绑定](installed-product-scm-browser-hardening-20261025.md)：受管 Host 在加载凭据/开放 listener 前核对固定 SCM 注册；上报 Running 后要求 OWN_PROCESS、Running 与当前 PID 精确一致，并在两次 PID 观察之间复核注册。永久宿主门已扩展并通过 163/163 命名场景；该证据仍不等于 LocalSystem token、core peer、管理员安装或 IP10/IP24。

2026-10-25 `a8ac11b` 完成 [真实浏览器 fixture framing 修复](installed-product-scm-browser-hardening-20261025.md)：完整请求头、有界读取、idle preconnection 隔离和不完整请求零 mutation。真实 S38、S68 均通过，真实 Windows AppContainer/overlay 3/3 通过；M5 总门此前仅因 NOTICE 缺少 `windows-sys 0.61.2` 被 release audit 阻断，已补齐，当前树独立 release audit 已 PASS（仓库回执随后写入）。

2026-10-25 `b0f5cfb` 完成 owner 选择的 [Windows 私有副本/overlay 与 Host 核验回写](installed-product-windows-private-workspace-acceptance-20261025.md)：原 workspace 不降低完整性或授予 worker，精确计划绑定、句柄 custody、整批冲突拒绝、durable intent/receipt、Unknown 保留与启动恢复已实现。最新源码质量门为 171 suites、1018 Rust passed / 0 failed / 9 默认 ignored，前端 77/77、strict Clippy/完整合规 PASS；宿主永久门 161 个命名场景和显式真实 AppContainer 3/3 PASS。三个场景使用临时数据与当前交互 Host，不代替 SCM 服务身份、真实 Brain/Web/keys ACL、会话 helper 或原生安装验收；生产仍为 `UnavailableWorkerIsolation`，IP10/IP24 未通过。此前 `d0fb681` 的 A 待实施描述是当时状态，由本条更新；完整 M5/current-tree release receipt 继续重验，不沿用旧回执。

2026-10-25 `d0fb681` 完成[Windows AppContainer adapter 组件](installed-product-windows-appcontainer-adapter-acceptance-20261025.md)：owner 已批准的 Win32 边界见 `2a0c289`；限定两个 FFI 文件、per-job identity、创建时 Job custody、token 检查、真实 DACL grant/revoke、原 lease 恢复与准入阻断已实现。专项 10 passed / 0 failed / 1 ignored，installed-host 147 个命名场景及统一源码质量门 PASS；M5 长链在 S68 浏览器 fixture 失败，同源码单独复跑 S68 通过，长链不记 PASS。该条是 overlay 实现前的历史状态，已由 `b0f5cfb` 的每 job 私有副本/Host 核验回写实现取代；原目录完整性标签不降低。生产仍使用 `UnavailableWorkerIsolation`，IP10/IP24 未通过。

2026-10-25 `277f551` 完成[原生包事务入口加固](installed-product-package-hook-hardening-20261025.md)：Linux/macOS 包 hook 固定管理员环境、`umask` 与 root/systemd 前置检查，卸载 fixture 显式模拟 root 后 POSIX 8/8 场景通过；workspace 与质量/合规门通过。该批不执行真实包管理器、systemd/launchd、管理员安装或签名，不提升 IP01–IP24，worker/AppContainer 与原生平台验收仍开放。

2026-10-25 `8936975` 完成[Windows 固定服务 Service SID 边界](installed-product-service-sid-acceptance.md)：四个固定 forme 服务在注册、既有配置复核和 updater 接管前均强制核验 `ServiceSidType::Unrestricted`，未知服务名与 SID 漂移 fail closed。workspace fmt/check/test、strict Clippy、代码质量与合规门通过；当前主机非管理员且未注册 SCM 服务，本组件不提升原生安装、worker 隔离或 IP01–IP24 状态。

2026-10-25 `82493ea` 完成[Windows sandbox lease journal 持久化与恢复底座](installed-product-windows-sandbox-lease-acceptance-20261025.md)：固定路径、有界 append-only/hash-chain、单 writer、逐步 intent/readback、启动恢复阻断，以及活动 lease 的 job 目录路径/file identity 唯一性。7 个 lease 专项场景、installed-host 永久门、workspace fmt/check/test/strict Clippy、代码质量与合规门通过。此组件不执行真实 ACL/AppContainer 操作、不提供 token 或文件访问正反断言，也未接入生产 dispatch；worker 仍 blocked，IP01–IP24（含 IP10/IP24）未通过。早期 M5 S99 clean-tree release audit 因其他未提交改动而报 `RELEASE_TREE_DIRTY`，不记作 release PASS。

2026-10-25 `639b2b4` 完成[发布离线数据与服务 image 组件](installed-product-release-data-and-images-20261025.md)：真实 SQLite schema/独占锁/一致性备份、有界私有 worker、签名 staging 重查、历史 journal 原字节校验，以及三服务完整预检和持久 one-shot image claim。最终统一门 965 Rust passed / 0 failed / 5 默认 ignored，前端 75/75、strict Clippy/合规及宿主 122 个命名场景 PASS。独立 updater 与新 Host Checking 接管仍未闭合，生产 Upgrade/Rollback 保持拒绝；原生/IP 验收未提升。下表以该记录及已有网络/恢复增量校准，不将组件状态写成整个模块完成。

2026-10-25 `cce9ea6` 完成[独立发布 updater 与 live readiness](installed-product-release-updater-acceptance.md)：固定按需 `forme-release-updater`、原操作 takeover、三服务有界切换、Checking/Committed/RecoveryRequired 恢复、真实 loopback nonce 签名 proof 与 admission gate 已实现。宿主永久门、workspace check/test、strict Clippy、代码质量和合规门通过；这仍是 Windows 控制面与受控 live socket 证据，不是管理员 SCM 注册、签名安装包、Linux/macOS 原生服务或 IP01–IP24 证据。完整产品里程碑继续保持未关闭。

2026-10-25 `a6e9a7c` 修复[设置可用性 fail-closed 展示](installed-product-settings-availability-readonly-acceptance.md)：旧安装缺少受管网络路径时，网络字段只读且不显示网络应用操作；模型/技能等已接通设置仍可按原事务保存。前端 76/76、workspace fmt/check/test（5 项按既定外部前置条件 ignored）、strict Clippy、代码质量与合规门通过。没有执行真实浏览器、管理员防火墙或 IP09/IP24 产品验收；PRD25/26 与整体产品里程碑仍开放。

2026-10-25 Windows worker 隔离边界审查：PRD24 §4.1.2 已明确 `forme-hostd` 是安装清单绑定的 LocalSystem broker；core peer 身份与每个 worker 的 AppContainer/低完整性 token、Job Object、有效文件权限分别取证。静态审查的现有候选没有同时满足挂起后先入 Job、精确且身份绑定的 ACL、可崩溃恢复的 ACL lease，以及真实 token/文件访问正反断言；未新增依赖或启用 adapter。当前进程非管理员，`forme-host`/`forme-web`/`forme-brain` 均未注册，因此没有原生 SCM 身份或 worker 权限验收证据。生产 Host 继续使用 `UnavailableWorkerIsolation`，普通 worker dispatch 保持 blocked，IP10 未通过。

2026-10-25 [Windows worker sandbox 候选审查](installed-product-windows-sandbox-candidate-review-20261025.md)：现有 `win-custody 0.1.1` 只能作为 Job/ACL 观测底座；`landstrip 0.20.9`、`rappct 0.13.3`、`sandboxrs-windows 0.1.1` 与 `skarn-sandbox 1.0.1` 均不能直接接入生产 adapter。候选缺少 forme 要求的确定性 job identity、durable ACL intent/revoke/readback、或首条指令前的可审计 Job/token 接缝；未新增依赖或启用 adapter。生产 dispatch 继续 fail closed，IP10/IP24 不变。

2026-10-24 `5cbe175` 完成 Windows `WindowsJobWorkerIsolation`：使用 Job Object 在 worker 首次执行前接管进程，启用 kill-on-close，取消和宿主退出可终止同一进程树；真实 Windows 合同测试覆盖成功/失败退出、后代进程树终止和取消幂等，2/2 PASS。该提交只闭合 Windows 进程树生命周期 custody，不等于完整 sandbox；服务账户、ACL、逐 job 文件/网络/credential scope、会话 helper、原生安装和 IP10/IP24 仍开放。详见[Windows Job Object worker adapter 验收](installed-product-windows-job-object-acceptance.md)。

2026-10-24 `780ec1e` 完成 Core→AgentExecution 的 typed driver 接缝：`WorkerExecutionSpec`、loopback-only `WorkerBrokerTransport`、job 专属 execution binding、精确 job/permit/plan 校验、Unknown/原 job probe-cancel 与进程入口传递已实现。协议专项 3/3、host transport 8/8、workspace check、strict Clippy、代码质量与合规门通过；workspace 全量 test 在 Windows 链接阶段因 C 盘空间不足（`No space left on device`）未完成，不能写成完整 workspace test PASS。该提交只闭合 wire/receipt/状态边界，不证明 ProductionRuntime 已接入真实平台隔离；普通 process worker、Windows/Linux/macOS 原生 sandbox、IP10/IP24、安装包和系统服务仍未完成。

当前产品主施工分支：`shu/installed-product`；受管 skills、公共设置 v2、网络健康与回退增量已合入本分支，尚未合入 `main` 或发布。产品分支主线基线为 `802e3d2`（原子文字纠正、失效/低置信候选与无正文回执）；前笔 `91072be` 为具体记忆来源的真实遗忘，`700bb6f` 为模型角色装配绑定，`0990bd0` 为默认工作空间与模型角色持久配置。其余历史组件按下方记录保留。验证日期 2026-10-24。本机 shell 非管理员、未注册 forme 系统服务；未读取真实 owner/model secret。

2026-10-24 `e405bd8`、`ba7f51a`、`4963169` 完成[受管 Web 网络配置组件](installed-product-network-configuration-acceptance.md)的后续公共设置 v2、listener 健康与失败回退：desired/published 分离、材料绑定、先回执后单次 dispatch、全局维护 fence、旧日志原字节兼容及独立发布权限；Windows 只读核验管理员预配 firewall，不自动修改规则。配置专项与 Web 工作台真实进程门 PASS；原生管理员、证书客户端信任、跨设备及 IP09/IP21/IP24 仍未通过。

2026-10-24 `8d9944f` 完成[受管 skills 配置与运行时接入组件](installed-product-managed-skills-acceptance.md)：skills 与模型/brain preference 共用同一配置 revision、operation、journal、CAS 和恢复语义；Host schema 5 保留旧配置兼容，正式 ProductionRuntime 只向同 scope 的可信启用项开放按需正文。Web 设置页已接入完整保存事务，禁用、不可信、越 scope 和可执行能力声明均 fail closed。全量 workspace Rust tests、配置专项、真实 Web→Host→core 进程桥、前端 verify、strict Clippy、代码质量与合规门通过。该组件不提升 IP08/IP21/IP24；MCP/plugin/package、消息渠道、真实 provider、原生服务与完整产品联合验收仍开放。

## 1. 实施顺序与当前范围

2026-10-24 `802e3d2` 完成[原子文字纠正组件](installed-product-cognition-correction-acceptance.md)：来源密文、原认知失效、新候选与回执在 SQLite 多 stream CAS 中一并提交；Unknown 只查询原命令，真实删除反馈后旧错误不复活。统一门 163 suites、878 Rust passed / 0 failed / 5 默认 ignored，前端 64/64、strict Clippy/完整合规、冻结图/99-kind 与纠正专项通过；607 文件、63 处既有规模债务未增长。真实临时浏览器验证纠正/刷新/原号核实和安全文本，修复长引用溢出；窄屏/双 tab/多 Web、真实 provider、原生安装及完整 IP20/IP24 仍开放。完整 M5 继续重验，整体任务未结束。

2026-10-24 前批 `91072be` 将具体记忆的可信材料范围接到原 PrepareLifecycle/Apply：只读选择、版本绑定、显式预览/批准和真实清理均经正式主脑。统一门 161 suites、871 Rust passed / 0 failed / 5 默认 ignored，前端 58/58、strict Clippy、冻结图/99-kind、NTFS 与完整合规通过；593 源文件/63 既有规模债务未增长。见[具体记忆到数据生命周期组件验收](installed-product-memory-lifecycle-acceptance.md)。当时文字纠正尚未实现，后由上方 `802e3d2` 补齐；原生安装与完整 IP20/IP24 仍开放。

2026-10-24 `700bb6f` 完成显式模型角色/注册选择语义到正式 composition digest 的绑定，保留无角色的历史算法及旧事件。完整统一门 161 suites、870 Rust passed / 0 failed / 5 默认 ignored，前端 54/54、strict Clippy、NTFS 与完整默认合规通过；冻结门及默认审批恢复否定专项通过。组件边界见[模型角色装配绑定验收](installed-product-model-role-binding-acceptance.md)，无新增原生/浏览器/IP 通过声明。

2026-10-24 `727e473` 完成正式 production composition 的纯职责拆分，原模块 3365→2926 行，独立装配模块 444 行；新增受管 store/重复 writer/外来 store 的否定边界测试。最终统一门 161 suites、868 Rust tests / 0 failed / 5 默认 ignored，前端 54/54、strict Clippy、完整合规及临时 NTFS 门通过，593 文件/63 处既有债务未增长。细节与日志摘要见[可维护性记录 §8](installed-product-maintainability-progress.md)。模型角色摘要的行为修正继续单独实施，完整产品里程碑未关闭。

2026-10-24 `0990bd0` 完成默认 workspace、前台优先与后台相关性模型的 Host→正式 ProductionRuntime 配置链路，保持旧摘要/物化兼容、保存和恢复发布 CAS、Unknown 原操作核实以及离线灾备独立性。最终统一源码门 PASS：161 suites、867 passed / 0 failed / 5 默认 ignored，前端 54/54、strict Clippy、完整合规、592 源文件/63 规模债务未增长；配置专项 26 Host cases + 1 managed runtime + 2 实际进程/权限场景通过，冻结图/事件分类通过。浏览器检查和早期 Unknown 的证据边界见[默认工作空间与模型角色配置组件验收](installed-product-brain-preferences-acceptance.md)。完整能力/渠道/网络设置、原生服务与 IP08/IP21/IP24 仍开放，继续整体里程碑。

2026-10-24 `de3c7b1` 通过统一源码门：160 suites、858 passed / 0 failed / 5 默认 ignored，strict Clippy/完整合规/结构门通过；前端 53/53，记忆来源材料专项 5 Rust cases、Web→core 1 个正式 runtime 场景、artifact 回归 5 cases 与 writer 回归通过。正文入口不接受任意 locator，读取不调用模型、不写脑事件、不创建 key；真实浏览器与 IP 仍未因此关闭。详见[记忆来源材料正文读取组件验收](installed-product-memory-material-acceptance.md)。

2026-10-24 `72db326` 通过统一源码门：158 suites、852 passed / 0 failed / 5 默认 ignored，strict Clippy/完整合规/结构门通过；前端 49/49，配置专项 22 Rust cases 与 2 个真实进程场景通过。真实浏览器验证历史会话、文本安全、终态同步、只读诊断与 core 停止/重连，未增加额外模型调用。本机恢复改用真实 OS 权限，Tokio advisory 的受影响版本已替换；此前完整 M5 的 S99 阻断事实保留，更新后完整总门待重跑。详见[运行诊断与恢复权限组件验收](installed-product-runtime-diagnostics-acceptance.md)。安装/服务隔离/升级回滚及完整 IP 仍开放，继续总里程碑。

2026-10-24 `a53557f` 完成项目监管与工作空间后续调整组件：[组件验收](installed-product-project-supervision-acceptance.md)。权威状态、监管 CAS、与后台 tick 的暂停串行、旧 ledger replay 兼容、工作空间控制与长期意图入口均已实现；统一源码门 Rust 851 passed / 0 failed / 5 默认 ignored，严格 Clippy/前端/合规门通过，8 个命名工作台集成场景通过。该组件不提升 IP19 或 IP24 状态；当前真实浏览器和原生服务仍未验收。

2026-10-24 基于 `a53557f` 重新编译 [Windows x64 原生候选](installed-product-windows-candidate-20261024.md)：17-file development bundle 与 Inno 安装器编译通过，SHA-256/签名/安装状态均有独立记录。候选明确 unsigned、uninstalled、`release_ready=false`；IP01/IP02/IP03/IP10/IP24 仍未通过，本机当前非管理员且没有已注册的 forme 服务。

2026-10-23 `507cccf` 增加普通主动建议处置的 durable recovery：identity schema v9、owner/target/binding 唯一约束、刷新只读原命令、Unknown 保留及显式原号核实；维护 session fence 同步兼容 v9。当前源码 Rust 844 passed / 0 failed / 5 默认 ignored、strict Clippy、40 项前端测试、完整合规和结构门通过；前端依赖占用后的恢复过程及证据边界见[普通建议恢复组件验收](installed-product-proposal-recovery-acceptance.md)。真实浏览器与 IP20/IP24 仍未关闭，继续整体里程碑。

2026-10-23 的后续提交补齐了对话会话产物目录/历史（`29a1790`）、pending run 的 owner 裁决密文保存/原命令核实/重启恢复（`69ee763`），以及会话 workspace 关联到后续正式 run 的主脑上下文绑定（`56dbed1`）。绑定组件已通过 workspace 回归和 Web→core 正式 runtime 集成场景；仍不等于真实浏览器、多进程、系统服务或 IP01–IP24 联合验收。详见[会话 workspace 运行上下文绑定组件验收](installed-product-session-workspace-binding-acceptance.md)。

2026-10-23 提交改密码 mutation 的 key 绑定（`dd76f20`）：独立认证库 schema v8 绑定 operation key、owner/session、account revision/epoch 与 Argon2id 候选；异义复用在 reauth/状态更新前拒绝，候选到期/成功后清除。前端未知结果锁定字段并保留原 key；静态 bundle 已重新生成。最终源码通过 Gateway all-targets check/strict Clippy、前端 verify 33/33、代码质量与 compliance doctor。完整 workspace Rust test 在 C 盘/Windows pagefile 资源不足时无法链接；重构后的最终源码未获得 runtime test 证据，本组件验收仍待该门重跑。详见[改密码操作幂等绑定组件验收](installed-product-password-change-idempotency-acceptance.md)。

2026-10-23 新增[运行挂起恢复与裁决工作台组件验收](installed-product-run-recovery-acceptance.md)：pending 状态只读投影、owner 裁决密文 companion 与事件 claim 原子提交、模型调用未知终止、重启后原 command 显式核实，以及 Web 侧待决/未知说明均已实现。当前源码完整 workspace 回归与统一代码质量门通过；该组件不改变 IP01–IP24 状态，也不关闭整个产品化里程碑。

2026-10-23 新增[恢复候选工作台与已暂存发布认证组件](installed-product-restore-workbench-acceptance.md)：恢复的最终 manifest 安全投影、独立切换确认/原操作核实及 Host 重复候选拒绝已实现；发布只读重新认证同时核对 journal、验签和实际文件。统一源码门 Rust 817 passed / 0 failed / 5 默认 ignored，strict Clippy/完整合规/宿主与配置进程门通过，真实浏览器已核对原生能力缺失时禁用切换。完整 Upgrade/Rollback 和原生产品验收仍开放，整体任务持续推进。

2026-10-23 新增[恢复主脑的执行隔离与 Host 提交组件验收](installed-product-restored-brain-acceptance.md)：SQLite 恢复水位、旧 run/intention/route 权限隔离、原 vault/活动数据 CAS、固定离线 worker、完整 CommitRestore 阶段、Web 在线 session fencing 与 Committed 后工作准入已实现。全量 Rust 813 passed / 0 failed / 5 默认 ignored，strict Clippy/完整合规、宿主/配置统一门及实际进程恢复链路通过；服务阶段使用受控 adapter，未冒充原生 SCM。完整 Web 候选激活、签名 Upgrade/Rollback、隔离和其余整体义务继续实施。

2026-10-23 新增[原生打包入口与 POSIX 权限组件](installed-product-native-packaging-acceptance.md)：原生编译器固定参数/SHA pin/独立快照入口、首次安装权限和精确 bundle→signer 接缝已实现。当前 Rust 802 passed / 0 failed / 5 默认 ignored，打包与真实 Rust 验签 28/28，strict Clippy/完整合规/结构门通过。Windows Inno 已真实编译未签名开发候选，未运行安装；其他原生编译、签名公证、实际平台安装仍未执行。固定源码完整 M5 到 S99 因缺依赖清单行失败，补清单后须重新审计；不报告全门通过。恢复切换、升级回滚、隔离和其余产品义务持续实施。

2026-10-23 新增[离线数据维护与恢复候选组件](installed-product-offline-data-acceptance.md)：受管/只读密钥库、authority clone 生命周期 OS fence、本机 inspect/backup/verify/plan/restore，以及新路径正式 core 重启恢复回答已实现。离线存储投影不依赖模型配置可运行；实际临时进程链路已验证，活动数据切换与完整 Host 生命周期继续实施。总目标是全部已批准设计的实现与统一验收，组件完成不作为任务终点。

2026-10-23 新增[签名发布暂存与窄维护入口组件](installed-product-release-maintenance-acceptance.md)：实际 Rust 验签与同卷不可变暂存、owner 重新认证启停/范围计划、原 operation 核实和回执精确绑定已实现。786 项 Rust、前端 19 项、签名 4 项等源码/进程证据通过；独立 npm audit 受 registry TLS 失败影响。完整升级/恢复、强隔离与原生安装验收仍开放，不能据此关闭全产品里程碑。

| 顺序 | 交付 | 当前状态 |
|---|---|---|
| 24.1 | 宿主协议、安装清单、管理 journal、签名与兼容性校验、内置 OS 服务管理适配。 | 已实现并提交 `92802fe`；Rust 全量门、冻结 18-crate/99-kind 门、宿主专项门通过；不代表原生安装验收。 |
| 24.2 | 真正的系统 service host、受认证双域 broker、实际账户/ACL/逐 job worker 隔离与会话 helper。 | 已有受管 Host、三个 daemon 的 SCM host 与 typed 注册/移除；`29984a4` 增加新安装路径分权，`5cbe175` 证明 Job Object 进程树 custody，`b0f5cfb` 完成私有 workspace/Host 回写及真实 Low worker 的文件/网络/子进程拒绝和宿主死亡恢复，`0f6829a` 将该隔离组合接入固定 SCM/LocalSystem managed Host 路径。真实 SCM Host/core peer、服务账户 token/Brain-Web-keys 有效拒绝与会话 helper 仍未验收或完成；缺少受管配置或身份时 dispatch 继续 blocked，本机尚无真实服务/IP10/IP24 证据。 |
| 24.3 | 原生包、受控停服/升级/恢复/卸载、签名供应链与原生平台演练。 | 已有 bundle/签名、原生编译入口、首次安装权限与失败边界、新路径恢复事务、`639b2b4` 离线检查/备份和 `cce9ea6` 独立 updater/live readiness 控制面。Windows SCM/管理员安装、签名 Inno、Linux/macOS 原生服务、完整隔离和可发布包仍开放；IP01–IP24 未因此关闭。 |
| 25 | 无配置初始化、旧脑验证/认领、账号/session、typed config/secret resolver、内部身份与 Web API/TLS。 | 认证/认领、旧脑/备份候选、OS 本机恢复、模型/脑偏好/skills 公共设置 v2、LAN TLS 发布/健康/回退及恢复提交已有组件和实际临时进程证据。尚缺完整能力/渠道接入、管理员及客户端证书信任/跨设备证据和整机联合初始化验收；不能将组件通过等同完整 IP06/IP08/IP09/IP13/IP21/IP24。 |
| 26 | 五个 Web 工作面、正式流/审批/续接、精确前端来源审查与安装包联调。 | 已有 React/TypeScript/Vite 静态前端、初始化/登录、会话创建/列表/重命名/归档、对话/事件续接/审批/取消、附件加密上传与授权产物历史、pending/Unknown 原命令核实、会话 workspace 到后续 `SessionBound` 正式 run 上下文绑定、项目登记/路线规划/执行/取消、Charter 读写、权威项目监管快照与暂停/恢复、后续项目分段调整及长期意图取消、认知投影与处置、typed 活动运行 pending Inbox 投影、模型设置和受限维护。仍未闭合：剩余设置/维护/API、真实浏览器完整流程及 IP17–23/IP24 联合产品验收。普通建议 Unknown 刷新恢复已由 [普通建议恢复组件验收](installed-product-proposal-recovery-acceptance.md) 记录；组件门不替代原生服务和正式安装包证据。 |

24.1 是 prd/24 的第一笔组件交付；25 的账号/session 是另一笔组件交付，均不表示 prd/24、prd/25 或 prd/26 整体完成。后续按依赖推进，不能以组件测试提前宣布独立部署/Web 产品已可用。

最新增量和证据边界见 [主脑投影与认知处置组件验收](installed-product-brain-views-acceptance.md)；前批见 [安装权限分区组件验收](installed-product-service-access-acceptance.md)、[模型配置与主脑启停验收](installed-product-configuration-acceptance.md)，2026-10-21 历史见 [受管服务与工作台组件记录](installed-product-managed-workbench.md)。当前在 `shu/installed-product`，不是 main/发布版本；本轮没有读取真实 owner 密钥、迁移个人数据或注册系统服务。

### 最新主脑工作台（2026-10-22，`477ef52`）

工作台新增正式记忆/认知/收件箱投影、证据与血缘、受 governor 约束的候选处置及持久命令回执，AskToLearn 回答/拒绝/延后接回原学习路径。Unknown 不重复执行，同一候选刷新后换号也阻断；浏览器不能自填 evidence/actor/派生列表。详细见[主脑投影与认知处置组件验收](installed-product-brain-views-acceptance.md)。完整 IP20、实际浏览器/原生产品验收尚未完成；建议 Unknown 刷新恢复和真正的运行待决收件箱此前未闭合。

### 最新运行待决收件箱投影（2026-10-23）

收件箱现在从现有 `RunPendingView` 投影活动运行的 ApprovalWait、ToolInterrupt、Handoff、ResolutionUnknown 检查点，以固定安全摘要显示并导航回原 run；终态 Failed/Limited 不再被当作待处理。没有新增 `EventKind`、protocol 对象、状态机或持久事实源。Rust、前端、全 workspace 和 Web 工作台命名门通过；完整 IP20、真实浏览器刷新/断线、多 Web 进程及安装产品联合验收仍开放。详见[运行待决收件箱组件验收](installed-product-inbox-pending-acceptance.md)。

### 最新权限分区（2026-10-22，`29984a4`）

新安装使用独立 core 发布目录与 brain runtime，保持认领前新脑为空；重复准备读取原 template，旧配置/密钥/ack/fence 原地保留。Windows 首次安装 helper 只处理固定默认根与服务 SID，先保护后写入，设置并回读 ACL 后才允许启动 Host/Web；失败阻止后续步骤，卸载失败也阻止移除程序。Linux/macOS 服务启动路径同步，权限与原生目标验收并未因此完成。

最终源码门 PASS：146 个 Rust suite 汇总 752 passed / 0 failed / 5 默认 ignored；前端 14/14、bundle 9/9、Windows PowerShell 5.1/7 的 NTFS 组件各 7/7、strict Clippy、完整合规和冻结图门通过。结构门扩大到安装脚本，438 个文件、63 处历史规模债务未增长。宿主/identity/ingress/workbench/配置进程门最终串行复验通过；中间传输失败和证据限制保留于上述报告。没有管理员 SCM、Inno 编译/签名、真实 provider 或 IP01–IP24 新验收。

### 最新配置闭合（2026-10-22，`01b77f9`）

统一源码门 PASS：Rust 749 passed / 0 failed / 5 默认 ignored，strict Clippy、前端 14/14、bundle 7/7、依赖漏洞审计和资源可重复校验通过；结构门扫描 433 个文件、63 处历史规模债务未增长。默认参数完整合规门另跑 PASS。宿主 70 个命名场景、identity 18/18、ingress 8/8、workbench 8/8 与冻结 workspace 门通过。

配置专项 16 个 Rust 场景和 1 个正式 binary 综合进程场景通过，真实浏览器完成临时账号登录/配置检查/错误编辑恢复；真实 Run 经 ProductionRuntime 调用本地计数模型。原生 SCM 启停仅有 adapter 契约证据，本轮没有原生安装、真实 provider、完整 M5 长链重验或新的发布回执。详细命令、截图与未闭合内容见上述报告。

### 前批工程维护与认证基础（截至 `1140101`，历史记录）

- 维护规范：`docs/engineering/code-quality.md`，结构检查器/CI 为 `tools/code-quality.py` 与 `.github/workflows/code-quality.yml`。初始 66 个历史超限文件是待还债基线，不是“质量全部通过”的认证；新增超限或旧文件增长拒绝。当前数值以最新组件记录为准。
- Gateway 拆分：`b373aec` 保持 public API、事件、生产装配与测试名字不变，按控制域和 HTTP 处理职责拆开。`lib.rs` 从 3641 行缩至约 230 行，`server.rs` 从 2256 行缩至 599 行。其余历史大模块继续按风险逐批维护，不全库机械切片。
- 宿主拆分：`26ce4b4` 将 783 行的 `forme-hostd.rs` 按配置/凭据、HTTP、进程生命周期与 SCM 分开，入口剩 40 行并移除旧规模豁免。最终 workspace 688 passed / 0 failed / 5 默认 ignored，宿主专项 60 个命名场景通过；当前扫描 341 个文件、65 处历史规模债务，详见 [可维护性记录](installed-product-maintainability-progress.md)。`57e34b8` 同步修复 M4/M5 审计的旧文件定位，保留缺失实现/案例时的阻断。
- 认证：`12b3f5d` 只写独立认证库，不打开脑库。签名账号认领接缝、Argon2id、session/CSRF/epoch、operation-bound reauth、口令替换与重启持久性详见 [组件报告](installed-product-25-identity-foundation.md)。`52808df` 增加了本机证明、有限 setup scope、候选绑定、签名 claim、journal hash chain 和丢响应恢复，详见 [claim 组件报告](installed-product-25-bootstrap-claim.md)。当前没有安装服务或可登录的新网页地址。
- Web 入口：`49674b7` 增加 `forme-webd`、精确 loopback Host/Origin/CSRF、独立限流、有界连接/密码工作、登录挑战和已知认证 schema 升级；`52808df` 将 `/api/v1/bootstrap/prove|data|claim` 接到有限 setup client。原有 HTTP/进程门与新增 setup 单测通过；联合 hostd+webd 初始化链路和真实浏览器仍待验收，尚无业务页面或 LAN TLS。
- 上述实现仍在 `shu/installed-product`，尚未推送/合入 main；本轮未读取真实模型密钥或迁移个人数据。原生安装、五面 UI 和 IP01–IP24 仍未由本组件门验收。

## 2. 宿主底板的实现边界

- `protocol::installed_product` 是新增版本化产品 DTO；不增加脑 EventKind，不改已有脑协议的序列化结果。
- `execution::installed::ProductJournal` 是与 brain store 分开的管理事实源。OS 文件锁只允许一个管理 writer；`product_seq` 是它自己的顺序。命令幂等、expected revision、阶段 intent/receipt、hash chain 和 replay 共用一个 fold。
- append 先校验，再写入并同步，最后更新内存投影。写入结果不明即阻断后续写；残缺记录、hash/顺序不一致、安装身份不一致均保留原件并拒绝，不截断后继续。
- pending stage 在进程被杀后保留；启动读取只重建状态，不执行服务命令。显式 RecoveryRequired 不能被普通 receipt 覆盖成成功。真正的经认证恢复/裁决入口仍属后续 broker 工作。
- `ReleaseKeyring` 仅接受管理员预置公钥；包不能自行新增信任根。签名绑定规范化 manifest，文件集封闭，验证二进制/静态 UI 内容、大小、平台与兼容范围；切换前还需再次校验。
- 路径检查拒绝相对路径、穿越、Windows device/ADS/大小写别名及已存在的 symlink/reparse-point。它是安装前检查，不独自证明 ACL 或抵御具有目录写权者的所有置换；实际账户/ACL 和执行时身份重查在 24.2/24.3 验证。
- `SystemServiceManager` 在 Windows 使用公开的类型化 SCM API；其他目标使用固定原生服务管理命令、清空环境、有界等待和输出。只接受内置服务，不接收任意程序或 shell。服务启停需观察到预期 OS 终态；进程 Running 不等于主脑 Ready。它不是 SCM dispatcher、LaunchDaemon 或 systemd service host，原生注册/冷启动尚未验收。
- `forme-hostd` 提供仅 loopback 的内部 HTTP 原型，维护与 AgentExecution 使用独立 listener/路由/凭据和连接配额；Windows SCM 路径加载受管凭据文件。全部路由认证，拒绝跨域 token/路由、错误 Host、模糊 framing、浏览器 Origin 和非 loopback bind，连接数与请求大小有界。尚无浏览器登录、近期 owner reauth 或原生 peer identity；不能把这一原型部署成公开管理服务。
- broker 仅激活 Inspect/StartCore/StopCore/RestartCore；其他 maintenance 类型在 plan 前拒绝，不落假操作。proof、active release、refs、deadline 与 command digest 绑定；动作前写持久 Checking intent。成功 operation 重查不再调用 adapter，中断则只能检查原 intent，不盲重试。真正的近期认证、typed lifecycle workers 和双凭据域属于 24.2/24.3。

### 2.1 Worker admission foundation (24.2)

- `WorkerJob`、`WorkerPermit`、`WorkerObservation`、`WorkerReceipt` 和 `WorkerCancelReceipt` 是版本化产品 DTO，不进入 brain EventKind；Job/Permit 同时绑定 owner、action、affected resources、exact plan digest、workspace scope、sandbox profile、authority epoch、deadline、nonce 和 cancel ref。
- `WorkerExecutorHost` 只接受 `HostCredentialDomain::AgentExecution`；OwnerMaintenance 凭据、错误 principal、陈旧 authority epoch、越界 workspace、替换后的 plan/resource、过期 permit 和无 scope allowlist 均在底层隔离器之前拒绝。
- `WorkerJournal` 使用独立 OS 锁、单 writer、顺序与 hash chain。admission、start、cancel、receipt、unknown 都先持久化再更新投影；进程重启发现未终态 job 时只把**原 job**置为 `Unknown`，不创建新 job、不自动重做副作用。
- 默认 `UnavailableWorkerIsolation` 永远拒绝执行，加载配置不会自动解除阻断。仅前台显式 `--development-process-workers` 可用于普通进程开发测试；SCM 服务路径拒绝该开关，生产需后续平台 adapter。这使“还没有真实 sandbox”表现为不可用，而不是偷偷以 brain/高权限宿主身份执行。
- `WorkerLaunchRegistry` 只接受安装配置预注册的绝对程序和固定参数；`ProcessWorkerIsolation` 通过 profile 选择入口、清空继承环境、写入受保护的 job/permit、切换到精确 workspace 并以子进程退出码生成 receipt。它是进程边界，不宣称已具备平台级强隔离；生产 strict profile 仍必须注入对应 OS adapter。
- `WindowsJobWorkerIsolation` 使用 `win-custody 0.1.1` 的公开 Job Object/launch API，在首次执行前接管 worker，启用 kill-on-close，并通过原 Job Object 终止进程树；它只证明进程树生命周期 custody，不证明账户、ACL、网络、credential 或会话隔离。managed service 在其余原生门闭合前不自动选择该 adapter。
- 专项正反测试为 5/5：隔离能力缺失阻断、双凭据域边界、plan/scope/epoch/deadline 绑定、重复 admission 不重复副作用、重启 Unknown 不自动重做、terminal cancel 不重复调用隔离器。测试使用仓库自建 fixture，不代表 Windows SCM、Linux systemd 或 macOS LaunchDaemon 已通过。
- 进程专项测试为 3/3：真实子进程成功 receipt、非零退出的失败 receipt、取消终止且二次取消不重复副作用。
- Windows Job Object 专项测试为 2/2：真实成功/失败退出、后代进程树终止及取消幂等通过；不提升 IP10/IP24。

### 2.2 Worker 控制面（24.2）

- 新增 closed `WorkerDispatchRequest` 与 4 MiB 有界 `WorkerRuntimeConfig`，拒绝未知字段/版本、重复 scope/profile、缺失映射、路径交叠及管理文件混用；注册入口不能由 Agent 请求替换。
- 执行请求只在 AgentExecution listener 认证；维护凭据不能 dispatch，Agent 凭据也不能在维护端口调用执行路由。两个端点各自限制连接，不以路径判断代替端点隔离。
- host 将 admission/start、probe/receipt、cancel/receipt 串行化；取消回执参与 journal fold，查询和重启保留 exact outcome/receipt。并发取消不会抢在 admission 完成前进入 adapter。
- 真实 HTTP 并发重复派发由 fixture 的追加计数证明只执行一次；错误请求的 journal 长度不变且无 marker。Windows 使用长度观察而非读取已被独占锁定的 journal，不解除锁来做测试。
- 证据详见 [worker 控制面报告](installed-product-24-2-worker-control-plane.md)。此批仍未注册服务、创建系统账户、操作真实 owner 数据或导入前端。

## 3. 验证与证据

组件门：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-installed-host.ps1`。每个命名测试必须真实执行一次，测试不存在/被忽略/零执行均失败；末尾明确 IP01–IP24 为 NOT_RUN。

组件覆盖：closed schema、三服务路径/账户清单、保留数据卸载边界、跨平台路径拒绝、并发 CAS、跨重启幂等、真实子进程持锁与被杀、阶段待核实、torn/tampered journal、签名/平台/二进制/UI/extra-file 拒绝与回滚兼容性；另外覆盖 bearer/proof/release 拒绝、过期零调用、并发只一次、真实 broker effect 后进程被杀不重放及真实 HTTP 认证/幂等。fixture 在独立临时目录产生一次计数；真实 HTTP 仅执行只读 Inspect，不操作已安装系统服务。

此前底板已完成 fmt、workspace check/test、strict clippy、compliance、冻结 18-crate/99-kind 门与原 S1–S99 workspace 回归。worker 控制面 `f1122ec` 当时的宿主专项 51 项、657 passed / 0 failed / 5 默认 ignored 是历史组件证据，保留于 [worker 控制面报告](installed-product-24-2-worker-control-plane.md)。最新源码 `26ce4b4` 的全量/宿主/质量证据见上文；当前 `tools/verify-m5.ps1` 与发布回执重验统一见 [M5 报告 §10](m5-acceptance-report.md#10-产品化分支的永久回归重验)。macOS/Linux 原生目标和全部 IP 安装场景继续 NOT_RUN，不使用 Windows 组件测试代替。

此前 `785027b` 后复跑结果：统一代码质量门 PASS（结构回归 6/6、421 个源文件、63 个历史规模债务未增长；Rust fmt/check/test/strict Clippy、前端依赖审计/TypeScript/行为测试、Web 资源、bundle、原创性与许可证均 PASS）；产品组件门 PASS（Host/worker 专项、Web identity 16/16、Web ingress 8/8、Web workbench 8/8，以及 webd 空环境启动、kill/restart 和独立认证存储 smoke）。这是历史组件证据；最新结果见上方 `01b77f9` 记录，两者均不提升为 IP01–IP24。

本批证据边界：组件测试、loopback HTTP 集成测试、受注册入口的真实子进程测试和 Windows Job Object 进程树合同通过，worker admission、进程启动、进程树终止和 receipt 恢复基础已闭合；这仍不等于 SCM/systemd/LaunchDaemon 已注册，不等于 worker 账户/ACL/网络/credential/会话隔离已证明，也不等于 Windows/macOS/Linux 安装包可发布。下一批继续实现平台级权限/网络隔离、常驻 service host、会话 helper 与 24.3 安装事务。

## 4. 依赖、数据与环境

execution 直接复用已有 `ed25519-dalek =2.2.0`（关闭 default features，仅 std）与 `getrandom =0.4.3`；新增 Windows-only `windows-service =0.8.1` 使用公开 SCM API，以及精确 pin 的 `win-custody =0.1.1` 使用公开 Job Object/launch API。没有新增内部 crate/依赖边或内核，锁文件、`compliance/borrowing-records.json` 和来源/许可清单同步更新。测试签名密钥是仓库自建的确定性 fixture，不是发布密钥。

本批不会读取或迁移本机 `.forme` 里的真实脑/模型配置，不注册系统服务，不更改系统账号/防火墙/开机项。测试数据只在临时目录创建和清理；密钥、bootstrap proof 和账号口令不进入提交。
