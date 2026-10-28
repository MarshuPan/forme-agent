# Windows 产品候选升级与统一 Product URL 验收计划（0a3d1cb）

状态：执行计划，尚未授权执行。本文只定义把当前已安装的旧候选受控切换到当前 `main@0a3d1cb` 候选；不授权 UAC、服务停止、程序替换、配置写入或重启。

## 1. 产品边界

本轮验证对象是一个完整的 ForMe 软件设备，而不是“网页桥接一个 Agent”：一个 Product URL、一个唯一 `SuperAdmin/Owner`、一个统一 Web 控制面、一个 ProductionRuntime/主脑和一套持久数据。`forme-webd`、`forme-hostd`、`forme-gatewayd` 与 worker 只是同一实例内部的权限、故障和生命周期隔离单元。

浏览器只访问 `http://127.0.0.1:18765/`。Web 进入 `Running` 后，页面应在 5 秒预算内显示登录、初始化、维护或启动状态；Brain readiness 可以随后到达。认领并保存有效模型配置后，Host 自动协调 Brain，正常流程不要求用户手动点击“启动主脑”。登录账户拥有产品控制面的完整权限，但不等于 Agent 获得 `SYSTEM`、任意 shell 或绕过 Harness 的权限。

## 2. 固定候选和当前基线

| 项目 | 固定值 |
|---|---|
| 源提交 | `0a3d1cb` |
| release id | `20261028-0a3d1cb` |
| 安装包 | `C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-0a3d1cb-package\artifacts\forme-0.0.1-windows-x86_64.exe` |
| 安装包 SHA-256 | `4d82b33322ce8fad65c9c5d0a3badf9782701ac1f65f5944c949d8bfe0ba68bb` |
| bundle digest | `sha256:ca5c7f7e3264193c719f1c90cd540736055dd1013f4c2c1a0aa514b7f2b7f8af` |
| 签名 | 未签名，仅限本机受控开发验收 |
| Product URL | `http://127.0.0.1:18765/` |

当前安装服务基线仍为旧候选；执行前必须重新只读核对四个 SCM 服务的 image/account/SID/recovery/PID、Product URL、配置 revision、owner binding 和 pending/Unknown/RecoveryRequired 状态。不能把旧现场状态或本候选构建结果混写成新候选的安装证据。

## 3. 执行前只读预检

执行前重新核对本候选安装包 SHA-256、bundle inventory、18 文件闭合集、native-build receipt、当前四个 SCM 服务、Product URL、配置 revision、owner binding 和 pending/Unknown/RecoveryRequired 状态。只记录摘要和元数据，不读取或输出密钥、口令、cookie、bootstrap proof、脑数据正文或个人 workspace 内容。

预检发现未知服务绑定、不明副作用、未决恢复、候选摘要不匹配、目标端口不可用或原安装数据边界不明确时，立即停止，不覆盖、不删除、不重试。

## 4. 受控切换顺序

1. 以新的固定 operation id 绑定候选摘要、当前 active release、产品 revision、服务集合和 Product URL，并要求 owner 重新认证。
2. 阻断新维护/AgentExecution 准入，等待在途操作有界结束；未知副作用进入 `RecoveryRequired`，不改写为成功。
3. 将候选放入新的版本化 release 目录，逐文件核对摘要和兼容性，不直接覆盖旧 active release。
4. 生成可核验的配置/产品状态备份引用；密钥只由受保护 resolver 处理，不进入报告或浏览器。
5. 由受管 Host/updater 原子切换 active release 和服务 image 绑定，保留旧 release、旧 journal、原配置和回退现场。
6. 按 Host -> Web 顺序恢复服务；Web 先达到可达状态，随后由 Host 根据已保存的有效配置自动协调 Brain。无有效模型/密钥时保持真实的 `Stopped/Degraded`，不得启动空脑。
7. 核对实际运行 image、install/owner/config revision、服务身份和 readiness 均绑定到本候选；页面只能显示真实 `Starting`、`Ready` 或故障状态。
8. 只有 BrainReady、配置 acknowledgement、worker/能力快照都匹配时才恢复新 run；旧 run 不热切换上下文。
9. 写入精确 operation receipt 和健康证据。响应丢失只查询原 operation，不换 key、不重复安装、不盲重试。

## 5. 必须观察的结果

- 浏览器只需 Product URL，不需要启动 CLI、第二个 Brain 地址或内部端口。
- 配置保存后的 Host 自动启动 Brain 真实发生；重复保存、Host 重启、Web 刷新不会重复产生启动副作用。
- 退出网页或宿主用户注销不停止非交互服务；重新登录仍回到同一 Owner、同一 workspace、同一 Product 状态。
- Web/core 分别恢复时进入同一产品的启动/维护状态，不创建第二个 owner、store 或主脑。
- Web、模型、工具和 worker 仍经过既有 Harness、policy、最终 plan、能力门和 Host allowlist；Web 不持有 Runtime bearer、provider key、master key 或 AgentExecution credential。

## 6. 失败与回退

程序替换、服务启动、健康检查、journal receipt 或副作用状态不明时，保留原 operation 为 `Unknown/RecoveryRequired`，停止后续动作，不自动重跑。新 release 未通过健康检查时，只能由 owner 依据原 operation 显式选择回退；回退代码不撤销已发生的外部动作。不得删除 `C:\ProgramData\ForMe`、brain、web-auth、keys、backups、workspaces 或旧 release。

本计划只覆盖当前 Windows 本机候选切换。它不能替代签名发布、Linux/macOS 原生服务、LAN HTTPS 跨设备信任、桌面 helper、worker 原生隔离或 IP01–IP24 全部联合验收；只有真实平台证据才能提升对应 IP 状态。
