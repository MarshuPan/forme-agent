# Linux systemd 受管 worker adapter 验收记录

日期：2026-10-25  
实现提交：`f2560da`  保障分支：`shu/installed-product`

## 1. 本次闭合范围

本次实现的是 Linux 平台的受管 worker 组件，不是 Linux 安装包、systemd 服务安装器或完整 IP01–IP24 产品验收。正式 managed Host 在 Linux 上可以选择 `LinuxSystemdWorkerIsolation`；开发入口仍使用显式的 development process adapter，未改变既有生产 fail-closed 边界。

组件入口要求全部成立，否则不加载 adapter：

- Host 以 root 运行，并且当前进程属于固定 `forme-host.service`；
- `/usr/bin/systemd-run`、`/usr/bin/systemctl`、`/usr/bin/id` 和 `forme-exec` 账户存在；
- runtime root 与所有 workspace 是绝对路径、无链接祖先、互不重叠；
- worker launch profile 仍来自受保护的 managed worker 配置，且执行 spec 必须绑定原 job、permit、plan digest、scope、deadline 和 nonce。

## 2. 执行与回写边界

每个 job 都建立独立的运行目录和 workspace 副本。systemd transient unit 以 `forme-exec` 运行，使用 `NoNewPrivileges`、私有临时目录、受保护系统目录、只读 runtime 输入、限定可写 workspace、Unix-only address family、cgroup/systemd 生命周期以及兄弟 job 路径隔离。Linux 当前没有已批准的 scoped network adapter，带网络 shell 会在 admission 前拒绝。

worker 不直接写原 workspace。Host 保存原始树的文件身份、内容摘要和执行绑定；worker 结束后重新捕获原树与副本，只有在原树未变化、变更集仍在最终动作允许范围内、overlay 身份和 intent/receipt 校验全部通过时才执行回写。回写使用临时文件和原子 rename；任何冲突、权限/控制命令失败、部分回写后的最终核验失败或结果不明，都保留 intent/运行目录并返回 `Unknown`，恢复不得删除副本或重新执行副作用。

## 3. 已验证证据

- `cargo check -p forme-execution --lib --target x86_64-unknown-linux-gnu`：PASS；
- `cargo test --workspace --all-targets`：PASS，退出码 0；
- `cargo fmt --all -- --check`：PASS；
- `cargo clippy --workspace --all-targets -- -D warnings`：PASS；
- `py -3 tools/code-quality.py`：PASS；
- `bash tools/compliance-doctor.sh`：PASS；
- `git diff --check`：PASS。

workspace 回归中的真实平台测试仍按各自前置条件保持 ignored；它们不能被 Linux 目标交叉编译替代。

## 4. 未宣称完成的范围

当前验收主机是 Windows，未在 Linux 主机上取得 root、systemd、`forme-host.service`、`forme-exec`、cgroup、网络拒绝和真实 Host 回写的原生运行证据。因此以下仍保持未验收：

- Linux systemd unit 安装、账户创建、目录/ACL 初始化、开机启动和升级/卸载演练；
- Linux 原生 worker 的真实成功、失败、取消、宿主崩溃恢复和回写冲突场景；
- Linux 平台 IP10/IP24 以及 IP01–IP24 联合产品验收；
- macOS LaunchDaemon/账户/sandbox adapter。

这项提交闭合的是 Linux 受管 worker 的代码骨架、平台边界和 Host overlay 语义；不把交叉编译或 Windows 上的通用回归写成 Linux 原生产品通过。
