# Web 运行时链路验收

2026-10-26，基于 `shu/installed-product@a5b6a62` 的可选真实浏览器运行时夹具，验证 Web → Gateway → ProductionRuntime 的一条完整本地对话路径。本文只记录该夹具证据，不把组件夹具或本地模型替身扩大为真实安装、真实 Provider 或 IP01–IP24 通过。

## 1. 验收范围

- 使用正式内嵌 Web 静态资源、正式 Web daemon、正式 Gateway 和 `ProductionRuntime`。
- 使用临时预认领 owner、临时 identity store、临时 bridge credential 和计数型本地模型；不连接外部模型 Provider，不注册系统服务，不使用用户数据目录。
- 允许浏览器完成登录、提交一条对话、等待运行结束、刷新页面，再读取会话和运行状态。
- 夹具默认 `ignored`，只有显式 `FORME_BROWSER_RUNTIME_PREVIEW=1` 才运行；停止必须由临时 stop marker 控制，结束后临时目录自动清理。

## 2. 实际结果

| 检查 | 结果 |
|---|---|
| 登录与工作台加载 | PASS；浏览器进入已认证工作台，使用临时 owner 账号。 |
| 对话提交 | PASS；一条用户消息进入 `workspace:lifecycle`，最终收到 `drained-result`。 |
| 运行链路 | PASS；实际运行记录 19 条事件，包含 `RunAccepted`、`ContextBuildFinished`、`ModelCallStarted`、`ModelCallFinished` 和 `RunComplete`，每类各 1 次。 |
| 模型调用 | PASS；计数型本地模型恰好调用 1 次，未把模型质量或 Provider 可用性作为本夹具结论。 |
| 工具副作用 | PASS；记录 0 次 `ActionStarted`，本次路径没有工具动作。 |
| 刷新恢复 | PASS；刷新后会话、工作空间和已完成运行仍可恢复，没有产生第二个运行或重复模型调用。 |
| 清理 | PASS；Web/Core 停止后临时数据目录不存在。 |

## 3. 事件与边界

本次验证证明的是生产装配路径可被真实浏览器驱动，并且一次普通对话经过 Gateway 进入同一 `ProductionRuntime`，事件和恢复状态可观察。它不证明：

- 真实模型 Provider、模型质量、费用或长期主动性；
- 真实管理员安装、Windows SCM/LocalSystem、Linux systemd、macOS LaunchDaemon；
- 签名安装包、升级/卸载、LAN/TLS 或跨平台原生服务；
- 真实工具副作用、审批、隔离 worker、桌面 helper 或多进程服务重启；
- IP01–IP24 的完整产品验收。

浏览器会话可见性竞态修复（BFCache/后台恢复期间先遮蔽页面，当前 session 校验成功后才显示）由 `a5b6a62` 提交，并有 7 个前端竞态测试覆盖旧请求、失败请求、重复恢复、隐藏/关闭和卸载边界。该修复与本验收报告分开提交。

## 4. 证据等级

结论：**Web 运行时本地链路 PASS（受限夹具证据）**。

这是“真实浏览器 + 真实 Web/Gateway/ProductionRuntime 装配 + 计数型本地模型”的运行证据；不是已安装产品证据，也不是外部 Provider 或完整产品 IP 证据。产品化总状态继续以 [独立部署与 Web 产品化实施记录](installed-product-implementation-progress.md) 及各平台验收记录为准。
