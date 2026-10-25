# Native Package Hook Hardening Acceptance

日期：2026-10-25  
实现提交：`277f551` (`fix(packaging): bound native transaction environment`)

## 1. 范围

本批只加固 POSIX 原生包事务入口：

- Linux `preinst`、`postinst`、`prerm` 和 RPM `%pre`/`%post`/`%preun` 显式设置固定系统 `PATH` 与 `umask 077`。
- Linux 包事务在继续前显式要求 root，并核验 `/usr/bin/systemctl` 存在且可执行。
- macOS `preinstall`、`postinstall` 显式设置固定系统 `PATH` 与 `umask 077`。
- 卸载测试 fixture 显式模拟 root；测试仍只替换 `systemctl` 边界，不写宿主 `/etc`、不注册服务。

这保持既有服务顺序、账户矩阵、数据保留范围、升级阻断和有限恢复策略不变。没有新增 crate、依赖、EventKind、服务名或维护命令。

## 2. 验证

通过：

- `node --test tools/tests/posix_packaging.test.mjs`：8/8。
- `cargo fmt --all -- --check`。
- `cargo check --workspace --all-targets`。
- `cargo test --workspace --all-targets`：退出码 0；既定 ignored 场景保持原状态。
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`。
- `py -3 tools/code-quality.py`。
- `py -3 -m unittest tools.tests.test_code_quality`：6/6。
- `bash tools/compliance-doctor.sh`。
- `git diff --check`。

定向测试覆盖 shell 语法、root/systemd 前置声明、Linux 服务卸载顺序与失败停止、POSIX 服务可写目录边界、TLS/Web 配置读权限，以及禁止无界服务重启策略。

## 3. 证据边界

本批没有执行真实 Linux/macOS 包管理器、systemd/launchd、原生账户创建、管理员安装、包签名/公证或跨设备浏览器流程。它不证明 `.deb`、`.rpm`、`.pkg` 可发布，不解除 Windows AppContainer/worker blocked，也不提升 IP01–IP24 或 IP24 状态。

fixture 中的 root 身份只用于验证脚本控制流，不能替代目标系统 root 事务证据。真实安装仍必须经过对应平台的安装包、服务管理器、账户/ACL、签名和联合产品验收。
