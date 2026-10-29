# Windows 桌面 Helper 当前源码实现回执（2026-10-30）

本回执记录 `main@566dfe7` 已合入的 Windows 登录会话 helper 实现及当前源码层回归。它证明生产装配代码存在且保持治理边界，不等于真实安装服务、真实登录会话桌面动作或 IP11/IP24 已通过。

## 1. 当前实现

受管 Windows Host 在存在当前活动 release 的 `forme-executord.exe` 时，通过 `open_managed_windows_host_with_desktop_helper` 装配 `WindowsDesktopHelperAdapter`。固定执行入口为：

```text
forme-executord --desktop-session-helper --pipe <Host-created-pipe>
```

实现分工如下：

- `windows_desktop_session_native.rs`：只查询真实 WTS session、活动/解锁状态、用户 SID、token session id 和 AuthenticationId；只用固定活动 release image，以登录用户 token 创建 suspended helper，并在恢复前放入 kill-on-close Job Object。
- `windows_desktop_helper_peer_native.rs`：只创建 grant 绑定的私有 named pipe；核对 helper PID、进程 image、用户 token、session、AuthenticationId 和交互桌面；执行有界 framing、超时和断开处理。
- `windows_desktop_helper_runtime.rs`：safe Rust 协议、Hello/Ready/Ping/Execute/Receipt、Computer action 范围检查；helper 不读取 brain、模型、secret、安装配置或任意内容，并拒绝截图、secret/content 注入和超出范围的动作。
- `desktop_helper.rs` 与 `worker_host.rs`：复用既有 `WorkerJob`、`WorkerPermit`、grant、plan digest、nonce、capability、TTL、journal、receipt 和 Unknown 语义。生产装配失败或未满足平台条件时继续 fail closed，不回退到高权限 Host 直接操作桌面。

没有新增 crate、EventKind 或依赖边。两个 Windows 原生文件属于已批准的审计边界；其它装配和治理代码保持 `unsafe_code` 禁止。

## 2. 当前源码证据

在当前工作树重新执行：

```text
py -3 tools/build_resources.py check
=> PASS (48.4 GiB free)

cargo fmt --all -- --check
=> PASS

cargo test -p forme-execution --all-targets --locked
=> 161 passed / 0 failed / 1 ignored (library)
   19 passed / 0 failed (forme-hostd)
   9 passed / 0 failed (execution_contract)
   2 passed / 0 failed (bootstrap transport)
   20 passed / 0 failed (installed host contract)
   11 passed / 0 failed (installed worker transport)
   14 passed / 0 failed (installed worker contract)
   11 passed / 0 failed (installed worker backend)
   5 passed / 0 failed (process worker)
   4 passed / 0 failed (managed host transport)
   remaining execution targets: PASS
```

桌面相关的当前源码负向覆盖包括：固定 desktop reference、受限 session reference、named-pipe SID 形状校验、grant/capability/plan 绑定、过期和错误 session 拒绝、helper 失联后的 Unknown 以及禁止重复 dispatch。

## 3. 尚未取得的证据

以下项目仍保持未验证，不能从本回执升级：

- 当前 `main` release 进入真实安装目录后的 Host 装配和服务账户运行；
- LocalSystem Host 对当前实际登录用户 token 的真实启动；
- 一次真实但有限的桌面动作及其 helper receipt 到原 WorkerJob 终态的完整链路；
- 锁屏、注销、重登录、会话切换、错误 peer、升级排空、Host/helper 崩溃恢复；
- 当前签名 release 的原生安装、升级/回滚和正式 Authenticode 签名；
- IP11 与 IP24。

在取得上述层级证据前，桌面能力在产品状态中必须显示为未验证/不可用，不得显示为已启用；没有单独的精确操作授权，不执行真实桌面输入或替换安装服务。

