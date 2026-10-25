# Windows AppContainer adapter 组件验收

日期：2026-10-25。分支：`shu/installed-product`。实现提交：`d0fb681`；边界文档：`2a0c289`。范围：PRD24 §4.1.2 与 canonical §29.9 的原生平台边界。owner 已明确批准限定 Win32 FFI 例外，并选择每 job 私有副本/overlay、Host 核验后回写；本组件提交尚不实现该后续方案，不代表通过 IP10/IP24。

## 1. 实现与审查范围

- 只在两个明列 adapter 文件封装 `windows-sys =0.61.2`。crate 根和 `installed` 使用 `deny(unsafe_code)`，其余入口继续 `forbid`；结构门拒绝第三个 unsafe 例外，并有拒绝路径回归。
- 每 job identity 绑定 runtime 路径、job 与 plan digest；挂起进程通过 `PROC_THREAD_ATTRIBUTE_JOB_LIST` 在创建时加入确定性命名的 kill-on-close Job Object。核验 AppContainer SID、精确 Low integrity SID、空 capability 集合和受限 privilege 后才允许 resume。
- ACL 以 lease intent → 原 file identity 检查 → DACL mutation → readback 记录。grant 必须证明请求位存在，revoke 必须证明请求位已移除；撤权成功的 `target_access_effective` 为 false。
- null 与 `INVALID_HANDLE_VALUE` 都不能转换成 `OwnedHandle`。Job 退出以固定单调时限内的实际 `ActiveProcesses == 0` 确认，不把 Job 发信号视为进程树已经退出。
- 启动恢复只处理原命名 Job 和原 lease。未完成 Grant 按可能已授予处理，先终止原树，再撤权；不重新授予或重放 worker。只有明确删除成功或精确的平台不存在 HRESULT 才接受 profile 清理；目录删除前复核绑定 identity。
- 原 lease 无活动 worker 接管、setup 清理失败、observe/cancel 出错时阻断后续准入；只有重新打开 Host 并核验原 lease 恢复后才能解除，不以新 job 绕开未知态。
- `WorkerRuntimeConfig::open_windows_appcontainer_host` 是显式平台装配入口，默认生产 Host 仍使用 `UnavailableWorkerIsolation`。

以上是源码实现和静态审查范围；未取得真实运行证据的路径不能据此声明已验证。

## 2. 组件证据

| 检查 | 当前结果 |
|---|---|
| AppContainer 专项默认测试 | 10 passed / 0 failed / 1 ignored；确定性 identity、有限环境、句柄失败路径、Low SID 校验与空 capability buffer 的单元回归、真实 NTFS DACL grant/revoke/repeated-revoke、query-only 空 Job 回收、未接管 lease 的准入拒绝、无回执 grant 的重启恢复幂等 |
| 结构检查器回归 | 7/7 PASS，含第三文件 unsafe/allow/expect 拒绝 |
| 规模与依赖方向 | PASS，700 files / 62 existing size debts；未扩大历史基线 |
| `tools/verify-code-quality.ps1` | PASS；Rust/strict Clippy、前端 77/77、资源/打包/NTFS/firewall 组件及完整合规通过；最终追加的恢复/准入场景已另行通过，当前源码的完整 M5 回归仍待完成 |
| `tools/verify-installed-host.ps1` | PASS，147 个命名场景均实际执行一次，含本批 10 项；终线 IP01–IP24 仍为 NOT_RUN |
| M5 长链 | 未通过：S68 真实浏览器 fixture 预期 mutation 2、实测 1；同源码独立复跑 S68 通过（11.10s），不抹去首次失败，也不等于长链整体通过。完整 workspace check/test 已在长链前段通过；S70–S99 本次未执行到 |

NTFS 场景只操作新建临时目录，使用派生 SID，不创建 AppContainer profile；它证明 DACL mutation 和 lease readback 一致，**不证明真实低完整性 worker 能读写**。恢复测试在真实 grant 后省略回执，再重新打开 journal，证明撤权、目录回收和第二次启动无重复写入；没有杀死真实 worker 或模拟完整服务崩溃。query-only Job 测试没有启动 worker。`existing_profile_is_rejected_before_acl_or_process_setup` 保持显式 ignored，未计为通过。

本轮环境只读核验：Windows build 26200，当前进程非管理员，四个固定 forme SCM 服务均未注册。

## 3. 尚未闭合的边界

Windows Mandatory Integrity Control 独立于 DACL：给现有 medium-integrity workspace 增加 AppContainer ACE，不能证明 Low worker 有写权限。owner 已选择 A：按 job 创建私有副本/overlay，由 Host 核验后回写。`d0fb681` 没有改原 workspace 的完整性标签，副本、冲突检查、回写授权和中断恢复须在后续提交实现并单独取证；该方案不再是待决选项。

还没有实际 AppContainer worker 的获准读写、兄弟目录/brain/key/Web-auth/管理路径拒绝、可写文件禁止执行、网络/loopback 拒绝、token/后代 Job custody，以及崩溃后原生恢复的完整正反证据。现有注册程序/DLL 和 runtime 根的实际权限也须由安装身份单独验证。

没有本轮管理员 SCM 安装、LocalSystem broker/core peer identity、会话 helper、跨平台原生隔离或签名包验收。Windows/Linux/macOS 原生支持、IP01–IP24 和整体产品里程碑均未因此关闭；尤其 IP10/IP24 不记 PASS。默认 worker dispatch 继续 blocked。
