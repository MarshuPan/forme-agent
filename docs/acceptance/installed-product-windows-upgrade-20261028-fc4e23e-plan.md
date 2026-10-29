# Windows 产品候选升级与统一 Product URL 验收计划（fc4e23e）

状态：已按 owner 批准执行一次；候选在安装器的受保护安装根预检阶段被拒绝，未发生版本切换。本文记录把当前已安装的旧候选受控切换到 `main@fc4e23e` 候选的计划与实际结果；不把候选存在、安装器启动或源码门通过记作产品升级成功。

## 1. 产品边界

本轮验证对象是一个完整的 ForMe 软件 appliance，而不是“网页桥接一个 Agent”：一个 Product URL、一个唯一 `SuperAdmin/Owner`、一个统一 Web 控制面、一个 ProductionRuntime/主脑和一套持久数据。`forme-webd`、`forme-hostd`、`forme-gatewayd` 与 worker 只是同一实例内部的权限、故障和生命周期隔离单元。

浏览器只访问 `http://127.0.0.1:18765/`。Web 进入 `Running` 后，页面应在 5 秒预算内显示登录、初始化、维护或启动状态；Brain readiness 可以随后到达。认领并保存有效模型配置后，Host 自动协调 Brain，正常流程不要求用户手动点击“启动主脑”。登录账户拥有产品控制面的完整权限，但不等于 Agent 获得 `SYSTEM`、任意 shell 或绕过 Harness 的权限。

## 2. 固定候选和当前基线

| 项目 | 固定值 |
|---|---|
| 源提交 | `fc4e23e6d173be3893d7a4afe390ec84f96ef33c` |
| release id | `forme-windows-x86_64-20261028-fc4e23e` |
| 安装包 | `C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-fc4e23e-package\artifacts\forme-0.0.1-windows-x86_64.exe` |
| 安装包 SHA-256 | `258a6e04823bc9782a045ae465ac95e86ae248c1b4229e7110b08f8c71f4abf6` |
| bundle digest | `sha256:7dcc75c7dd8fef54cc0339da41fd0d25a6ed2ef092d671310a4a6e25dc5cecbd` |
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

## 7. 本次已批准执行结果（2026-10-29）

### 7.1 执行授权与候选预检

owner 已明确批准对本计划固定的 `fc4e23e` 候选执行一次本机受控升级。执行前的只读核对结果如下：

- 候选安装包 SHA-256 仍为 `258a6e04823bc9782a045ae465ac95e86ae248c1b4229e7110b08f8c71f4abf6`，与本计划一致。
- `native-build-receipt.json` 明确记录 `native_package_signature: not_performed`、`native_installation: not_performed`、`release_ready: false`；候选是已编译的开发安装器，不是可供生产 updater 消费的已验证签名发布。
- bundle 仍只有编译产物、静态资源和打包输入，没有可供已安装 updater 消费的签名 `manifest.json`/原始签名发布目录。
- 已安装服务的只读快照为：`forme-host` Running/Automatic/LocalSystem，`forme-web` Running/Automatic/`NT SERVICE\\forme-web`，`forme-brain` Stopped/Manual/`NT SERVICE\\forme-brain`，`forme-release-updater` Stopped/Manual/LocalSystem。
- Product URL `http://127.0.0.1:18765/` 仍可达并返回 HTML 200；未认证的产品状态接口返回 `401 Unauthenticated`。未取得认证控制面的 `allowed_operations` 回执，因此不把 UI 可见性或源码中的操作枚举当作本机升级授权证据。

### 7.2 实际执行与结果

本机曾按固定候选启动 Inno Setup 受控安装器，回执保存在 `target/installed-product-upgrade-fc4e23e.log`。安装器在 `PrepareToInstall` 的受保护安装根预检阶段停止，并明确记录：`Existing files and services were retained`，随后 `Got EAbort exception`。没有进入文件替换、服务 image 切换、配置写入、Brain 启动或 updater 接管。该入口是“新安装”安装器；它发现已有受保护目录和服务后拒绝覆盖，因此本次**没有到达 updater 的发布签名验证路径**。

独立的后续只读核对显示：Host/Web 仍监听原 Product URL，Brain/updater 仍为停止状态，原四个 SCM 注册仍存在；没有证据表明本候选覆盖了既有程序、数据、密钥、配置或服务绑定。该次操作的结果是**受保护拒绝，原安装保留**，不是升级成功，也不是可回滚的中间态。

### 7.3 IP 影响

- IP01/IP02/IP03/IP04：本次未重新执行冷启动、注销、崩溃恢复或无配置安装正向链；现有 Host/Web 可达证据不能替代这些 IP 的完整证据。
- IP05–IP13：本次没有执行认领、登录、配置、数据迁移、worker 或权限正向动作；保持既有状态，不因本次候选预检改变。
- **IP14 升级事务：正向链 `签名→预检→排空→备份→切换→健康→恢复` 为 `NOT_RUN`。** 本次取得的反向证据仅证明：新安装入口拒绝在已有受保护安装根上运行，并保留原安装；它**不证明** updater 已执行未签名发布拒绝，也不能抵扣 IP14 正向验收。
- IP15–IP24：本次没有执行回滚、卸载、完整 Web 工作台联合流程或完整产品回归，保持 `NOT_RUN/UNVERIFIABLE`，不得由本次安装器日志提升。

本次失败现场、候选摘要和服务快照均保留。由于候选没有签名发布 envelope、未进入已安装 release inbox，不能继续重试同一新安装入口或绕过 updater/owner 控制面；后续若要取得 IP14 正向证据，必须先生成并独立核验受信任的签名发布，再以新的明确 operation 进入 Host/updater 维护路径。

## 8. 正式升级前置条件（后续新 operation）

下次升级不能再次运行本安装器覆盖现有根目录，必须使用已安装 `forme-hostd --release-updater` 所托管的发布生命周期。开始新的 operation 前必须同时具备：

1. 与目标 bundle 一一对应的 `manifest.json`、原始 64 字节 `signature.ed25519`、`content/` 和兼容性字段；兼容范围不能由版本号猜测。
2. 管理员独立预置的 `ReleaseStagingConfig` 信任根；签名回执中的公钥只能作为信息，不能自动成为信任根。
3. 通过同一 Rust verifier 的离线核验：目标为 `windows_x86_64`，发布摘要、闭合文件集、UI、managed dependencies、config/data range 和 `minimum_upgrader` 全部匹配。
4. 已安装 Host 的 `release_staging_config_path` 指向受保护配置，发布目录进入该 installation 的 `host-state/release-inbox/<release_id>/`；不能把安装器路径、任意 Web 路径或聊天内容作为发布源。
5. Owner 在 Product URL 完成新的近期认证和 exact operation 绑定；预检、排空、备份、切换、健康和恢复每一步都写入同一 operation receipt。

满足前置条件前，IP14 保持 `NOT_RUN`，不申请新的安装器重试授权。升级失败或副作用状态不明时，只查询原 operation 并保留 `Unknown/RecoveryRequired`，不换 operation 盲重放。
