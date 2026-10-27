# Windows 产品候选升级与统一 Product URL 验收计划

状态：执行计划，尚未授权执行。本文只定义如何把当前已安装的旧候选受控切换到 `0cdec99`；不授权 UAC、服务停止、程序替换、配置写入或重启。它承接 [Windows 原生验收边界](installed-product-windows-native-acceptance-plan.md)、requirements/17–18、prd/24–26 和 architecture/10。

## 1. 目的与产品边界

本轮验证的对象是一个完整的 ForMe 产品实例，而不是“网页桥接一个 Agent”：

- 一个安装实例只对外呈现一个 Product URL、一个 `SuperAdmin/Owner`、一个 Web 控制面和一个唯一 ProductionRuntime。
- `forme-webd`、`forme-hostd`、`forme-gatewayd` 与 worker 是同一产品的内部安全/故障隔离单元；服务名、loopback 地址和内部凭据不进入用户工作流。
- Web 进入 `Running` 后，Product URL 必须在 5 秒预算内呈现登录、初始化、维护或启动状态；BrainReady 可以异步到达，但不能被空白页或无限 spinner 掩盖。
- 登录账户拥有 ForMe 产品控制面的完整权限，但不等于模型或 Agent 获得 Windows `SYSTEM`、任意 shell 或绕过 Harness 的权限。
- 关闭网页、退出登录或宿主用户注销不能停止已授权的非交互服务；宿主关机、断电或休眠不宣称仍在本机计算。

## 2. 固定输入与当前基线

### 2.1 待验证候选

| 项目 | 固定值 |
|---|---|
| 源提交 | `0cdec991a69de74400b5c124850387ec9e7fc79a` |
| release id | `forme-windows-x86_64-20261028-0cdec99` |
| 安装包 | `C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-0cdec99-package\artifacts\forme-0.0.1-windows-x86_64.exe` |
| 安装包 SHA-256 | `07f91f948215c0f14ec776f35794376b5cbe840735da5068752302c6aa728c8a` |
| bundle digest | `7a5e3397c64e0f2344c84d22cc0d96e1b39cb778ccdf614d33d1ad2cbea36a28` |
| 签名 | 未签名，仅限本机受控开发验收 |

候选尚未进入 `C:\Program Files\ForMe` 或 `C:\ProgramData\ForMe`，不能用构建成功替代安装和运行证据。

### 2.2 已观察安装基线

以下是本计划建立时的只读观察，不读取密钥、口令、cookie 或脑数据正文：

| 项目 | 观察值 |
|---|---|
| `forme-host` | Running，Automatic，LocalSystem |
| `forme-web` | Running，Automatic，`NT SERVICE\\forme-web` |
| `forme-brain` | Stopped，Manual，`NT SERVICE\\forme-brain` |
| `forme-release-updater` | Stopped，Manual |
| Product URL | `http://127.0.0.1:18765/` |
| bootstrap status | HTTP 200；`claimed=true`、`setup_needed=false` |

`18765` 是当前实例已经持久化的产品端口。升级不得因为候选默认模板使用 `8765` 就静默改端口；端口变化必须走已认证配置事务并另存证据。

## 3. 执行前只读预检

只有以下检查全部通过，才可向 owner 请求本轮具体执行授权：

1. 对候选安装包重新计算 SHA-256，并核对 bundle inventory、release id、目标架构、18 文件闭合集和 `native-build-receipt.json`。
2. 核对当前四个服务的 image path、启动参数、账户、Service SID、恢复策略、PID 和状态；发现未知服务绑定、未知程序或不明状态立即停止。
3. 核对 `C:\Program Files\ForMe`、`C:\ProgramData\ForMe`、当前配置 revision、安装 authority、数据/备份/工作空间根和当前 Product URL；只记录路径、摘要和元数据，不导出私密内容。
4. 核对现有服务、Web listener 和 Host 管理操作没有 pending/Unknown/RecoveryRequired；有未决操作时不能把升级当作普通覆盖。
5. 核对目标端口可继续使用；不自动修改 Windows 防火墙、证书信任、网络配置或个人 workspace。
6. 保存只读预检报告和当前程序/配置摘要，确保失败后能够精确知道旧版本、旧绑定和旧数据边界。

## 4. 受控切换顺序

执行时只能使用一个固定 operation id，并由 Host/updater 的现有 journal、CAS、排空、备份和健康检查语义驱动。安装器不能直接覆盖正在运行的旧 release，也不能重跑“首次空根安装”流程接管现有数据。

1. **Admission**：owner 重新认证；固定候选摘要、当前 release、产品 revision、服务集合和目标 Product URL，创建升级 operation。
2. **Drain**：阻断新维护/AgentExecution 准入，等待在途操作有界完成；不把未完成动作改写成成功。发现未知副作用则停在 `RecoveryRequired`。
3. **Stage**：把候选放入新的版本化 release 目录，逐文件核对 bundle digest、程序 image 和配置兼容性；不替换当前 active release。
4. **Backup**：按 PRD24 生成可核验的配置/产品状态备份引用；不把 provider key 原文写入报告或浏览器。
5. **Switch**：由受管 Host/updater 原子切换 active release 和服务 image 绑定；保留旧 release、旧 journal 和原配置，不能通过 Web 请求拼接命令或路径。
6. **Start**：先按固定顺序恢复 Host 与 Web；Web 先达到可达状态，Product URL 在 5 秒预算内呈现真实状态。配置已经有效时，Host 再协调启动 Brain；没有有效模型/密钥或 doctor 阻断时，Brain 保持明确的 `Stopped/Degraded`，不得启动空脑。
7. **Readiness**：核对实际运行 image 来自 `0cdec99`，Host/Web/Brain 的 install、owner、config revision、service identity 和 readiness 绑定一致。Web 显示 `Starting`、`Ready` 或真实故障，不由页面自报。
8. **Re-admission**：只有 BrainReady 和配置确认版本与当前 revision 一致，且 worker/能力快照有效时，才恢复新 run；进行中的旧 run 保持原快照。
9. **Receipt**：持久化精确 operation receipt、健康证据、服务状态和 Product URL 响应；任何响应丢失只查询原 operation，不换号重试。

## 5. 本轮必须证明的行为

### 5.1 Web 与产品一体化

- 浏览器只访问 Product URL，不能需要用户启动第二个终端、CLI、brain URL 或内部端口。
- 登录后的唯一账号可以在同一工作台查看产品状态、配置、会话、workspace、审批、认知、维护和恢复入口。
- Web/core 分别重启时，页面回到同一 ForMe 实例的登录/维护/启动状态，不创建第二个 owner、第二个脑或第二个 store。
- 退出网页只撤销会话，不停止后台服务；重新登录后能以原产品状态继续观察。

### 5.2 主脑自动启动

- 首次未认领或未配置时，只启动 Web/Host，页面可初始化，不能冒充 BrainReady。
- 配置保存成功后，Host 根据同一配置 revision 自动协调 Brain 启动；正常路径不要求用户再执行命令或点击“启动主脑”。
- 自动协调必须幂等：重复保存、Host 重启、Web 刷新或重复回调不能产生重复 Start/Restart operation。
- 配置无效、provider 不可达、配置版本未知、服务未安装或恢复状态不明时，保持可诊断状态，不盲目重试、不创建空脑。

### 5.3 统一权限与隔离

- SuperAdmin 可操作产品控制面声明的全部功能，但每个高风险动作仍需 operation-bound reauth、policy、最终 plan 和 Host allowlist。
- Web 不持有内部 Runtime bearer、provider key、master key 或 AgentExecution credential。
- 模型输出、浏览器请求、网页参数和内部服务身份都不能绕过 Harness、permission、CompetenceGate 或 worker 隔离。

## 6. 失败与回退边界

- 程序替换、服务启动、健康检查、journal receipt 或副作用状态不明时，保留原 operation 为 `Unknown/RecoveryRequired`，不自动重跑、不换 operation id。
- 新 release 未通过健康检查时，只能由 owner 依据原 operation 显式选择兼容回退；回退代码不能撤销已经发生的外部动作。
- 回退前必须重新核对旧 release、旧配置、数据 authority、服务绑定和当前 active PID；回退失败保留现场并让 Product URL 进入维护态。
- 不删除 `C:\ProgramData\ForMe`、brain、web-auth、keys、backups、workspaces 或旧 release；删除/卸载另需明确操作和独立报告。

## 7. 证据与完成判定

本轮输出应至少包含：

- 候选 digest、实际 active release、六个程序摘要和四个服务的 SCM 绑定/账户/状态；
- Product URL 的冷启动可达耗时、bootstrap/status、登录和状态页真实内容；
- 配置 revision → Host 自动启动 Brain → BrainReady 的事件/状态链；
- Web 关闭、重新登录、Host/Web/Brain 受控重启后的同一实例恢复证据；
- 重复保存/重复刷新/响应丢失时的 operation 幂等证据；
- 若失败，完整的旧/新 release、journal、Unknown/RecoveryRequired 和未继续执行边界。

本计划只覆盖当前 Windows 本机候选升级，不能替代 Linux/macOS 原生服务、签名发布、LAN HTTPS 跨设备信任、worker 原生隔离或 IP01–IP24 全部联合验收。只有真实平台和真实产品流程证据才能提升对应 IP 状态；fixture、静态前端、HTTP 200 或模型文本自报均不够。
