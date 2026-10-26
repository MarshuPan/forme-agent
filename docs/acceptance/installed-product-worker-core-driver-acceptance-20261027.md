# 受管 core→worker 与结果封存组件验收

- 日期：2026-10-27；分支：`shu/installed-product`。
- 实现提交：`1ae55c6`；权威设计：PRD24 §4.1.1a–c、architecture/10 §7.1、canonical §29.9。
- 本报告记录组件与本机原生 AppContainer 证据，不关闭 IP01–IP24，不表示安装服务或发布已完成。

## 实现范围

受管 `ProductionRuntime` 经显式 `ManagedWorkerBinding` 装配 `WorkerActionBackend`；缺配置或不在 backend 集合内时保持 unavailable，不能回退到 brain 账户的进程内文件、shell 或 MCP driver。最终计划固定原 timeout、scope、资源、动作/计划摘要与真实 rollback 边界；dispatch 一次后只查询、取消或读取原 job。

Host 结果先在独立执行数据域使用真实 key resolver/AEAD 封存，再持久化 manifest 与原 receipt。core 独立核验原 observation、安装 authority、manifest/chunk 摘要、连续偏移、总预算和组装时限，完整校验后才产生既有受保护的执行观察。结果缺失/损坏/取消不明不伪装成成功，不换 job 重派。

Windows AgentExecution named pipe 双向核验实际 PID、固定 SCM/Service SID 与同 release image；core 在验证 Host 前不发送 bearer。临时同名伪 pipe 的拒绝证据不代表本机已注册真实系统服务。AppContainer 收尾必须先终止原树、撤销 ACL/profile，再交接并封存原结果；封存不明保留证据且阻断新准入。

## 本次验证

本轮最终源码日志位于 `target/worker-driver-evidence-20261026/final-*.log`（执行完成于 2026-10-27，不复用该目录中的旧日志结论）。

| 验证 | 结果与边界 |
|---|---|
| workspace 全量测试 | 1066 passed / 0 failed / 12 ignored，175 个 test targets。 |
| fmt / workspace all-targets check / strict Clippy | PASS。 |
| 结构规模与合规 | PASS；743 files、63 项既有规模债务，未刷新 baseline；完整 upstream/license/copy 门通过。 |
| `tools/verify-installed-host.ps1` | PASS，包含真实子进程、结果、恢复、writer/child pipe 接管等组件合同；输出明确 IP01–IP24 NOT_RUN。 |
| `tools/verify-core-brain-closure.ps1 -Wave A` | PASS；正式入口/主脑装配回归，不替代真实 Provider 或安装证据。 |
| Windows AppContainer 显式原生测试 | `installed_windows_appcontainer_contract` 的四个 ignored 场景全部显式执行并 PASS：私有副本与核验回写、失败/取消/owner 冲突无回写、Host 死亡原 lease 恢复、结果封存先于清理与失败保留。无服务安装。 |
| 资源 | 阶段空间检查 PASS；未清理缓存。最终里程碑另保存资源报告。 |

关停回归测试修正了夹具竞态：在 scheduler 完全退出后提交已到期意图，证明不再 claim；没有改变生产停服语义。

## 仍开放的义务

- 已定义但尚未实现的真实 `forme-executord --installed-file-worker`、Host `WorkerFileTarget` 映射和该二进制的 AppContainer 读/写/删证据（PRD24 §4.1.1d）。
- 安装配置的 worker profile、workspace 注册与 core capability 显式装配；入口或 adapter 存在不会自动开启安装产品工具能力。
- 当前源码的完整 M5/其他适用 closure/release audit、完整浏览器/真实 Provider、管理员 SCM 安装、签名与 IP01–IP24 联合验收。
- Linux/macOS 按 owner 选择继续保持有限恢复与 `release_ready=false`，无原生产品支持声明。

结论：受管 core→worker 与结果封存组件验收 PASS；整体产品里程碑继续进行。

后续实施记录：`cc43440`、`1270e54` 与 `7cb5b8b` 已实现本报告当时仍开放的有限文件入口、首装 profile/workspace/capability 装配，以及活动发布程序绑定；新证据见[文件 worker 与离线维护报告](installed-product-file-worker-and-offline-maintenance-20261027.md)。以上历史测试结果不改写，也不自动覆盖后续提交；原生服务/桌面 helper、真实 Provider 和完整产品 IP 继续开放。
