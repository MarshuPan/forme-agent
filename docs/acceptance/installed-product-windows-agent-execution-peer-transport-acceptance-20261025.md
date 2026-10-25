# Windows AgentExecution peer transport 组件验收

日期：2026-10-25。分支：`shu/installed-product`。实现提交：`2f5f6a9`；Windows fail-closed 回归提交：`b901dff`。

本报告记录 managed Windows Host 的 Core -> AgentExecution 原生 peer transport 接缝。它承接 architecture/10 §7.2、canonical §29.9 和 PRD24 §4.1；不把组件证据提升为真实 SCM 安装、完整 worker sandbox 或 IP01–IP24 通过。

## 1. 已实现

- managed Windows Host 不再为 AgentExecution 开放 loopback TCP；开发模式继续保留 TCP 以支持协议测试。
- pipe 名称由 `install_id` 经固定摘要确定性派生，Core 只能使用受校验的安装端点；维护域仍使用独立的 HTTP listener 和凭据域。
- named pipe 使用 `PIPE_REJECT_REMOTE_CLIENTS`，DACL 只授予 `LocalSystem` 与 `NT SERVICE\\forme-brain`；不接受请求正文自报身份作为 peer 证明。
- Host 在 HTTP framing 之前读取连接的 client PID，核验 Brain Service SID membership，并将实际进程 image path 与 active release 的 `forme-gatewayd.exe` 做 canonical path 比对。任一证明失败只拒绝当前连接，不降级到 TCP 或普通子进程。
- pipe server 使用阻塞 accepted instance 交给既有 HTTP framing；监听、并发上限、停机排空和 AgentExecution bearer 校验仍复用既有 Host 生命周期与控制面。
- 新增的 Win32 FFI 仅位于授权的 `windows_agent_execution_native.rs` 边界；其他业务和协议代码继续 `forbid(unsafe_code)`。没有新增 crate、内部依赖边或脑 EventKind。

## 2. 源码验证

| 门 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo test -p forme-execution --all-targets` | PASS，134 passed、0 failed、1 ignored；managed Host 4/4 场景通过 |
| `cargo test --workspace --all-targets` | PASS，退出码 0，无失败测试 |
| `powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-installed-host.ps1` | PASS；宿主命名门全通过，明确输出 `IP01-IP24: NOT_RUN` |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `py -3 tools/code-quality.py` | PASS，710 files、62 existing size debts |
| `bash tools/compliance-doctor.sh` | PASS：upstream、license、copy |
| `git diff --check` | PASS；仅有 Git 的 LF/CRLF 提示 |

Windows 下的 pipe 名称闭合性、安装绑定和协议编译随 `forme-execution` 测试覆盖；Windows 专属反向场景还证明：未注册真实 `NT SERVICE\\forme-brain` 时，managed Host 在打开 listener 前失败退出，不回退 TCP、不创建管理 journal，替换 manifest 也不能绕过该失败。既有正向 TCP fixture 在 Windows 上不冒充真实服务身份，因此按平台条件不运行。

## 3. 未闭合边界

当前主机不是管理员，未注册或启动 `forme-brain`/`forme-host` SCM 服务，也没有可用于验收的真实 `NT SERVICE\\forme-brain` peer。因此以下内容仍未验证：

- LocalSystem Host 与真实 Brain Service SID 的双向正反连接；
- active release image、服务账户、pipe DACL 和远程拒绝在实际安装布局中的联合证据；
- Linux/macOS 原生 peer transport；
- 每 job 文件/网络/credential/session 隔离、完整安装包、升级/恢复和 LAN/Web 联合流程；
- IP01–IP24，尤其 IP10、IP24。

所以本提交只关闭“managed Windows AgentExecution peer transport 已有实现和源码门”的组件状态；生产 worker 仍遵守既有 fail-closed 条件，不能据此宣称完整安装产品或 Windows 原生支持已验收。
