# 独立发布 updater 与 live readiness 验收

日期：2026-10-25
实现提交：`cce9ea6`
分支：`shu/installed-product`

## 1. 本次闭合的范围

本次实现把已安装产品的发布切换从业务 Host 的进程寿命中拆出，加入固定的、按需触发的 `forme-release-updater`。它不是第四个业务 daemon，不拥有大脑、模型、工具或普通任务准入。

- Host 在 owner 重新认证和配置 CAS 后，生成一次性、原操作绑定的 takeover，并在响应结束后交给独立 updater。
- updater 在任何停止动作前验证安装绑定、目标签名发布、原发布、配置 digest、期限、journal 和服务清单；marker 先消费，未知副作用不盲重试。
- 三个业务服务只通过固定的 Windows SCM 生命周期接缝停止/启动；数据备份、release image 切换、target 兼容性核对和恢复状态均写入独立 product journal，不写 brain event/store。
- target 服务启动后，Host 通过 loopback-only、无 bearer 泄露的 nonce challenge-response 取得 live signed readiness；成功后才开放该 release 的工作 admission。
- `Checking`、`Committed`、`RecoveryRequired` 和丢响应恢复只重查原 operation，不以进程重新启动或普通运行状态冒充完成。

## 2. 证据

通过的源码/受控运行门：

- `pwsh -NoProfile -ExecutionPolicy Bypass -File tools/verify-installed-host.ps1`：`installed-host-foundation and core-shutdown-components: PASS`。
- updater 场景覆盖真实 writer 转移、子进程 pipe、marker 消费、目标篡改/伪造 proof、备份与切换、commit 后 admission 开放、失败恢复和原 operation 重查。
- Gateway live readiness 场景覆盖错误 Host/Origin/Authorization、超大请求、错误签名、独立限流、普通运行时没有 readiness 权限以及成功 proof 不写脑库。
- `cargo fmt --all -- --check`：PASS。
- `cargo check --workspace --all-targets`：PASS。
- `cargo test --workspace --all-targets --quiet`：退出码 0；无失败测试，默认 ignored 场景保持 ignored。
- `cargo clippy --locked --workspace --all-targets -- -D warnings`：PASS。
- `py -3 tools/code-quality.py`：PASS（689 files，62 项既有规模债务未增长）。
- `bash tools/compliance-doctor.sh`：PASS（upstream/license/copy）。

## 3. 证据边界

以下内容本次没有被宣称完成：

- 没有在本机真实注册/运行 Windows SCM 服务；当前 shell 不是管理员，因此没有 IP01–IP24 原生产品验收。
- Windows Inno 安装器仍是未签名、未安装候选；没有把受控 Host gate 当成安装包发布证据。
- Linux `systemd` 和 macOS `launchd` 的 updater 适配仍 fail closed，尚未达到跨平台发布条件。
- 没有证明服务账户、ACL、逐 job 文件/网络/凭据/会话隔离，也没有证明真实 provider、跨设备 TLS 信任或完整 Web 联合流程。
- 旧的 `0c7722e` worktree receipt 不用于本次结论；本报告只对应当前源码和 `cce9ea6`。

## 4. 后续边界

下一阶段应从管理员环境取得真实 Windows 安装/服务/升级演练证据，并分别实现和验收 Linux/macOS 原生服务适配；在这些证据出现前，产品状态仍是“控制面与 updater 组件完成，完整独立部署产品未发布”。
