# Windows 本地开发签名与原生候选回执（2026-10-30，`main@433039b`）

本回执记录本轮基于当前 `main` 生成的 Windows 候选。它只用于本机受控验收，不是正式发布信任，也不代表已安装、已升级或 IP01–IP24 已通过。

## 1. Release envelope

| 项目 | 结果 |
|---|---|
| 源码提交 | `433039bc2478ee5dbec7eaa2f76181c04e9ab89e` |
| release id | `20261030-433039b` |
| target | `windows_x86_64` |
| bundle | `target/product-bundles/forme-windows-x86_64-20261030-433039b` |
| signed release | `target/product-bundles/forme-windows-x86_64-20261030-433039b-signed-local-dev-20261030` |
| release digest | `sha256:dabaa6bd81e56b1bf69ef4bd836a320424827e96ad41471285f304414b557962` |
| key id | `local-dev-20261029` |
| compatibility | config `1..1`，data `10..10`，minimum upgrader `1` |

`target/release` 构建、18-file bundle、Ed25519 signed release 和当前 `forme-hostd --verify-release` 均通过。verifier 输出为 `signature_and_closed_content`，退出码为 `0`。私钥与 Ed25519 trust config 仍在仓库外，未进入源码、bundle、日志或本回执。

## 2. Native installer

原生候选使用固定 Inno Setup 6.6.1 工具链编译，编译 receipt 的 `native_compilation=performed`、`native_installation=not_performed`、`release_ready=false`。

| 项目 | 结果 |
|---|---|
| unsigned installer | `forme-0.0.1-windows-x86_64.exe` |
| unsigned bytes / SHA-256 | `73,576,278` / `sha256:dec05d1941366c65b2ba8b2257826877a7bea3c1fff6d34fa6b0252585a3ffcf` |
| local signed installer | `forme-0.0.1-windows-x86_64-local-dev-v2.exe` |
| signed bytes / SHA-256 | `73,577,824` / `sha256:c77e4a1516bebb47208420bdfd52deede6949a3bf3b56c0d0ca275b59600fe1f` |
| certificate subject | `CN=ForMe Local Development Package Signing 20261030 v2` |
| certificate issuer | `CN=ForMe Local Development Package Root 20261030 v2` |
| leaf thumbprint | `3A6241B8942F2CB07A342C76FB9A5D5CF68CB2B1` |
| Authenticode | `Authenticode / UnknownError / unverifiable` |
| native installation | `not_performed` |

签名工具独立读取 `Get-AuthenticodeSignature`，签名者指纹与请求证书一致。当前 Windows 信任提供程序仍未把本地 v2 Root 解析为 `Valid`，因此不能把 `UnknownError` 提升为可信，也不能把该候选称为正式发布包。

## 3. 当前安装实例只读状态

- Product URL：`http://127.0.0.1:18765/`。
- `/api/v1/bootstrap/status`：HTTP `200`，`claimed=true`、`setup_needed=false`。
- `forme-host`、`forme-web`：Running / Automatic。
- `forme-brain`、`forme-release-updater`：Stopped / Manual。
- 当前 Web 浏览器为未登录状态；没有读取、记录或代填 owner 密码。

以上只读结果不证明当前安装已切换到 `20261030-433039b`。没有执行 Inno 安装器、release inbox staging、服务 image 切换、备份、升级、回滚、配置写入或 Brain 启动。

## 4. 未关闭边界

本回执没有关闭：

- 真实 installation binding、active release、实际 config/data schema 与受保护 trust config 的读取；
- owner 登录后的 exact upgrade operation、排空、备份、原子切换和健康恢复；
- BrainReady、真实 provider、认证后的完整 Web 工作台和 worker/desktop helper 安装后证据；
- Windows 原生 IP01–IP24、Linux/macOS 原生服务和正式发布签名。

下一步必须由 owner 在本地 Product URL 登录后，针对本候选的精确 release digest 重新核对并批准；不得直接运行安装器覆盖现有安装根。
