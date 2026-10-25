# Windows managed worker composition 接入验收

日期：2026-10-25。分支：`shu/installed-product`。实现提交：`0f6829a`。

本报告承接 owner 已选择的 Windows 每 job 私有副本/overlay 方案、PRD24 §4.1.2–4.1.3 与 architecture/10 §7；它只记录 managed Host 的代码接入，不改写此前 AppContainer、Job Object、overlay 或 M0–M5 的历史验收。

## 1. 实现范围

- Windows SCM service 入口显式标记 `managed_service_bound = true`；前台/开发入口显式传 `false`，不改变开发测试语义。
- 只有 managed Windows Host 在存在并通过校验的 `WorkerRuntimeConfig` 时，才组合每 job AppContainer、Job Object 和私有 workspace overlay。缺少 worker 配置仍使用不可用隔离器。
- 组合隔离状态前，Host 在现有 Windows ACL/native 审计边界内读取当前进程 token，并要求用户 SID 为 `LocalSystem`。身份不符、token 无法读取或 SCM 绑定缺失时 fail closed，不创建 worker 状态。
- worker 的原 workspace 不直接授予低完整性 worker；副本变更仍由 Host 按 baseline、permit、plan 和完整变更集核验后回写。Host 不因安装提权而给 Agent 无限 SYSTEM 权限。
- 没有新增 crate、内部依赖边或脑 EventKind；Linux/macOS 路径不变。

## 2. 验证

| 门 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo test --workspace --all-targets` | PASS；0 failed，默认 ignored 仍按既定真实平台前置条件保留 |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `py -3 tools/code-quality.py` | PASS；既有规模债务未增长 |
| `bash tools/compliance-doctor.sh` | PASS |
| `git diff --check` | PASS |

本机只进行了只读身份/服务枚举：没有发现已注册的 `forme-brain`、`forme-web`、`forme-host` 或 `forme-release-updater` SCM 服务；当前交互用户 SID 不是 `S-1-5-18`。因此本报告没有宣称真实 SCM dispatcher、LocalSystem token、管理员安装、冷启动或 IP10/IP24 已通过。

## 3. 证据边界

- 本提交证明的是生产代码已经具有明确的 managed Windows 组合路径，并在条件不满足时拒绝启动；它不是已安装产品的运行回执。
- 既有真实 AppContainer/overlay fixture 证明继续由对应组件报告承接；本机没有条件执行真实 SCM/LocalSystem 场景，不能把组件门合并成 IP10/IP24。
- Windows 之外的原生 sandbox、Linux systemd/cgroup、macOS service/sandbox、安装包/签名/管理员安装、真实 Brain/Web/keys ACL 与会话 helper 仍需独立实施和验收。
