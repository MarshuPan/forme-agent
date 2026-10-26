# Windows 原生安装候选更新

2026-10-26，在 `shu/installed-product` 上为实现提交 `5391750e6f968e5690692636a8797c14206eba53` 的源码重新构建 Windows x86_64 候选。该提交包含窄屏导航修复；构建期间只有验收文档与未激活草案提交，没有其他编译输入修改。本报告不覆盖真实安装或 IP01–IP24。

## 1. 构建与独立核对

| 项目 | 结果 |
|---|---|
| 主机 | Windows `10.0.26200.0`，x64；当前 token 非管理员。 |
| Rust | `cargo build --release --locked --workspace` PASS，2m 20s。 |
| Bundle | `forme-windows-x86_64-20261026-5391750`，17 个文件，development=true。 |
| Bundle content digest | `2b2a416db37fe08ce83baf94f2e4b589ff3b9999e61aebbbbe456ed88224d119`；构建后另用 `verifyBundle` 重新核对闭合集、文件摘要和二进制头，PASS。 |
| Native compiler | Inno Setup 6.6.1；ISCC SHA-256 为 `f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc`，运行前独立核对一致。 |
| 原生编译 | PASS，exit 0；编译器使用 bundle 的独立快照，编译前后源 bundle 核验一致。 |
| 安装器 | `forme-0.0.1-windows-x86_64.exe`，65,564,782 bytes。 |
| 安装器 SHA-256 | `3acfe724a4c9f3a82faf687c16660f243eae1fec39e23a383deb97ac8068b957`；receipt 与独立 `Get-FileHash` 一致。 |
| 签名/安装 | Authenticode=`NotSigned`；receipt 为 `native_package_signature=not_performed`、`native_installation=not_performed`、`release_ready=false`。 |

本地候选保留在 ignored 构建目录：

```text
target/product-bundles/forme-windows-x86_64-20261026-5391750/
target/product-bundles/forme-windows-x86_64-20261026-5391750-package/native-build-receipt.json
target/product-bundles/forme-windows-x86_64-20261026-5391750-package/artifacts/forme-0.0.1-windows-x86_64.exe
```

源码、嵌入 UI 与打包模板均来自本仓库。没有读取或打包实际 owner 数据、模型配置或密钥。旧的 2026-10-24 候选及其报告保留，不覆盖或删除。

## 2. 复现命令

先执行 `py -3 tools/build_resources.py check`，再运行 release build。以下两个 output 必须是尚不存在的新目录；示例已用于本次证据，不能原样重复覆盖。

```powershell
node tools/build-product-bundle.mjs --target windows-x86_64 --release-id forme-windows-x86_64-20261026-5391750 --binary-dir C:\code\forme\target\release --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261026-5391750 --development
node tools/build-native-package.mjs --bundle C:\code\forme\target\product-bundles\forme-windows-x86_64-20261026-5391750 --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261026-5391750-package --toolchain C:\code\forme\target\native-toolchain-95b4b6a39c9a48d39819f6a262a07a3c\toolchain.json --version 0.0.1 --allow-development
```

## 3. 明确未验证

本机只做编译和只读核验，没有启动安装器，没有创建/启动/移除系统服务；复核时注册的 `forme-*` 服务为零。该候选仍是未签名、未安装、不可发布的开发材料，不是正式 ReleaseManifest 或平台通过证据。

真实 SCM/LocalSystem、服务账户/ACL、安装后 core→worker 结果链、桌面会话、升级/回滚/卸载、正式签名与完整 IP24 均未由本次构建证明。其他 OS/CPU 也没有本次原生证据。IP01/IP02/IP03/IP10/IP24 不提升；当前剩余义务继续以整体交付清单为准。
