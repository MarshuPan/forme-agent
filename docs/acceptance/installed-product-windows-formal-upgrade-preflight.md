# Windows 正式升级预检清单

状态：预检与操作规约。本文不授权安装、服务停止、程序替换、配置写入或重启；它用于准备下一次经 owner 单独批准的新升级 operation。当前 `fc4e23e` 新安装入口拒绝记录见 [对应计划](installed-product-windows-upgrade-20261028-fc4e23e-plan.md)。

## 1. 适用边界

正式升级针对已经安装的 ForMe appliance，入口是受管 Host/updater 的签名发布生命周期，不是再次运行 Inno 新安装器。安装器只负责首次安装；发现已有受保护根目录或固定服务时，必须停止并保留原现场。

升级始终绑定一个 installation、一个 active release、一个 owner operation、一个目标 release digest 和一个 Product URL。Web、Host、Brain、worker 与 updater 是同一产品实例的隔离单元；不创建第二个 owner、store 或主脑。

## 2. 发布材料

发布方在仓库外准备以下材料，私钥不得进入仓库、bundle、日志、浏览器或验收报告：

- 经审查的 Windows x86_64 bundle 及其固定 `bundle-inventory.json`。
- `manifest.json`、原始 64 字节 `signature.ed25519`、`content/` 和安全 signing receipt。
- 明确的兼容性文件：`schema_version`、`config_range`、`data_range`、`minimum_upgrader`。
- 管理员独立预置的 `ReleaseStagingConfig`，其中的 public key 与发布方 key id 通过独立信任流程确认；不能从 receipt 自动导入。

签名发布命令和 verifier 以 [packaging README](../../packaging/README.md) 与 `tools/sign-product-release.mjs` 为准。签名 release contents 不等于 Authenticode 签名，也不等于原生安装、服务身份或 IP 验收通过。

## 3. 离线预检

在任何安装目录写入前，由受控 Host verifier 对 release 目录执行只读核验：

1. 检查路径为绝对路径、无链接祖先、目录闭合且不与 bundle/output/key 重叠。
2. 使用管理员预置 trust config 验证 manifest digest 和 Ed25519 签名。
3. 核对 `target=windows_x86_64`、release id、完整 binary/UI 文件集合、每项 digest/size、managed dependencies 和 UI digest。
4. 核对 config/data schema range、`minimum_upgrader` 与已安装 updater 能力；禁止根据版本字符串推断兼容性。
5. 保存脱敏预检摘要：release id、digest、target、trust key id、verifier 版本、结果和时间。不得保存私钥、口令、cookie、bootstrap proof、provider key 或脑/工作区正文。

预检失败时不写 release inbox、不创建 journal operation、不停止服务、不替换文件。

## 4. Host/updater 操作顺序

只有 owner 对该具体摘要重新认证并批准后，才按以下顺序执行一次 operation：

1. Host 绑定 installation、owner、active release、目标 digest、配置 revision、服务集合和 Product URL，持久化 Checking intent。
2. 关闭新维护与 AgentExecution 准入，等待在途操作有界排空；已有 Waiting/Unknown 不伪装成完成。
3. 将签名发布放入 `host-state/release-inbox/<release_id>/`，逐文件复制、sync、复核并生成不可变 staging receipt。
4. 创建可核验的一致性备份引用；原配置、keys、journal、workspace 和旧 release 保留。
5. Host/updater 原子切换 active release 和服务 image binding；不覆盖旧 release，不改写历史 operation。
6. 按 Host → Web 恢复；Host 根据已保存且已 acknowledgement 的有效模型配置自动协调 Brain。无有效配置时，Brain 保持真实 `Stopped/Degraded`，不启动空脑。
7. 核对实际 service image、Service SID、owner/config revision、Brain readiness、worker/能力快照和 Product URL 均绑定目标 release。
8. 仅在健康证据完整后恢复新 run admission；写入最终 operation receipt。

## 5. Unknown 与失败

程序替换、服务启动、健康检查、journal receipt 或外部副作用状态不明时，operation 保持 `Unknown/RecoveryRequired`。只允许查询原 operation、读取原 receipt/probe 或由 owner 选择显式恢复；不得换 operation、再次运行安装器、盲目重派 worker 或猜测已完成。

升级失败不自动删除旧 release、配置、keys、brain、workspace、backups 或 Unknown 证据。代码回退也不撤销已经发生的外部动作。

## 6. 关闭条件

本预检清单本身不提升任何 IP 状态。只有取得真实的签名发布、Host/updater 切换、健康恢复、配置 acknowledgement、BrainReady、Web/worker/审批回归和脱敏 operation receipt 后，才可更新 IP14；完整产品仍需按 [IP01–IP24 策略](../../requirements/18-installed-product-verification-strategy.md)逐项记录。
