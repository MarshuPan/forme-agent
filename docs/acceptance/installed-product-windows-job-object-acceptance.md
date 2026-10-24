# Windows Job Object Worker Adapter 验收

日期：2026-10-24
实现提交：`5cbe175`
范围：`prd/24` 的 Windows worker 进程树生命周期接缝；不等同于 IP10/IP24 或完整产品验收。

## 1. 已实现范围

- `WindowsJobWorkerIsolation` 使用 Windows Job Object 的 `kill-on-close` 约束 worker 进程树。
- 通过 `Launch::spawn_in_job` 在 worker 首次执行前完成 Job Object 接管，避免先启动后归属的窗口。
- `cancel` 终止原 Job Object 并在有界等待内观察结果；重复取消不重新派发原 job。
- `observe` 只读取受管 process handle 的退出码，继续使用既有 job/permit/plan digest、journal、receipt 和 Unknown 语义。
- Managed service 仍不自动选择该 adapter；服务账户、ACL、网络/credential scope、桌面会话和安装注册尚未由本组件闭合。

## 2. 可执行证据

| 场景 | 证据 | 结果 |
|---|---|---|
| 成功退出与非零失败退出 | `crates/execution/tests/installed_windows_job_worker_contract.rs::windows_job_worker_reports_real_completion_and_failure` | PASS |
| 后代进程树终止 | `windows_job_terminates_a_descendant_without_replaying_the_job` | PASS |
| 取消幂等与原 job 查询 | 同一 Windows 合同测试的 `cancel`/`probe` 断言 | PASS |

实际运行：

```text
running 2 tests
test windows_job_worker_reports_real_completion_and_failure ... ok
test windows_job_terminates_a_descendant_without_replaying_the_job ... ok
test result: ok. 2 passed; 0 failed
```

## 3. 回归门

- `cargo fmt --all -- --check`：PASS。
- `cargo check -p forme-execution --all-targets -j 1`：PASS，使用 F 盘独立 Cargo target。
- `cargo check --workspace --all-targets -j 1`：PASS，随后在 C 盘已有 target 完成。
- `py -3 tools/code-quality.py`：PASS（641 files，62 existing size debts）。
- `bash tools/compliance-doctor.sh`：PASS。
- `git diff --check`：PASS。
- `cargo test --workspace --all-targets -j 1`：本机 C 盘在 `forme-gateway` 链接阶段因 `No space left on device` 中止；不是测试断言失败，不能记为全 workspace test PASS。

## 4. 明确未闭合的边界

Job Object 只证明 Windows 进程树的生命周期 custody。它不能单独证明：

- worker 使用独立低权限服务账户及真实 ACL 拒绝；
- 每 job 的 workspace、网络和 credential scope 隔离；
- 桌面/会话 helper 的授权与撤销；
- Windows SCM 安装、原生服务账户、管理员安装和完整 IP01–IP24；
- Linux systemd/cgroup 或 macOS 原生隔离。

因此本组件不提升 `IP10`、`IP24`，也不解除 ProductionRuntime 的生产 worker blocked 状态。后续必须以各平台原生账户、权限、网络和安装证据闭合这些门，不能用本报告替代。
