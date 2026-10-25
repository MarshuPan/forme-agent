# Windows 固定服务 Service SID 组件验收

日期：2026-10-25。实现提交：`8936975`，分支 `shu/installed-product`。范围：固定 forme 服务的 Service SID 配置边界；不等同于真实 SCM 安装、worker AppContainer 隔离、IP10 或 IP24 验收。

## 1. 已实现范围

- `forme-host`、`forme-web`、`forme-brain` 和 `forme-release-updater` 只接受固定服务名，并要求 SCM 的 Service SID 类型为 `Unrestricted`。
- 新注册服务在故障恢复配置前先设置并回读 Service SID；同名既有服务在任何配置复核或变更前重新核验，不接受仅凭账户名字符串通过。
- release updater 的服务接管路径复用同一 SID 校验；SID 配置漂移会拒绝操作并保留原注册。
- 新增固定服务集合、未知服务名和 SID 漂移拒绝测试。

## 2. 验证结果

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --locked --workspace --all-targets` | PASS |
| `cargo test --locked --workspace --all-targets` | PASS，完整测试退出码为 0 |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | PASS |
| `py -3 tools/code-quality.py` | PASS |
| `bash tools/compliance-doctor.sh` | PASS |
| Service SID 专项测试 | PASS |

这些是源码、契约和受控适配器证据；当前主机不是管理员，未注册 forme SCM 服务，因此没有把它们提升为原生安装或真实服务 token 证据。

## 3. 未闭合边界

Service SID 只证明固定服务身份可被 SCM 配置和核验。它不证明：

- 已完成管理员安装、签名安装包或实际服务启动；
- core peer 身份已经与 broker 建立原生认证绑定；
- 每个 worker 已使用 AppContainer、低完整性 token、精确 ACL 和 Job Object；
- 网络、credential、桌面会话和跨平台 worker 隔离已闭合。

因此本组件不提升 `IP01`、`IP02`、`IP09`、`IP10`、`IP24`，也不解除生产 worker 的 fail-closed 状态。
