# macOS LaunchDaemon 受管 worker 验收记录

日期：2026-10-25  
实现提交：`558c7e7`  保障分支：`shu/installed-product`

## 1. 本次闭合范围

本次实现的是 macOS 平台受管 worker 的代码适配，不是 macOS 安装包、LaunchDaemon 注册或完整 IP01–IP24 产品验收。正式 managed Host 在 macOS 上可以选择 `MacOsLaunchdWorkerIsolation`；开发入口仍使用显式的 development process adapter，未把普通进程提升为系统服务 worker。

组件入口要求全部成立，否则不加载 adapter：

- Host 以 root 运行，并且当前进程属于固定 `com.forme.host` LaunchDaemon；
- `/bin/launchctl`、`/usr/bin/sandbox-exec`、`/usr/bin/id` 和 `_formeexec` 执行账户存在；
- runtime root 与所有 workspace 是绝对路径、无链接祖先、互不重叠；
- worker launch profile 来自受保护的 managed worker 配置，执行 spec 必须绑定原 job、permit、plan digest、scope、deadline 和 nonce。

## 2. 执行与回写边界

每个 job 建立独立的 LaunchDaemon plist、运行目录、临时目录和 workspace 副本。worker 以 `_formeexec` 运行，入口经过固定的 `sandbox-exec` profile：默认拒绝，仅读取系统运行时、该 job 的输入和程序，读写私有 workspace/tmp，当前没有 scoped network adapter 时拒绝网络 shell。LaunchDaemon 的 label 由 job 和 plan digest 确定性派生，控制操作只使用固定 `launchctl` 子命令。

worker 不直接写原 workspace。Host 保存原始树的文件身份、内容摘要和执行绑定；worker 结束后重新捕获原树与副本，只有在原树未变化、变更集仍在最终动作允许范围内、overlay 身份和 durable intent/receipt 校验全部通过时才执行回写。冲突、权限/控制命令失败、部分回写后的最终核验失败或结果不明，都保留 intent/运行目录并返回 `Unknown`，恢复不得删除副本或重新执行副作用。

## 3. 已验证证据

- `cargo fmt --all -- --check`：PASS；
- `cargo check -p forme-execution --lib`：PASS；
- `cargo test -p forme-execution --lib`：134 passed / 0 failed / 1 ignored；
- `cargo clippy -p forme-execution --all-targets -- -D warnings`：PASS；
- `py -3 tools/code-quality.py`：PASS；
- `bash tools/compliance-doctor.sh`：PASS；
- `git diff --check`：PASS；
- 生产文件行数符合当前 600 行棘轮：macOS adapter 598 行，sandbox helper 182 行。

尝试使用 `cargo check -p forme-execution --lib --target x86_64-apple-darwin` 时，当前 Windows 环境没有安装该 Rust target，返回 `can't find crate for core/std`；这不是 macOS 编译通过证据。

## 4. 未宣称完成的范围

当前验收主机是 Windows，未在 macOS 主机取得 root、LaunchDaemon、`_formeexec`、sandbox、TCC、网络拒绝、安装服务或真实 Host 回写的原生运行证据。因此以下仍保持未验收：

- macOS `.pkg` 安装、账户创建、目录/ACL 初始化、LaunchDaemon 注册、开机启动和升级/卸载演练；
- macOS 原生 worker 的真实成功、失败、取消、宿主崩溃恢复和回写冲突场景；
- `sandbox-exec` 在受支持 macOS 版本上的实际文件/进程/网络拒绝正反断言；
- macOS TCC、桌面会话 helper、签名与公证；
- macOS 平台 IP10/IP24 以及 IP01–IP24 联合产品验收。

这项提交闭合的是 macOS 受管 worker 的代码装配、账户/服务边界表达、sandbox 配置生成和 Host overlay 语义；不把 Windows 上的通用回归、Rust 源码检查或交叉编译失败写成 macOS 原生产品通过。
