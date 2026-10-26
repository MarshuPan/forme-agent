# Worker 结果通道专项验收报告

- 验收日期：2026-10-26
- 实现提交：`6c06a880a92dbaa8225d07a3159c407dcd87784a`
- 分支：`shu/installed-product`
- 权威设计：`docs/prd/24-installer-and-service-lifecycle.md` §4.1.1a、`docs/architecture/10-installed-product-and-web-workbench-architecture.md` §7.1.1、`docs/architecture/canonical-contract.md` §29.9
- 决策状态：owner 已批准结果通道；POSIX 恢复保持方案 A 与 `release_ready=false` 边界

## 1. 验收范围

本报告只验收 worker 结果的协议、Host 封存、原 job 绑定、AgentExecution 只读路由和开发 Process worker 的有限结果文件。它不验收正式 ProductionRuntime worker backend、真实 SCM/systemd/LaunchDaemon、安装包、应用层密钥加密或 IP01–IP24。

结果正文不进入 brain EventStore、普通日志、管理回执或公共配置。Host 为每个 job 提供 `FORME_WORKER_RESULT_PATH`，读取后封存到 Host 私有目录；journal 只保存经过校验的 manifest。默认 `UnavailableWorkerIsolation` 仍拒绝生产执行。

## 2. 实现核对

| 项目 | 当前实现 |
|---|---|
| 协议对象 | `WorkerResultBinding`、`WorkerResultManifest`、`WorkerResultRead`、`WorkerResultChunk`，均为版本化、拒绝未知字段的 DTO。 |
| 绑定 | manifest/read 绑定 installation、owner、job、action、workspace scope、plan/action digest、permit、authority epoch、nonce 和 output budget。 |
| 结果状态 | `Available`、`Empty`、`Pending`、`Missing`、`Corrupt`、`Cleaned` 与真实 `WorkerOutcome` 分离表达；缺失或未知不能合成成功正文。 |
| 有界读取 | 总量同时受最终 output budget 与 64 MiB 限制，单 chunk ≤256 KiB；每次读取复核普通文件、长度和整体摘要，chunk 另有摘要。 |
| 路由 | AgentExecution 只读 `GET /v1/agent/workers/{job_id}/result` 与 `POST /v1/agent/workers/{job_id}/result/read`；请求不能换 job、路径、URL、程序、环境或授权参数。 |
| 恢复/幂等 | 结果 manifest 与原 job journal 绑定；未知副作用只查询原 job，不因缺失、超时或响应丢失新派发。 |

## 3. 实际证据

| 门 | 命令 | 结果 |
|---|---|---|
| Process worker | `cargo test --locked -p forme-execution --test installed_worker_process_contract -- --nocapture` | `4 passed, 0 failed`；覆盖成功结果、缺失结果、失败退出和取消不重试。 |
| HTTP/typed worker transport | `cargo test --locked -p forme-execution --test installed_host_worker_transport -- --nocapture` | `8 passed, 0 failed`；覆盖 AgentExecution/OwnerMaintenance 分域、请求拒绝、幂等、并发和结果读取投影。 |
| Windows Job worker | `cargo test --locked -p forme-execution --test installed_windows_job_worker_contract -- --nocapture` | `2 passed, 0 failed`；结果通道提交未改变 Job Object 进程树 custody 合同。 |
| 代码质量棘轮 | `py -3 tools/code-quality.py` | `PASS (733 files, 63 existing size debts)`。 |
| 构建资源门 | `py -3 tools/build_resources.py check` | `PASS (238.9 GiB free)`；未执行清理。 |

## 4. 未完成与发布边界

以下项目仍明确未完成，不由本报告折算：

- 正式 ProductionRuntime 到隔离 worker backend 的完整接入；当前生产默认仍为 unavailable/blocked。
- 结果工件应用层加密与产品执行数据域真实 key resolver 接入；私有目录权限不是加密替代品。
- Windows SCM/LocalSystem、Linux systemd、macOS LaunchDaemon 的真实安装、服务账户、网络/文件/凭据隔离和原生回写证据。
- Linux/macOS 继续 `release_ready=false`；安装包签名、管理员安装、完整 IP01–IP24 与真实 Provider 仍未通过。

结论：**Worker 结果通道组件验收 PASS；独立部署与 Web 产品整体仍未完成，不能据此宣称 forme 已达到可发布安装产品状态。**
