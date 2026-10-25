# Windows 桌面 Helper Dispatch 组件验收

日期：2026-10-26
实现提交：`89bec64`
永久验收门提交：`68cca5a`

## 范围

本次闭合的是桌面动作从受治理 Worker job 到 `DesktopHelperAdapter` 的 Host dispatch 边界，不是操作真实桌面的原生 helper。

- action 必须绑定原 `WorkerJob`、`WorkerPermit`、最终 plan digest、desktop grant、capability、nonce 与 deadline；Host 不创建授权、不选择 OS session，也不重规划。
- grant 过期或 capability 不匹配时，在调用 helper 前拒绝。
- helper 在动作开始后失联时保留 `Unknown`，只查询原 job，不重试副作用；精确终态回执可幂等重放。
- 默认实现仍是 `UnavailableDesktopHelper`，交互式桌面执行继续 fail closed。

## 验收证据

- `forme-execution` 的 `installed_worker_contract`：11/11 通过；覆盖精确 dispatch/终态回执幂等、过期或 capability 不匹配零 helper 调用、开始后 helper 失联转 Unknown 且不重试。
- 新增三个场景已加入 [宿主永久门](../../tools/verify-installed-host.ps1)，并在该门中通过；真实 AppContainer 专项仍单独判定，不与协议/模拟场景混算。
- 当前 `verify-m5.ps1` 所运行的 S1–S99、C1–C24、workspace、strict Clippy、Python 质量测试和合规场景均通过。该脚本第一次终止于 S99，是因为当时验收脚本修改尚未提交；提交 `68cca5a` 后，当前树 M5 release audit 与生成回执的独立验证通过。当前树回执与仓库回执的最终比较记录在 [M5 验收报告](m5-acceptance-report.md#10-产品化分支的永久回归重验)。

## 明确未验收

- 生产 `ProductionRuntime` 尚未接通 `WorkerExecutionSpec` / `WorkerBrokerTransport` 到该 helper dispatch。
- 没有实现或运行真实 Windows 登录会话 helper；生产 adapter 仍不可用。
- 本机没有已注册的 forme SCM 服务或 LocalSystem Host 运行身份；没有原生安装与 IP01–IP24 证据，尤其不能据此标记 IP10/IP11/IP24 通过。

因此本报告只证明 Host 对桌面 helper 调用的治理与恢复边界，不表示 ForMe 已能在真实桌面执行动作或已完成安装产品验收。
