# 独立产品 24.2 Worker Admission 基础验收

## 1. 范围

本报告只验收 `prd/24` 的 24.2 基础协议与恢复层，不宣称完成原生 worker、服务注册、安装包或 Web 产品。它不增加 brain EventKind，也不改变 18-crate 内部依赖图。

实现提交：`7eec019 feat(execution): add isolated worker admission foundation`

## 2. 已交付

- `forme_protocol`：`WorkerJob`、`WorkerPermit`、`WorkerObservation`、`WorkerReceipt`、`WorkerCancelReceipt`、worker state/outcome。
- `forme_execution::installed`：`WorkerJournal`、`WorkerExecutorHost`、`IsolatedExecutorHost` trait、`WorkerIsolation` seam 和默认 fail-closed `UnavailableWorkerIsolation`。
- permit 与 job 的 exact binding：owner、action、affected resources、plan digest、workspace scope、sandbox profile、authority epoch、deadline、nonce、cancel ref。
- worker journal：独立 writer lock、顺序/hash chain、admission/start/cancel/receipt/unknown fold、重启未终态 job 转 `Unknown`。
- AgentExecution 与 OwnerMaintenance 凭据域继续分离；worker host 不接受维护域凭据。

## 3. 专项证据

命令：

```text
cargo test -p forme-execution --test installed_worker_contract -- --nocapture
```

结果：`5 passed, 0 failed`。

覆盖的反向断言：

1. 未配置真实隔离能力时，dispatch 在副作用前拒绝；维护凭据不能进入 AgentExecution。
2. plan digest、workspace scope、authority epoch、deadline 变化均在 isolation adapter 前拒绝。
3. 同一 job/permit 重复 dispatch 只产生一次 admission，不重复调用 isolation adapter。
4. worker 进程/宿主重启后，原 pending/running job 变为 `Unknown`；再次提交同一 job 不会新建执行或重做副作用。
5. 已有 terminal receipt 后再次 cancel 不会再次调用 isolation adapter。

全局门禁结果：

- `cargo fmt --all -- --check`：PASS
- `cargo check --workspace --all-targets`：PASS
- `cargo test --workspace --all-targets`：PASS
- `cargo clippy --workspace --all-targets -- -D warnings`：PASS
- `bash tools/compliance-doctor.sh`：PASS
- 原有 workspace 测试与新专项测试均通过；真实 Chrome、M4/M5 多进程和真实模型等原有外部条件用例仍按各自报告保持 ignored/独立状态。

## 4. 未完成与不可宣称事项

- 尚未在 Windows SCM、Linux systemd、macOS LaunchDaemon 中注册并运行 worker/service host。
- 尚未证明独立 OS 账户、ACL、Job Object/cgroup、网络隔离、credential slot 和桌面会话 helper 的真实效果。
- `WorkerIsolation` 目前只有测试 seam 与默认拒绝实现；没有把任意 `ExecutionPlan` 直接以 brain 服务账户运行的后门。
- IP01–IP24 仍未完成；安装包、升级/恢复/卸载、bootstrap、owner 登录、Web API 和 React 工作台仍待后续 PRD 24.3、25、26。

因此，本报告结论是：**24.2 worker admission 与恢复基础 PASS；24.2 的平台隔离交付和独立产品整体 NOT COMPLETE。**
