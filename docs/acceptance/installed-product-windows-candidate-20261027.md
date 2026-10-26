# Windows 六程序原生安装候选

2026-10-27，`shu/installed-product` 的实现源码为 `7cb5b8bee988bdc899e9e99110908c904fdebc12`。构建期间只有验收文档和待审设计草案编辑，没有编译输入变更。该候选包含真实有限文件 worker、离线备份核验与活动发布的程序绑定；不是安装或 IP01–IP24 PASS 报告。

## 构建和独立核对

| 项目 | 结果 |
|---|---|
| 本机 | Windows `10.0.26200.0`，x64；非管理员；核对时没有 `forme-*` SCM 服务。 |
| Release | `cargo build --release --locked --workspace` PASS，1m 37s。 |
| 完整 bundle | `forme-windows-x86_64-20261027-7cb5b8b`；18 个文件，含六个程序、静态 Web 与许可资源；development=true。 |
| Bundle content digest | `0942bea4f9edea35c0b90ae851ccb920bf506666991df850171d33a948a9fde4`；编译前后、独立 `verifyBundle(resolve(path), true)` 均核对闭合集、摘要和二进制头，PASS。 |
| 固定 native compiler | Inno Setup 6.6.1；ISCC SHA-256 `f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc`；工具在运行前重新核对。 |
| 原生编译 | exit 0，使用独立编译输入快照，未改变源 bundle。 |
| 安装器 | `forme-0.0.1-windows-x86_64.exe`，73,225,411 bytes。 |
| 安装器 SHA-256 | `8703938b3b2e24f13b759583838cb70b3632c8de7ef4b400a6aaea5f0ecc6658`；receipt 与独立 `Get-FileHash` 一致。 |
| 签名/安装/发布 | Authenticode `NotSigned`；`native_package_signature=not_performed`、`native_installation=not_performed`、`release_ready=false`。 |

六个固定产品程序为 `forme-hostd`、`forme-webd`、`forme-gatewayd`、`forme-cli`、`forme-hostctl`、`forme-executord`。测试/fixture binaries 没有作为产品程序打包，没有 owner 数据、模型配置或密钥。

## 本地材料与复现

```text
target/product-bundles/forme-windows-x86_64-20261027-7cb5b8b/
target/product-bundles/forme-windows-x86_64-20261027-7cb5b8b-package/native-build-receipt.json
target/product-bundles/forme-windows-x86_64-20261027-7cb5b8b-package/artifacts/forme-0.0.1-windows-x86_64.exe
target/installed-file-worker-evidence/current-release-build.log
target/installed-file-worker-evidence/current-bundle.log
target/installed-file-worker-evidence/current-native-build.log
```

先运行资源预检，再运行下列命令。output 必须尚不存在；以上材料已经使用这些目录，不能覆盖重跑：

```powershell
cargo build --release --locked --workspace
node tools/build-product-bundle.mjs --target windows-x86_64 --release-id forme-windows-x86_64-20261027-7cb5b8b --binary-dir C:\code\forme\target\release --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261027-7cb5b8b --development
node tools/build-native-package.mjs --bundle C:\code\forme\target\product-bundles\forme-windows-x86_64-20261027-7cb5b8b --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261027-7cb5b8b-package --toolchain C:\code\forme\target\native-toolchain-95b4b6a39c9a48d39819f6a262a07a3c\toolchain.json --version 0.0.1 --allow-development
```

手工独立验证时，首次直接传入未规范化的 Windows 路径被验证器拒绝；使用 `node:path.resolve` 得到精确原生路径后验证通过，没有修改验证规则或 bundle 文件。

## 未完成的原生证据

本轮没有启动安装器、创建/启动/移除服务、改账户/ACL/防火墙、迁移实际数据或发布。旧 2026-10-24/26 候选保留；新候选不继承它们的任何安装结论。

实际 SCM/LocalSystem、冷启动/注销、完整工具权限隔离、登录会话 helper、原生升级/回滚/卸载和正式签名均未由本次构建证明，IP01–IP24 不提升。后续实际操作须按[本机原生验收计划](installed-product-windows-native-acceptance-plan.md)确认具体候选及权限；桌面实现的额外 Win32 边界仍为[待审草案](../architecture/proposals/installed-windows-desktop-helper-native-boundary.md)。Linux/macOS 保持既有有限恢复和不发布边界。
