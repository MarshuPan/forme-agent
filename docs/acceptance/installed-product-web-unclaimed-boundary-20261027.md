# 已安装产品未认领 Web 边界证据

日期：2026-10-27。产品施工分支：`shu/installed-product`；当前源码提交：`cf25ebf`；本地 `main` 已与该提交对齐。本文记录一次已安装 Windows 产品的只读 HTTP 观察，不宣称完整 IP04、IP05、IP07、IP22 或 IP24 通过。

## 范围与现场

- 目标：`http://127.0.0.1:18765/`。
- `forme-host`：`Running`，自动启动。
- `forme-web`：`Running`，自动启动。
- `forme-brain`：`Stopped`，手动启动。
- `forme-release-updater`：`Stopped`，手动启动。
- bootstrap 状态：`claimed=false`、`setup_needed=true`、`protocol_version=1`、`schema_version=1`。
- 本次没有提交 bootstrap proof、没有创建 setup session、没有输入或读取账号密码、没有启动 Brain、没有调用模型或工具。
- 已安装程序仍按此前真实安装候选和端口恢复记录界定；当前源码提交不被反向冒充为已安装二进制来源。

## 只读场景

| 场景 | 实际结果 | 反向含义 |
|---|---:|---|
| `GET /`，默认 Host | `200` | 初始化页面在未认领、Brain 停止时可达。 |
| `GET /api/v1/bootstrap/status` | `200` | 只返回最小初始化状态，未暴露 owner、路径或模型配置。 |
| 默认 Host 响应 | `Cache-Control: no-store`、CSP、`X-Content-Type-Options: nosniff` | 浏览器缓存与脚本来源边界已由实际响应表达。 |
| `GET /api/v1/bootstrap/status`，错误 `Host` | `403` | 未知 Host 在进入业务路由前被拒绝。 |
| `GET /api/v1/bootstrap/status`，错误 `Origin` | `403` | 错误来源不能借公开状态路由绕过入口绑定。 |
| `GET /assets/../index.html` | `404` | 资源表不把请求路径当作文件系统路径。 |

以上请求均为本机只读请求，未改变产品状态。响应正文只保留脱敏的状态与安全头观察，不保存 cookie、proof、密钥或私有数据。

## 证据边界

这次观察证明的是已安装 Host/Web 在未认领状态下的入口可达性和若干请求绑定/资源边界。它不能证明：

- owner claim、登录、密码恢复或跨设备初始化；
- 真实 provider、Brain 启动、ProductionRuntime 对话或工具调用；
- 多进程/断线恢复、真实浏览器业务流程、审批、项目、认知和设置工作流；
- Windows worker 的完整账户/token/ACL/网络隔离或桌面 helper；
- LAN TLS 客户端信任、升级/回滚/卸载、Linux/macOS 原生产品支持；
- 任一完整 IP01–IP24 或 IP24 联合回归。

完整产品验收仍按 `requirements/18` 和 `installed-product-implementation-progress.md` 的逐 IP 证据要求执行。
